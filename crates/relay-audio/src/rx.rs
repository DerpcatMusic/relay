use core::fmt;

use relay_jitter::{
    AcceptedPacket, Playout, PushResult, RejectedPacket, ReorderBuffer, ReorderConfigError,
};
use relay_opus::{
    CHANNELS, Decoder, DecoderConfig, Error as OpusError, FrameDuration, SAMPLE_RATE_HZ,
};

use crate::{
    AudioPipelineConfig, ExtendedSequence, ExtensionError, MediaPacket, PayloadType, RtpTimestamp,
    Ssrc, extend_sequence,
};

const MAX_INTERLEAVED_SAMPLES: usize = FrameDuration::Ms20.interleaved_samples();

/// Trusted identity and RTP timeline origin for one receive-stream epoch.
///
/// A control-plane owner must authenticate and validate a source transition
/// before constructing this value or passing it to [`RxWorker::reset`]. Remote
/// packets can never change this epoch implicitly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxStreamConfig {
    /// Synchronization source accepted during this epoch.
    pub ssrc: Ssrc,
    /// Negotiated seven-bit RTP payload type.
    pub payload_type: PayloadType,
    /// Extended position corresponding to the first playout decision.
    pub initial_sequence: ExtendedSequence,
    /// Wire timestamp corresponding to `initial_sequence`.
    pub initial_timestamp: RtpTimestamp,
}

/// Construction or trusted-reset failure for [`RxWorker`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RxBuildError {
    /// The configured reorder capacity was invalid.
    Reorder(ReorderConfigError),
    /// The canonical Opus decoder could not be constructed.
    Opus(OpusError),
}

impl fmt::Display for RxBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RxBuildError {}

/// Why an owned ingress packet was returned instead of retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngressMismatch {
    /// The payload exceeded the pipeline's negotiated byte bound.
    PacketTooLarge {
        /// Configured maximum encoded bytes.
        maximum: usize,
        /// Packet's encoded byte count.
        actual: usize,
    },
    /// The packet belonged to another synchronization source.
    Ssrc {
        /// Trusted epoch value.
        expected: Ssrc,
        /// Rejected packet value.
        actual: Ssrc,
    },
    /// The packet used a different RTP payload type.
    PayloadType {
        /// Negotiated payload type.
        expected: PayloadType,
        /// Rejected packet value.
        actual: PayloadType,
    },
    /// The wire sequence was exactly half a serial range from the playout head.
    AmbiguousSequence,
    /// Extending the wire sequence would place it before the trusted epoch.
    SequenceBeforeEpoch,
    /// Extending the wire sequence exceeded the representable epoch timeline.
    SequenceOverflow,
    /// The timestamp was not the exact value implied by the extended sequence.
    Timestamp {
        /// Exact wrapping timestamp implied by the epoch.
        expected: RtpTimestamp,
        /// Rejected packet value.
        actual: RtpTimestamp,
    },
    /// The encoded Opus duration differed from the negotiated duration.
    Duration {
        /// Negotiated samples per channel.
        expected_samples_per_channel: usize,
        /// Encoded samples per channel.
        actual_samples_per_channel: usize,
    },
    /// Stateless libopus packet inspection rejected the payload.
    MalformedPacket,
    /// An identical sequence is already buffered or was recently handled.
    Duplicate,
    /// The packet arrived after its deadline.
    Late {
        /// Sequence positions behind the current playout head.
        distance: u16,
    },
    /// The packet was valid but outside fixed reorder storage.
    AheadOfWindow {
        /// Sequence positions ahead of the current playout head.
        distance: u16,
    },
}

/// Small, copyable disposition for one ingress call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngressStatus {
    /// The next wanted packet was retained.
    AcceptedInOrder,
    /// A packet ahead of the playout head was retained.
    AcceptedReordered {
        /// Number of skipped sequence positions.
        depth: u16,
    },
    /// The packet was not retained.
    Rejected(IngressMismatch),
}

/// Allocation-free owned result of an ingress call.
///
/// Rejections return the original packet without cloning its fixed payload.
#[derive(Debug)]
#[must_use]
pub struct IngressOutcome {
    status: IngressStatus,
    returned_packet: Option<MediaPacket>,
}

impl IngressOutcome {
    /// Returns the copyable packet disposition.
    #[must_use]
    pub const fn status(&self) -> IngressStatus {
        self.status
    }

    /// Borrows the original rejected packet, when present.
    #[must_use]
    pub const fn returned_packet(&self) -> Option<&MediaPacket> {
        self.returned_packet.as_ref()
    }

    /// Moves out the original rejected packet, when present.
    #[must_use]
    pub fn into_returned_packet(self) -> Option<MediaPacket> {
        self.returned_packet
    }

    fn accepted(status: IngressStatus) -> Self {
        Self {
            status,
            returned_packet: None,
        }
    }

    fn rejected(reason: IngressMismatch, packet: MediaPacket) -> Self {
        Self {
            status: IngressStatus::Rejected(reason),
            returned_packet: Some(packet),
        }
    }
}

/// Fixed inline PCM storage for one maximum-duration stereo frame.
///
/// Only [`Self::samples`] is initialized for the current negotiated duration.
/// The same inline storage is reused between calls while the containing worker
/// remains at the same address; moving the worker also moves this storage.
#[derive(Clone)]
pub struct PcmFrame {
    samples: [f32; MAX_INTERLEAVED_SAMPLES],
    len: usize,
}

impl PcmFrame {
    /// Empty frame; fill with [`Self::copy_from_interleaved`].
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            samples: [0.0; MAX_INTERLEAVED_SAMPLES],
            len: 0,
        }
    }

    /// Returns the exact interleaved stereo frame prefix.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples[..self.len]
    }

    /// Returns samples per channel in this frame.
    #[must_use]
    pub const fn samples_per_channel(&self) -> usize {
        self.len / CHANNELS as usize
    }

    /// Returns the immutable maximum scalar-sample capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.samples.len()
    }

    /// Copies one interleaved stereo frame into this buffer.
    ///
    /// # Errors
    ///
    /// Returns `false` when `samples` is empty, not stereo-aligned, or larger
    /// than the inline capacity.
    #[must_use]
    pub fn copy_from_interleaved(&mut self, samples: &[f32]) -> bool {
        if samples.is_empty()
            || !samples.len().is_multiple_of(usize::from(CHANNELS))
            || samples.len() > self.samples.len()
        {
            return false;
        }
        self.samples[..samples.len()].copy_from_slice(samples);
        self.len = samples.len();
        true
    }

    #[cfg(test)]
    pub(crate) fn from_test_samples(samples: &[f32]) -> Self {
        let mut frame = Self::empty();
        assert!(frame.copy_from_interleaved(samples));
        frame
    }
}

impl fmt::Debug for PcmFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PcmFrame")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

/// Codec path that produced an emitted frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameSource {
    /// Normal decode of the packet at this sequence position.
    Packet,
    /// FEC decode from the following packet; libopus may internally use PLC
    /// when that otherwise-valid packet carries no usable FEC data.
    InbandFecOrPlc,
    /// Explicit decoder packet-loss concealment.
    PacketLossConcealment,
}

/// Whether the requested codec operation succeeded or required error concealment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameStatus {
    /// The selected normal, FEC, or ordinary gap-PLC operation succeeded.
    Produced,
    /// A codec error was contained and replaced with PLC (or bounded silence if
    /// PLC itself failed), leaving the worker usable.
    ConcealedCodecError,
}

/// One emitted sequence position borrowing the worker's reusable inline PCM storage.
#[derive(Debug)]
#[must_use]
pub struct FrameOutcome<'a> {
    sequence: ExtendedSequence,
    timestamp: RtpTimestamp,
    source: FrameSource,
    status: FrameStatus,
    frame: &'a PcmFrame,
}

impl FrameOutcome<'_> {
    /// Returns the epoch-relative sequence position emitted by this decision.
    #[must_use]
    pub const fn sequence(&self) -> ExtendedSequence {
        self.sequence
    }

    /// Returns the exact wire timestamp for the emitted sequence.
    #[must_use]
    pub const fn timestamp(&self) -> RtpTimestamp {
        self.timestamp
    }

    /// Returns the codec path used to produce PCM.
    #[must_use]
    pub const fn source(&self) -> FrameSource {
        self.source
    }

    /// Returns whether codec-error containment was needed.
    #[must_use]
    pub const fn status(&self) -> FrameStatus {
        self.status
    }

    /// Borrows the stable, fixed-capacity PCM frame.
    #[must_use]
    pub const fn frame(&self) -> &PcmFrame {
        self.frame
    }
}

/// Saturating receive-core counters for one trusted stream epoch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RxMetrics {
    /// Packets presented to ingress.
    pub ingress_packets: u64,
    /// Packets retained at the playout head.
    pub accepted_in_order: u64,
    /// Packets retained ahead of the playout head.
    pub accepted_reordered: u64,
    /// Returned duplicate packets.
    pub duplicates: u64,
    /// Returned packets that missed their deadline.
    pub late: u64,
    /// Returned valid packets outside the fixed window.
    pub ahead_of_window: u64,
    /// Returned packets with the wrong SSRC or payload type.
    pub identity_mismatches: u64,
    /// Returned packets with an unexpected timestamp or encoded duration.
    pub duration_timestamp_mismatches: u64,
    /// Returned payloads rejected by stateless Opus packet inspection.
    pub malformed_packets: u64,
    /// Returned payloads exceeding the negotiated packet byte bound.
    pub oversized_packets: u64,
    /// Returned ambiguous, before-epoch, or overflowing sequence extensions.
    pub extension_rejections: u64,
    /// Deadline decisions staged by [`RxWorker::tick`].
    pub deadline_decisions: u64,
    /// Frames returned by [`RxWorker::tick`] or [`RxWorker::drain`].
    pub emitted_frames: u64,
    /// Emitted frames whose source is a normal packet decode.
    pub packet_frames: u64,
    /// Requests made to the following packet's FEC path.
    ///
    /// This is an operation count, not proof that LBRR data was present. A
    /// failed request may also increment `codec_errors` and `plc_frames`.
    pub fec_attempts: u64,
    /// Emitted frames whose source is explicit PLC.
    ///
    /// This can overlap `codec_errors` when PLC conceals another codec failure.
    pub plc_frames: u64,
    /// Failed codec operations contained without faulting the worker.
    ///
    /// Multiple failed operations can belong to one emitted frame, so this is
    /// intentionally not exclusive with `fec_attempts` or `plc_frames`.
    pub codec_errors: u64,
}

// The packet variant deliberately retains fixed-inline packet ownership. Boxing it
// would add allocation/deallocation to steady-state playout, which this core forbids.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "fixed inline MediaPacket keeps the preallocated RX worker allocation-free per tick"
)]
enum PendingDecision {
    Packet {
        sequence: ExtendedSequence,
        timestamp: RtpTimestamp,
        packet: MediaPacket,
    },
    Missing {
        sequence: ExtendedSequence,
        timestamp: RtpTimestamp,
    },
    Ready {
        sequence: ExtendedSequence,
        timestamp: RtpTimestamp,
        source: FrameSource,
        status: FrameStatus,
    },
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecodeCall {
    Fec,
    Normal,
    Plc,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct ScriptStep {
    call: DecodeCall,
    succeeds: bool,
}

#[cfg(test)]
struct ScriptedDecoder {
    script: Vec<ScriptStep>,
    cursor: usize,
    trace: [Option<DecodeCall>; 16],
    trace_len: usize,
}

#[cfg(test)]
impl ScriptedDecoder {
    fn new(script: Vec<ScriptStep>) -> Self {
        assert!(script.len() <= 16);
        Self {
            script,
            cursor: 0,
            trace: [None; 16],
            trace_len: 0,
        }
    }

    fn run(&mut self, call: DecodeCall, output: &mut [f32]) -> Result<(), OpusError> {
        let step = self
            .script
            .get(self.cursor)
            .copied()
            .expect("scripted decoder received an unexpected extra call");
        assert_eq!(step.call, call, "scripted decoder call order changed");
        self.trace[self.trace_len] = Some(call);
        self.trace_len += 1;
        self.cursor += 1;
        if step.succeeds {
            output.fill(self.cursor as f32);
            Ok(())
        } else {
            Err(OpusError::InvalidCodecResult)
        }
    }

    fn trace(&self) -> &[Option<DecodeCall>] {
        &self.trace[..self.trace_len]
    }
}

enum DecoderBackend {
    Opus(Decoder),
    #[cfg(test)]
    Scripted(ScriptedDecoder),
}

impl DecoderBackend {
    fn decode(&mut self, packet: &[u8], output: &mut [f32]) -> Result<(), OpusError> {
        match self {
            Self::Opus(decoder) => decoder.decode(packet, output).map(|_| ()),
            #[cfg(test)]
            Self::Scripted(decoder) => decoder.run(DecodeCall::Normal, output),
        }
    }

    fn decode_fec(&mut self, packet: &[u8], output: &mut [f32]) -> Result<(), OpusError> {
        match self {
            Self::Opus(decoder) => decoder.decode_fec(packet, output).map(|_| ()),
            #[cfg(test)]
            Self::Scripted(decoder) => decoder.run(DecodeCall::Fec, output),
        }
    }

    fn decode_plc(&mut self, output: &mut [f32]) -> Result<(), OpusError> {
        match self {
            Self::Opus(decoder) => decoder.decode_plc(output).map(|_| ()),
            #[cfg(test)]
            Self::Scripted(decoder) => decoder.run(DecodeCall::Plc, output),
        }
    }
}

/// Caller-driven bounded reorder/decode/FEC/PLC worker.
///
/// Ingress accepts only owned packets. [`Self::tick`] is the sole deadline
/// input and takes no wall-clock or arrival timestamp. The first tick stages one
/// decision and emits no PCM, establishing the one-packet lookahead required by
/// in-band FEC. No tick allocates, waits, performs I/O, or grows storage.
pub struct RxWorker {
    frame_duration: FrameDuration,
    packet_capacity: usize,
    stream: RxStreamConfig,
    reorder: ReorderBuffer<MediaPacket>,
    decoder: DecoderBackend,
    next_extended: ExtendedSequence,
    timeline_exhausted: bool,
    pending: Option<PendingDecision>,
    output: PcmFrame,
    staged: PcmFrame,
    metrics: RxMetrics,
}

impl RxWorker {
    /// Constructs fixed reorder storage and a canonical stereo 48 kHz decoder.
    ///
    /// # Errors
    ///
    /// Returns a reorder configuration or libopus construction failure.
    pub fn new(config: AudioPipelineConfig, stream: RxStreamConfig) -> Result<Self, RxBuildError> {
        let mut reorder =
            ReorderBuffer::new(config.reorder_capacity()).map_err(RxBuildError::Reorder)?;
        reorder.reset_and_rebase(stream.initial_sequence.wire().get());
        let decoder = Decoder::new(DecoderConfig::stereo_48k(config.frame_duration()))
            .map_err(RxBuildError::Opus)?;
        Ok(Self {
            frame_duration: config.frame_duration(),
            packet_capacity: config.packet_capacity(),
            stream,
            reorder,
            decoder: DecoderBackend::Opus(decoder),
            next_extended: stream.initial_sequence,
            timeline_exhausted: false,
            pending: None,
            output: PcmFrame::empty(),
            staged: PcmFrame::empty(),
            metrics: RxMetrics::default(),
        })
    }

    /// Validates and conditionally retains one owned media packet.
    ///
    /// Payload length, SSRC, payload type, sequence extension, exact timestamp,
    /// and encoded duration are all checked without mutating reorder or decoder
    /// state. Every rejection returns the original packet.
    pub fn ingress(&mut self, packet: MediaPacket) -> IngressOutcome {
        sat_inc(&mut self.metrics.ingress_packets);
        if let Err(reason) = self.validate_packet(&packet) {
            self.record_rejection(reason);
            return IngressOutcome::rejected(reason, packet);
        }

        match self.reorder.push(packet.sequence().get(), packet) {
            PushResult::Accepted(AcceptedPacket::InOrder) => {
                sat_inc(&mut self.metrics.accepted_in_order);
                IngressOutcome::accepted(IngressStatus::AcceptedInOrder)
            }
            PushResult::Accepted(AcceptedPacket::Reordered { depth }) => {
                sat_inc(&mut self.metrics.accepted_reordered);
                IngressOutcome::accepted(IngressStatus::AcceptedReordered { depth })
            }
            PushResult::Rejected { reason, packet } => {
                let reason = match reason {
                    RejectedPacket::Duplicate => IngressMismatch::Duplicate,
                    RejectedPacket::Late { distance } => IngressMismatch::Late { distance },
                    RejectedPacket::AheadOfWindow { distance } => {
                        IngressMismatch::AheadOfWindow { distance }
                    }
                    RejectedPacket::AmbiguousSerialDistance => IngressMismatch::AmbiguousSequence,
                };
                self.record_rejection(reason);
                IngressOutcome::rejected(reason, packet)
            }
        }
    }

    /// Stages one deadline decision and emits the preceding pending position.
    ///
    /// The first call always returns `None`. A missing pending position gets one
    /// packet of lookahead: when the current decision contains a packet, that
    /// packet is first used for FEC and then decoded normally into fixed staged
    /// storage for emission by the next call.
    pub fn tick(&mut self) -> Option<FrameOutcome<'_>> {
        let current = self.pop_decision();
        let Some(pending) = self.pending.take() else {
            self.pending = current;
            return None;
        };
        self.resolve(pending, current)
    }

    /// Resolves the final staged position without inventing another deadline.
    ///
    /// Repeated calls return `None` until another tick stages a position.
    pub fn drain(&mut self) -> Option<FrameOutcome<'_>> {
        let pending = self.pending.take()?;
        self.resolve(pending, None)
    }

    /// Reconstructs decoder history, then atomically adopts a trusted epoch and
    /// clears reorder, pending, PCM, and metric history.
    ///
    /// Decoder construction occurs before any live state is changed, so a
    /// failure leaves the existing worker and epoch untouched.
    pub fn reset(&mut self, stream: RxStreamConfig) -> Result<(), RxBuildError> {
        let decoder = Decoder::new(DecoderConfig::stereo_48k(self.frame_duration))
            .map_err(RxBuildError::Opus)?;
        self.decoder = DecoderBackend::Opus(decoder);
        self.reorder
            .reset_and_rebase(stream.initial_sequence.wire().get());
        self.stream = stream;
        self.next_extended = stream.initial_sequence;
        self.timeline_exhausted = false;
        self.pending = None;
        self.output.len = 0;
        self.staged.len = 0;
        self.metrics = RxMetrics::default();
        Ok(())
    }

    /// Returns a coherent caller-thread counter snapshot.
    #[must_use]
    pub const fn metrics(&self) -> RxMetrics {
        self.metrics
    }

    /// Returns the fixed reorder packet capacity.
    #[must_use]
    pub fn reorder_capacity(&self) -> usize {
        self.reorder.capacity()
    }

    #[cfg(test)]
    fn with_scripted_decoder(
        config: AudioPipelineConfig,
        stream: RxStreamConfig,
        script: Vec<ScriptStep>,
    ) -> Self {
        let mut worker = Self::new(config, stream).expect("test RX worker");
        worker.decoder = DecoderBackend::Scripted(ScriptedDecoder::new(script));
        worker
    }

    #[cfg(test)]
    fn scripted_trace(&self) -> &[Option<DecodeCall>] {
        match &self.decoder {
            DecoderBackend::Scripted(decoder) => decoder.trace(),
            DecoderBackend::Opus(_) => panic!("test worker is using libopus"),
        }
    }

    fn validate_packet(&self, packet: &MediaPacket) -> Result<(), IngressMismatch> {
        if packet.payload_len() > self.packet_capacity {
            return Err(IngressMismatch::PacketTooLarge {
                maximum: self.packet_capacity,
                actual: packet.payload_len(),
            });
        }
        if packet.ssrc() != self.stream.ssrc {
            return Err(IngressMismatch::Ssrc {
                expected: self.stream.ssrc,
                actual: packet.ssrc(),
            });
        }
        if packet.payload_type() != self.stream.payload_type {
            return Err(IngressMismatch::PayloadType {
                expected: self.stream.payload_type,
                actual: packet.payload_type(),
            });
        }
        let extended =
            extend_sequence(self.next_extended, packet.sequence()).map_err(
                |error| match error {
                    ExtensionError::AmbiguousHalfRange => IngressMismatch::AmbiguousSequence,
                    ExtensionError::BeforeEpoch => IngressMismatch::SequenceBeforeEpoch,
                    ExtensionError::ExtendedOverflow => IngressMismatch::SequenceOverflow,
                },
            )?;
        let offset = extended
            .get()
            .checked_sub(self.stream.initial_sequence.get())
            .ok_or(IngressMismatch::SequenceBeforeEpoch)?;
        let timestamp_delta = offset.wrapping_mul(
            u64::try_from(self.frame_duration.samples_per_channel())
                .expect("supported Opus frame sizes fit u64"),
        ) as u32;
        let expected_timestamp = self.stream.initial_timestamp.wrapping_add(timestamp_delta);
        if packet.timestamp() != expected_timestamp {
            return Err(IngressMismatch::Timestamp {
                expected: expected_timestamp,
                actual: packet.timestamp(),
            });
        }
        let expected_samples = self.frame_duration.samples_per_channel();
        match relay_opus_sys::packet_samples_per_channel(packet.payload(), SAMPLE_RATE_HZ as i32) {
            Ok(actual) if actual == expected_samples => Ok(()),
            Ok(actual) => Err(IngressMismatch::Duration {
                expected_samples_per_channel: expected_samples,
                actual_samples_per_channel: actual,
            }),
            Err(_) => Err(IngressMismatch::MalformedPacket),
        }
    }

    fn pop_decision(&mut self) -> Option<PendingDecision> {
        if self.timeline_exhausted {
            return None;
        }
        let sequence = self.next_extended;
        let timestamp = self.timestamp_for(sequence);
        // Construction and reset always rebase the reorder head. Therefore
        // `Playout::Empty` is an internal invariant violation, never a supported
        // representation of an ordinary missing deadline.
        let (wire_head, decision) = match self.reorder.pop_at_deadline() {
            Playout::Packet {
                sequence: wire_head,
                packet,
            } => (
                wire_head,
                PendingDecision::Packet {
                    sequence,
                    timestamp,
                    packet,
                },
            ),
            Playout::MissingAtDeadline {
                sequence: wire_head,
                ..
            } => (
                wire_head,
                PendingDecision::Missing {
                    sequence,
                    timestamp,
                },
            ),
            Playout::Empty => {
                debug_assert!(false, "rebased RX reorder buffer returned Playout::Empty");
                return None;
            }
        };
        debug_assert_eq!(
            wire_head,
            sequence.wire().get(),
            "reorder wire head diverged from RX extended head"
        );
        if let Some(next) = self.next_extended.get().checked_add(1) {
            self.next_extended = ExtendedSequence::new(next);
        } else {
            // An extended epoch has no representable position after u64::MAX.
            // Emit this final decision, then require a trusted reset to continue.
            self.timeline_exhausted = true;
        }
        sat_inc(&mut self.metrics.deadline_decisions);
        Some(decision)
    }

    fn resolve(
        &mut self,
        pending: PendingDecision,
        current: Option<PendingDecision>,
    ) -> Option<FrameOutcome<'_>> {
        let (sequence, timestamp, source, status) = match (pending, current) {
            (
                PendingDecision::Missing {
                    sequence,
                    timestamp,
                },
                Some(PendingDecision::Packet {
                    sequence: current_sequence,
                    timestamp: current_timestamp,
                    packet,
                }),
            ) => {
                sat_inc(&mut self.metrics.fec_attempts);
                let len = self.frame_duration.interleaved_samples();
                let fec_result = {
                    let decoder = &mut self.decoder;
                    let output = &mut self.output.samples[..len];
                    decoder.decode_fec(packet.payload(), output)
                };
                self.output.len = len;
                let (source, status) = match fec_result {
                    Ok(_) => (FrameSource::InbandFecOrPlc, FrameStatus::Produced),
                    Err(_) => {
                        sat_inc(&mut self.metrics.codec_errors);
                        (
                            FrameSource::PacketLossConcealment,
                            self.plc_into_output(true),
                        )
                    }
                };
                let (ready_source, ready_status) = self.decode_current_into_staged(&packet);
                self.pending = Some(PendingDecision::Ready {
                    sequence: current_sequence,
                    timestamp: current_timestamp,
                    source: ready_source,
                    status: ready_status,
                });
                (sequence, timestamp, source, status)
            }
            (
                PendingDecision::Packet {
                    sequence,
                    timestamp,
                    packet,
                },
                current,
            ) => {
                let (source, status) = self.decode_packet_into_output(&packet);
                self.pending = current;
                (sequence, timestamp, source, status)
            }
            (
                PendingDecision::Missing {
                    sequence,
                    timestamp,
                },
                current,
            ) => {
                let status = self.plc_into_output(false);
                self.pending = current;
                (
                    sequence,
                    timestamp,
                    FrameSource::PacketLossConcealment,
                    status,
                )
            }
            (
                PendingDecision::Ready {
                    sequence,
                    timestamp,
                    source,
                    status,
                },
                current,
            ) => {
                let len = self.frame_duration.interleaved_samples();
                self.output.samples[..len].copy_from_slice(&self.staged.samples[..len]);
                self.output.len = len;
                self.pending = current;
                (sequence, timestamp, source, status)
            }
        };
        match source {
            FrameSource::Packet => sat_inc(&mut self.metrics.packet_frames),
            FrameSource::InbandFecOrPlc => {}
            FrameSource::PacketLossConcealment => sat_inc(&mut self.metrics.plc_frames),
        }
        sat_inc(&mut self.metrics.emitted_frames);
        Some(FrameOutcome {
            sequence,
            timestamp,
            source,
            status,
            frame: &self.output,
        })
    }

    fn decode_packet_into_output(&mut self, packet: &MediaPacket) -> (FrameSource, FrameStatus) {
        let len = self.frame_duration.interleaved_samples();
        let result = {
            let decoder = &mut self.decoder;
            let output = &mut self.output.samples[..len];
            decoder.decode(packet.payload(), output)
        };
        self.output.len = len;
        match result {
            Ok(_) => (FrameSource::Packet, FrameStatus::Produced),
            Err(_) => {
                sat_inc(&mut self.metrics.codec_errors);
                (
                    FrameSource::PacketLossConcealment,
                    self.plc_into_output(true),
                )
            }
        }
    }

    fn decode_current_into_staged(&mut self, packet: &MediaPacket) -> (FrameSource, FrameStatus) {
        let len = self.frame_duration.interleaved_samples();
        match self
            .decoder
            .decode(packet.payload(), &mut self.staged.samples[..len])
        {
            Ok(_) => {
                self.staged.len = len;
                (FrameSource::Packet, FrameStatus::Produced)
            }
            Err(_) => {
                sat_inc(&mut self.metrics.codec_errors);
                let status = match self.decoder.decode_plc(&mut self.staged.samples[..len]) {
                    Ok(_) => FrameStatus::ConcealedCodecError,
                    Err(_) => {
                        sat_inc(&mut self.metrics.codec_errors);
                        self.staged.samples[..len].fill(0.0);
                        FrameStatus::ConcealedCodecError
                    }
                };
                self.staged.len = len;
                (FrameSource::PacketLossConcealment, status)
            }
        }
    }

    fn plc_into_output(&mut self, concealing_codec_error: bool) -> FrameStatus {
        let len = self.frame_duration.interleaved_samples();
        let result = {
            let decoder = &mut self.decoder;
            let output = &mut self.output.samples[..len];
            decoder.decode_plc(output)
        };
        let status = match result {
            Ok(_) if concealing_codec_error => FrameStatus::ConcealedCodecError,
            Ok(_) => FrameStatus::Produced,
            Err(_) => {
                sat_inc(&mut self.metrics.codec_errors);
                self.output.samples[..len].fill(0.0);
                FrameStatus::ConcealedCodecError
            }
        };
        self.output.len = len;
        status
    }

    fn timestamp_for(&self, sequence: ExtendedSequence) -> RtpTimestamp {
        let offset = sequence
            .get()
            .wrapping_sub(self.stream.initial_sequence.get());
        let delta = offset.wrapping_mul(self.frame_duration.samples_per_channel() as u64) as u32;
        self.stream.initial_timestamp.wrapping_add(delta)
    }

    fn record_rejection(&mut self, reason: IngressMismatch) {
        match reason {
            IngressMismatch::Duplicate => sat_inc(&mut self.metrics.duplicates),
            IngressMismatch::Late { .. } => sat_inc(&mut self.metrics.late),
            IngressMismatch::AheadOfWindow { .. } => {
                sat_inc(&mut self.metrics.ahead_of_window);
            }
            IngressMismatch::Ssrc { .. } | IngressMismatch::PayloadType { .. } => {
                sat_inc(&mut self.metrics.identity_mismatches);
            }
            IngressMismatch::Timestamp { .. } | IngressMismatch::Duration { .. } => {
                sat_inc(&mut self.metrics.duration_timestamp_mismatches);
            }
            IngressMismatch::MalformedPacket => sat_inc(&mut self.metrics.malformed_packets),
            IngressMismatch::PacketTooLarge { .. } => {
                sat_inc(&mut self.metrics.oversized_packets);
            }
            IngressMismatch::AmbiguousSequence
            | IngressMismatch::SequenceBeforeEpoch
            | IngressMismatch::SequenceOverflow => {
                sat_inc(&mut self.metrics.extension_rejections);
            }
        }
    }
}

fn sat_inc(value: &mut u64) {
    *value = value.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AdaptiveClockConfig, AudioPipelineConfigInput, ClockRecoveryConfig, MAX_PACKET_BYTES,
    };
    use relay_opus::{
        Bitrate, Encoder, EncoderConfigV1, EncoderPolicyV1, InbandFec, PacketLossPercent,
    };

    const TEST_SSRC: u32 = 7;
    const TEST_PAYLOAD_TYPE: u8 = 111;
    const TEST_SEQUENCE: u64 = 100;
    const TEST_TIMESTAMP: u32 = 1_000;

    fn test_config() -> AudioPipelineConfig {
        AudioPipelineConfig::new(AudioPipelineConfigInput {
            capture_rate_hz: 48_000,
            playback_rate_hz: 48_000,
            channels: 2,
            frame_duration: FrameDuration::Ms10,
            capture_src_chunk_frames: 480,
            capture_ring_samples: 10_000,
            playback_ring_samples: 10_000,
            tx_accumulator_samples: 10_000,
            reorder_capacity: 4,
            network_capacity: 4,
            network_due_batch_capacity: 4,
            packet_capacity: MAX_PACKET_BYTES,
            controller_cadence_frames: 480,
            clock_recovery: ClockRecoveryConfig::default(),
            adaptive_clock: AdaptiveClockConfig::default(),
        })
        .expect("valid scripted RX config")
    }

    fn test_stream() -> RxStreamConfig {
        RxStreamConfig {
            ssrc: Ssrc::new(TEST_SSRC),
            payload_type: PayloadType::new(TEST_PAYLOAD_TYPE).expect("payload type"),
            initial_sequence: ExtendedSequence::new(TEST_SEQUENCE),
            initial_timestamp: RtpTimestamp::new(TEST_TIMESTAMP),
        }
    }

    fn valid_payload() -> Vec<u8> {
        let policy = EncoderPolicyV1::new(
            Bitrate::try_new(64_000).expect("bitrate"),
            InbandFec::Disabled,
            PacketLossPercent::ZERO,
        );
        let mut encoder = Encoder::new(EncoderConfigV1::stereo_48k(FrameDuration::Ms10, policy))
            .expect("encoder");
        let pcm = [0.0_f32; FrameDuration::Ms10.interleaved_samples()];
        let mut output = [0_u8; MAX_PACKET_BYTES];
        let len = encoder
            .encode(&pcm, &mut output)
            .expect("encode test packet");
        output[..len].to_vec()
    }

    fn packet(payload: &[u8], offset: u64) -> MediaPacket {
        MediaPacket::try_new(
            TEST_SSRC,
            TEST_SEQUENCE.wrapping_add(offset) as u16,
            TEST_TIMESTAMP.wrapping_add(
                offset.wrapping_mul(FrameDuration::Ms10.samples_per_channel() as u64) as u32,
            ),
            TEST_PAYLOAD_TYPE,
            payload,
        )
        .expect("test packet")
    }

    fn step(call: DecodeCall, succeeds: bool) -> ScriptStep {
        ScriptStep { call, succeeds }
    }

    #[test]
    fn scripted_fec_then_normal_runs_once_and_ready_does_not_decode_again() {
        let payload = valid_payload();
        let mut worker = RxWorker::with_scripted_decoder(
            test_config(),
            test_stream(),
            vec![step(DecodeCall::Fec, true), step(DecodeCall::Normal, true)],
        );
        assert_eq!(
            worker.ingress(packet(&payload, 1)).status(),
            IngressStatus::AcceptedReordered { depth: 1 }
        );

        assert!(worker.tick().is_none());
        let fec = worker.tick().expect("FEC result");
        assert_eq!(fec.source(), FrameSource::InbandFecOrPlc);
        assert_eq!(fec.status(), FrameStatus::Produced);
        assert_eq!(
            worker.scripted_trace(),
            [Some(DecodeCall::Fec), Some(DecodeCall::Normal)]
        );

        let ready = worker.drain().expect("staged normal result");
        assert_eq!(ready.source(), FrameSource::Packet);
        assert_eq!(ready.status(), FrameStatus::Produced);
        assert_eq!(
            worker.scripted_trace(),
            [Some(DecodeCall::Fec), Some(DecodeCall::Normal)]
        );
        assert_eq!(
            worker.metrics(),
            RxMetrics {
                ingress_packets: 1,
                accepted_reordered: 1,
                deadline_decisions: 2,
                emitted_frames: 2,
                packet_frames: 1,
                fec_attempts: 1,
                ..RxMetrics::default()
            }
        );
    }

    #[test]
    fn scripted_fec_error_falls_back_to_plc_then_normal() {
        let payload = valid_payload();
        let mut worker = RxWorker::with_scripted_decoder(
            test_config(),
            test_stream(),
            vec![
                step(DecodeCall::Fec, false),
                step(DecodeCall::Plc, true),
                step(DecodeCall::Normal, true),
            ],
        );
        assert_eq!(
            worker.ingress(packet(&payload, 1)).status(),
            IngressStatus::AcceptedReordered { depth: 1 }
        );
        assert!(worker.tick().is_none());
        let concealed = worker.tick().expect("PLC fallback");
        assert_eq!(concealed.source(), FrameSource::PacketLossConcealment);
        assert_eq!(concealed.status(), FrameStatus::ConcealedCodecError);
        let following = worker.drain().expect("following normal decode");
        assert_eq!(following.source(), FrameSource::Packet);
        assert_eq!(following.status(), FrameStatus::Produced);
        assert_eq!(
            worker.scripted_trace(),
            [
                Some(DecodeCall::Fec),
                Some(DecodeCall::Plc),
                Some(DecodeCall::Normal)
            ]
        );
        assert_eq!(worker.metrics().codec_errors, 1);
        assert_eq!(worker.metrics().plc_frames, 1);
    }

    #[test]
    fn scripted_normal_error_uses_plc_and_next_packet_still_succeeds() {
        let payload = valid_payload();
        let mut worker = RxWorker::with_scripted_decoder(
            test_config(),
            test_stream(),
            vec![
                step(DecodeCall::Normal, false),
                step(DecodeCall::Plc, true),
                step(DecodeCall::Normal, true),
            ],
        );
        assert_eq!(
            worker.ingress(packet(&payload, 0)).status(),
            IngressStatus::AcceptedInOrder
        );
        assert_eq!(
            worker.ingress(packet(&payload, 1)).status(),
            IngressStatus::AcceptedReordered { depth: 1 }
        );
        assert!(worker.tick().is_none());
        let concealed = worker.tick().expect("normal error fallback");
        assert_eq!(concealed.source(), FrameSource::PacketLossConcealment);
        assert_eq!(concealed.status(), FrameStatus::ConcealedCodecError);
        let recovered = worker.drain().expect("subsequent normal success");
        assert_eq!(recovered.source(), FrameSource::Packet);
        assert_eq!(recovered.status(), FrameStatus::Produced);
        assert_eq!(
            worker.scripted_trace(),
            [
                Some(DecodeCall::Normal),
                Some(DecodeCall::Plc),
                Some(DecodeCall::Normal)
            ]
        );
    }

    #[test]
    fn scripted_plc_error_emits_zeroes_then_decoder_recovers() {
        let payload = valid_payload();
        let mut worker = RxWorker::with_scripted_decoder(
            test_config(),
            test_stream(),
            vec![step(DecodeCall::Plc, false), step(DecodeCall::Normal, true)],
        );
        assert!(worker.tick().is_none());
        {
            let zero = worker.drain().expect("bounded silence fallback");
            assert_eq!(zero.source(), FrameSource::PacketLossConcealment);
            assert_eq!(zero.status(), FrameStatus::ConcealedCodecError);
            assert!(zero.frame().samples().iter().all(|sample| *sample == 0.0));
        }
        assert_eq!(
            worker.ingress(packet(&payload, 1)).status(),
            IngressStatus::AcceptedInOrder
        );
        assert!(worker.tick().is_none());
        let recovered = worker.drain().expect("normal decode after PLC failure");
        assert_eq!(recovered.source(), FrameSource::Packet);
        assert_eq!(recovered.status(), FrameStatus::Produced);
        assert_eq!(
            worker.scripted_trace(),
            [Some(DecodeCall::Plc), Some(DecodeCall::Normal)]
        );
        assert_eq!(worker.metrics().codec_errors, 1);
        assert_eq!(worker.metrics().plc_frames, 1);
        assert_eq!(worker.metrics().emitted_frames, 2);
    }
}
