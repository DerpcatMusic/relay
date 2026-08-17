use core::fmt;

use relay_opus::{
    Bitrate, CHANNELS, Encoder, EncoderConfigV1, EncoderPolicyV1, Error as OpusError,
};
use relay_resample::{
    FiniteFixedRatioConverter, FiniteProcessReport, FixedRatioConverter, ResampleError,
    WorkerResampler,
};

use crate::{
    AccumulatorError, AudioPipelineConfig, Interleaved48kAccumulator, MediaPacket, PacketError,
    PayloadType, RtpTimestamp, SequenceNumber, Ssrc,
};

/// Fixed stream identity and initial wire timeline for a TX epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxStreamConfig {
    /// Synchronization source for every emitted packet.
    pub ssrc: Ssrc,
    /// Fixed negotiated RTP payload type.
    pub payload_type: PayloadType,
    /// First emitted wire sequence number.
    pub initial_sequence: SequenceNumber,
    /// First emitted 48 kHz RTP timestamp.
    pub initial_timestamp: RtpTimestamp,
    /// Versioned negotiated bitrate/FEC/loss policy; V1 forces DTX off.
    pub encoding_policy: EncoderPolicyV1,
}

/// Exact wrapping transmit timeline. Values advance only after packet creation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxTimeline {
    sequence: SequenceNumber,
    timestamp: RtpTimestamp,
    timestamp_step: u32,
}

impl TxTimeline {
    /// Starts a timeline with an exact 48 kHz timestamp increment.
    #[must_use]
    pub const fn new(
        sequence: SequenceNumber,
        timestamp: RtpTimestamp,
        timestamp_step: u32,
    ) -> Self {
        Self {
            sequence,
            timestamp,
            timestamp_step,
        }
    }

    /// Returns values for the next packet without advancing them.
    #[must_use]
    pub const fn current(self) -> (SequenceNumber, RtpTimestamp) {
        (self.sequence, self.timestamp)
    }

    /// Advances sequence by one and timestamp by the negotiated frame size.
    pub fn advance(&mut self) {
        self.sequence = self.sequence.wrapping_next();
        self.timestamp = self.timestamp.wrapping_add(self.timestamp_step);
    }
}

/// Fixed-capacity reusable packet output for one caller-driven TX operation.
#[derive(Debug)]
pub struct PacketBatch {
    slots: Box<[Option<MediaPacket>]>,
    cursor: usize,
    len: usize,
}

impl PacketBatch {
    /// Allocates exactly `capacity` packet slots.
    pub fn new(capacity: usize) -> Result<Self, PacketBatchError> {
        if capacity == 0 {
            return Err(PacketBatchError::ZeroCapacity);
        }
        capacity
            .checked_mul(core::mem::size_of::<Option<MediaPacket>>())
            .ok_or(PacketBatchError::CapacityOverflow)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| PacketBatchError::AllocationFailed)?;
        slots.resize_with(capacity, || None);
        Ok(Self {
            slots: slots.into_boxed_slice(),
            cursor: 0,
            len: 0,
        })
    }

    /// Immutable packet capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    /// Packets not yet taken.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len.saturating_sub(self.cursor)
    }
    /// Whether no packet remains.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Moves out the next packet in emission order.
    pub fn take_next(&mut self) -> Option<MediaPacket> {
        if self.cursor == self.len {
            self.clear();
            return None;
        }
        let packet = self.slots[self.cursor].take();
        self.cursor += 1;
        if self.cursor == self.len {
            self.cursor = 0;
            self.len = 0;
        }
        packet
    }
    /// Drops unconsumed packets and reuses the same slots.
    pub fn clear(&mut self) {
        for slot in &mut self.slots[..self.len] {
            *slot = None;
        }
        self.cursor = 0;
        self.len = 0;
    }
    fn is_full(&self) -> bool {
        self.len == self.slots.len()
    }
    fn push(&mut self, packet: MediaPacket) {
        self.slots[self.len] = Some(packet);
        self.len += 1;
    }
    #[cfg(test)]
    fn storage_identity(&self) -> (*const Option<MediaPacket>, usize) {
        (self.slots.as_ptr(), self.slots.len())
    }
}

/// Packet-batch construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketBatchError {
    /// Capacity was zero.
    ZeroCapacity,
    /// Capacity arithmetic overflowed.
    CapacityOverflow,
    /// Fixed construction allocation failed.
    AllocationFailed,
}
impl fmt::Display for PacketBatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PacketBatchError {}

/// One explicit live capture event. No endpoint, wait, or I/O is hidden here.
pub enum CaptureInput<'a> {
    /// Exactly one converter-required interleaved capture chunk.
    Chunk(&'a [f32]),
    /// The live source disconnected; streaming SRC tail is not finite-drained.
    Disconnected,
}

/// Per-call deterministic live processing counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TxProcessReport {
    /// Capture frames accepted in this call.
    pub capture_frames_consumed: usize,
    /// 48 kHz frames produced by SRC in this call.
    pub media_frames_produced: usize,
    /// Packets appended to the supplied batch.
    pub packets_emitted: usize,
    /// Complete capture chunk is still blocked behind prior output.
    pub input_pending: bool,
}

/// Live disconnect policy/accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveDisconnectReport {
    /// Complete packets emitted by this disconnect call.
    pub packets_emitted: usize,
    /// Unpacketized 48 kHz stereo frames explicitly discarded (never encoded short).
    pub discarded_partial_media_frames: usize,
    /// Configured streaming converter algorithmic delay.
    pub configured_converter_delay_frames: usize,
    /// Retained converter tail abandoned by this stream.
    ///
    /// This is zero until a capture chunk has entered the converter; after that
    /// it is conservatively accounted as the configured algorithmic delay.
    pub abandoned_converter_tail_frames: usize,
}

/// Live processing failure with all committed per-call progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TxProcessFailure {
    /// Underlying validation, converter, codec, packet, or lifecycle failure.
    pub cause: TxError,
    /// Input, converter output, and packet progress committed before failure.
    pub progress: TxProcessReport,
}

/// Explicit nonblocking outcome from one caller-driven live step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TxProcessOutcome {
    /// Input was consumed and all currently available output was handled.
    Complete(TxProcessReport),
    /// Reusable packet output filled; call again with an empty batch.
    BatchFull(TxProcessReport),
    /// Disconnect drain completed under the documented live-tail policy.
    Disconnected(LiveDisconnectReport),
    /// Processing failed; the payload reports all committed per-call progress.
    Error(TxProcessFailure),
}

/// TX construction failure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TxBuildError {
    /// Fixed accumulator construction failed.
    Accumulator(AccumulatorError),
    /// Fixed converter construction failed.
    Resampler(ResampleError),
    /// Canonical stereo Opus encoder construction failed.
    Opus(OpusError),
    /// Buffer sample arithmetic overflowed.
    CapacityOverflow,
    /// A fixed worker buffer could not be allocated.
    AllocationFailed,
}
impl fmt::Display for TxBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TxBuildError {}

/// TX processing failure. Prevalidated input failures leave the live stream
/// usable; backend/output, codec, and packet failures fault it until reset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TxError {
    /// Supplied output batch still owns packets.
    BatchNotEmpty,
    /// Capture input was not the exact converter-required length.
    InvalidCaptureLength {
        /// Required scalar samples.
        expected: usize,
        /// Supplied scalar samples.
        actual: usize,
    },
    /// A capture event was submitted after disconnect, or a chunk was
    /// submitted while disconnect draining was already in progress.
    AlreadyDisconnected,
    /// A one-shot finite worker already completed successfully.
    FiniteAlreadyProcessed,
    /// Delay-compensated finite output does not fill the final Opus frame.
    IncompleteFinalOpusFrame {
        /// Valid 48 kHz media frames in the incomplete final frame.
        valid_media_frames: usize,
        /// Required 48 kHz media frames in one negotiated Opus packet.
        packet_frames: usize,
    },
    /// Worker is faulted and requires reset (when reset is supported).
    Faulted,
    /// Resampler rejected input or processing.
    Resampler(ResampleError),
    /// Opus rejected canonical-frame encoding.
    Opus(OpusError),
    /// Inline packet construction failed.
    Packet(PacketError),
}
impl fmt::Display for TxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TxError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveState {
    Active,
    Disconnecting,
    Disconnected,
    Faulted,
}

struct Packetizer {
    config: AudioPipelineConfig,
    encoder: Encoder,
    encoder_config: EncoderConfigV1,
    accumulator: Interleaved48kAccumulator,
    pcm_frame: Box<[f32]>,
    encoded: Box<[u8]>,
    timeline: TxTimeline,
    ssrc: Ssrc,
    payload_type: PayloadType,
    #[cfg(test)]
    fail_encode_after: Option<usize>,
    #[cfg(test)]
    fail_next_reset: bool,
}

impl Packetizer {
    fn new(config: AudioPipelineConfig, stream: TxStreamConfig) -> Result<Self, TxBuildError> {
        let encoder_config =
            EncoderConfigV1::stereo_48k(config.frame_duration(), stream.encoding_policy);
        let encoder = Encoder::new(encoder_config).map_err(TxBuildError::Opus)?;
        let accumulator = Interleaved48kAccumulator::new(config.tx_accumulator_samples())
            .map_err(TxBuildError::Accumulator)?;
        let pcm_frame = boxed_zeroed(config.opus_packet_samples())?;
        let encoded = boxed_zeroed_bytes(config.packet_capacity())?;
        Ok(Self {
            config,
            encoder,
            encoder_config,
            accumulator,
            pcm_frame,
            encoded,
            timeline: TxTimeline::new(
                stream.initial_sequence,
                stream.initial_timestamp,
                config.frame_duration().samples_per_channel() as u32,
            ),
            ssrc: stream.ssrc,
            payload_type: stream.payload_type,
            #[cfg(test)]
            fail_encode_after: None,
            #[cfg(test)]
            fail_next_reset: false,
        })
    }

    fn emit_complete(&mut self, batch: &mut PacketBatch) -> Result<usize, (TxError, usize)> {
        let mut emitted = 0;
        while self.accumulator.len_samples() >= self.pcm_frame.len() && !batch.is_full() {
            self.accumulator.peek_exact(&mut self.pcm_frame);
            if let Err(error) = self.encode_pcm(batch) {
                return Err((error, emitted));
            }
            self.accumulator.discard_exact(self.pcm_frame.len());
            emitted += 1;
        }
        Ok(emitted)
    }

    fn encode_pcm(&mut self, batch: &mut PacketBatch) -> Result<(), TxError> {
        #[cfg(test)]
        if let Some(remaining) = &mut self.fail_encode_after {
            if *remaining == 0 {
                self.fail_encode_after = None;
                return Err(TxError::Opus(OpusError::InvalidCodecResult));
            }
            *remaining -= 1;
        }
        let len = self
            .encoder
            .encode(&self.pcm_frame, &mut self.encoded)
            .map_err(TxError::Opus)?;
        let (sequence, timestamp) = self.timeline.current();
        let packet = self
            .config
            .create_media_packet(
                self.ssrc,
                sequence,
                timestamp,
                self.payload_type,
                &self.encoded[..len],
            )
            .map_err(TxError::Packet)?;
        batch.push(packet);
        self.timeline.advance();
        Ok(())
    }

    fn reset(&mut self, sequence: SequenceNumber, timestamp: RtpTimestamp) -> Result<(), TxError> {
        #[cfg(test)]
        if self.fail_next_reset {
            self.fail_next_reset = false;
            return Err(TxError::Opus(OpusError::InvalidCodecResult));
        }
        self.encoder.reset().map_err(TxError::Opus)?;
        self.accumulator.clear();
        self.timeline = TxTimeline::new(
            sequence,
            timestamp,
            self.encoder_config.frame_duration().samples_per_channel() as u32,
        );
        Ok(())
    }
}

/// Caller-driven deterministic live capture-to-Opus worker.
///
/// Construction owns all converter, PCM, packet, and accumulator storage.
/// Processing accepts only complete required chunks and never waits, performs
/// I/O, spawns, or grows its buffers.
pub struct TxWorker {
    converter: FixedRatioConverter,
    capture_input: Box<[f32]>,
    converter_output: Box<[f32]>,
    pending_output_samples: usize,
    pending_output_cursor: usize,
    packetizer: Packetizer,
    state: LiveState,
    converter_delay_frames: usize,
    converter_has_input: bool,
    #[cfg(test)]
    next_resampler_error: Option<ResampleError>,
}

impl TxWorker {
    /// Constructs/prewarms the fixed capture -> 48 kHz path and canonical
    /// stereo Opus encoder. Unity 48 kHz uses the converter's exact bypass.
    pub fn new(config: AudioPipelineConfig, stream: TxStreamConfig) -> Result<Self, TxBuildError> {
        let converter = FixedRatioConverter::new(
            config.capture_rate_hz(),
            48_000,
            usize::from(CHANNELS),
            config.capture_src_chunk_frames(),
        )
        .map_err(TxBuildError::Resampler)?;
        let req = converter.requirements();
        let capture_input = boxed_zeroed(checked_samples(req.input_frames_max)?)?;
        let converter_output = boxed_zeroed(checked_samples(req.output_frames_max)?)?;
        let converter_delay_frames = req.output_delay;
        Ok(Self {
            converter,
            capture_input,
            converter_output,
            pending_output_samples: 0,
            pending_output_cursor: 0,
            packetizer: Packetizer::new(config, stream)?,
            state: LiveState::Active,
            converter_delay_frames,
            converter_has_input: false,
            #[cfg(test)]
            next_resampler_error: None,
        })
    }

    /// Exact scalar sample count required in every [`CaptureInput::Chunk`].
    #[must_use]
    pub fn capture_chunk_samples(&self) -> usize {
        self.capture_input.len()
    }

    /// Interleaved 48 kHz samples in one negotiated media frame.
    #[must_use]
    pub fn media_pcm_frame_samples(&self) -> usize {
        self.packetizer.pcm_frame.len()
    }

    /// Updates the live Opus bitrate. Off-callback only.
    ///
    /// # Errors
    ///
    /// Returns [`TxError::Opus`] if the encoder rejects the rate.
    pub fn set_bitrate(&mut self, bps: i32) -> Result<(), TxError> {
        let bitrate = Bitrate::try_new(bps).map_err(TxError::Opus)?;
        self.packetizer
            .encoder
            .set_bitrate(bitrate)
            .map_err(TxError::Opus)
    }

    /// Resamples one capture chunk into the 48 kHz accumulator without encoding.
    ///
    /// # Errors
    ///
    /// Returns the same capture-length and resampler failures as
    /// [`Self::process_capture`].
    pub fn ingest_capture(&mut self, samples: &[f32]) -> Result<TxProcessReport, TxError> {
        if !matches!(self.state, LiveState::Active) {
            return Err(TxError::Faulted);
        }
        if samples.len() != self.capture_input.len() {
            return Err(TxError::InvalidCaptureLength {
                expected: self.capture_input.len(),
                actual: samples.len(),
            });
        }
        if let Some(sample_index) = samples.iter().position(|sample| !sample.is_finite()) {
            return Err(TxError::Resampler(ResampleError::NonFiniteInput {
                sample_index,
            }));
        }
        self.capture_input.copy_from_slice(samples);
        let processed = self
            .converter
            .process_interleaved(&self.capture_input, &mut self.converter_output)
            .map_err(TxError::Resampler)?;
        self.converter_has_input = true;
        self.pending_output_cursor = 0;
        self.pending_output_samples = processed.output_frames * usize::from(CHANNELS);
        while self.pending_output_cursor < self.pending_output_samples {
            let pushed = self.packetizer.accumulator.push_prefix(
                &self.converter_output[self.pending_output_cursor..self.pending_output_samples],
            );
            if pushed == 0 {
                break;
            }
            self.pending_output_cursor += pushed;
        }
        Ok(TxProcessReport {
            capture_frames_consumed: processed.input_frames,
            media_frames_produced: processed.output_frames,
            ..TxProcessReport::default()
        })
    }

    /// Copies one complete 48 kHz frame without consuming the encoder queue.
    #[must_use]
    pub fn copy_ready_pcm(&self, dest: &mut [f32]) -> bool {
        self.copy_ready_pcm_all(dest) >= self.packetizer.pcm_frame.len()
    }

    /// Copies every complete 48 kHz frame waiting to encode, without consuming them.
    #[must_use]
    pub fn copy_ready_pcm_all(&self, dest: &mut [f32]) -> usize {
        let frame = self.packetizer.pcm_frame.len();
        if frame == 0 {
            return 0;
        }
        let available = self.packetizer.accumulator.len_samples();
        let take = (available / frame) * frame;
        let take = take.min(dest.len());
        if take == 0 {
            return 0;
        }
        self.packetizer.accumulator.peek_exact(&mut dest[..take]);
        take
    }

    /// Encodes already-ingested 48 kHz frames. Does not resample again.
    pub fn encode_ready(&mut self, batch: &mut PacketBatch) -> TxProcessOutcome {
        if !batch.is_empty() {
            return process_error(TxError::BatchNotEmpty, TxProcessReport::default());
        }
        if !matches!(self.state, LiveState::Active) {
            return process_error(TxError::Faulted, TxProcessReport::default());
        }
        let mut report = TxProcessReport::default();
        if let Err(error) = self.drain_pending(batch, &mut report) {
            return self.fail(error, report);
        }
        TxProcessOutcome::Complete(report)
    }

    /// Pops one complete 48 kHz media frame and advances the TX timeline.
    pub fn take_media_pcm(&mut self, dest: &mut [f32]) -> Option<(SequenceNumber, RtpTimestamp)> {
        let needed = self.packetizer.pcm_frame.len();
        if dest.len() < needed || self.packetizer.accumulator.len_samples() < needed {
            return None;
        }
        self.packetizer.accumulator.peek_exact(&mut dest[..needed]);
        self.packetizer.accumulator.discard_exact(needed);
        let stamp = self.packetizer.timeline.current();
        self.packetizer.timeline.advance();
        Some(stamp)
    }

    /// Retained 48 kHz frames, including complete frames blocked by batch capacity.
    #[must_use]
    pub fn accumulated_media_frames(&self) -> usize {
        self.packetizer.accumulator.len_frames()
    }

    /// Processes one explicit capture event into a reusable bounded packet batch.
    pub fn process_capture(
        &mut self,
        input: CaptureInput<'_>,
        batch: &mut PacketBatch,
    ) -> TxProcessOutcome {
        if !batch.is_empty() {
            return process_error(TxError::BatchNotEmpty, TxProcessReport::default());
        }
        match self.state {
            LiveState::Faulted => {
                return process_error(TxError::Faulted, TxProcessReport::default());
            }
            LiveState::Disconnected => {
                return process_error(TxError::AlreadyDisconnected, TxProcessReport::default());
            }
            LiveState::Disconnecting if matches!(input, CaptureInput::Chunk(_)) => {
                return process_error(TxError::AlreadyDisconnected, TxProcessReport::default());
            }
            LiveState::Active | LiveState::Disconnecting => {}
        }

        // Validate new caller-owned input before draining prior accepted output.
        if let CaptureInput::Chunk(samples) = input {
            if samples.len() != self.capture_input.len() {
                return process_error(
                    TxError::InvalidCaptureLength {
                        expected: self.capture_input.len(),
                        actual: samples.len(),
                    },
                    TxProcessReport::default(),
                );
            }
            if let Some(sample_index) = samples.iter().position(|sample| !sample.is_finite()) {
                return process_error(
                    TxError::Resampler(ResampleError::NonFiniteInput { sample_index }),
                    TxProcessReport::default(),
                );
            }
        }

        let disconnect = matches!(input, CaptureInput::Disconnected);
        if disconnect {
            self.state = LiveState::Disconnecting;
        }
        let mut report = TxProcessReport::default();
        if let Err(error) = self.drain_pending(batch, &mut report) {
            return self.fail(error, report);
        }
        if batch.is_full()
            && (self.pending_output_cursor < self.pending_output_samples
                || self.packetizer.accumulator.len_samples() >= self.packetizer.pcm_frame.len())
        {
            report.input_pending = !disconnect;
            return TxProcessOutcome::BatchFull(report);
        }
        if self.state == LiveState::Disconnecting {
            let discarded = self.packetizer.accumulator.len_frames();
            let abandoned_tail = if self.converter_has_input {
                self.converter_delay_frames
            } else {
                0
            };
            self.packetizer.accumulator.clear();
            self.converter.reset();
            self.converter_has_input = false;
            self.state = LiveState::Disconnected;
            return TxProcessOutcome::Disconnected(LiveDisconnectReport {
                packets_emitted: report.packets_emitted,
                discarded_partial_media_frames: discarded,
                configured_converter_delay_frames: self.converter_delay_frames,
                abandoned_converter_tail_frames: abandoned_tail,
            });
        }
        let CaptureInput::Chunk(samples) = input else {
            unreachable!()
        };
        self.capture_input.copy_from_slice(samples);
        #[cfg(test)]
        let injected_error = self.next_resampler_error.take();
        #[cfg(not(test))]
        let injected_error: Option<ResampleError> = None;
        let processed = match injected_error.map_or_else(
            || {
                self.converter
                    .process_interleaved(&self.capture_input, &mut self.converter_output)
            },
            Err,
        ) {
            Ok(processed) => processed,
            Err(error @ (ResampleError::Backend | ResampleError::NonFiniteOutput { .. })) => {
                return self.fail(TxError::Resampler(error), report);
            }
            Err(error) => return process_error(TxError::Resampler(error), report),
        };
        self.converter_has_input = true;
        report.capture_frames_consumed = processed.input_frames;
        report.media_frames_produced = processed.output_frames;
        self.pending_output_cursor = 0;
        self.pending_output_samples = processed.output_frames * usize::from(CHANNELS);
        if let Err(error) = self.drain_pending(batch, &mut report) {
            return self.fail(error, report);
        }
        if self.pending_output_cursor < self.pending_output_samples
            || self.packetizer.accumulator.len_samples() >= self.packetizer.pcm_frame.len()
        {
            TxProcessOutcome::BatchFull(report)
        } else {
            TxProcessOutcome::Complete(report)
        }
    }

    /// Reconstructs codec state and clears streaming history while retaining all
    /// caller-visible worker buffers and capacities. This is a control operation.
    ///
    /// The fallible codec reset is staged before converter/pending-state clearing.
    /// Any reset failure leaves the worker explicitly faulted; callers may retry
    /// reset, but may not continue the previous epoch.
    pub fn reset(
        &mut self,
        sequence: SequenceNumber,
        timestamp: RtpTimestamp,
    ) -> Result<(), TxError> {
        self.state = LiveState::Faulted;
        self.packetizer.reset(sequence, timestamp)?;
        self.converter.reset();
        self.pending_output_samples = 0;
        self.pending_output_cursor = 0;
        self.converter_has_input = false;
        self.state = LiveState::Active;
        Ok(())
    }

    fn drain_pending(
        &mut self,
        batch: &mut PacketBatch,
        report: &mut TxProcessReport,
    ) -> Result<(), TxError> {
        loop {
            match self.packetizer.emit_complete(batch) {
                Ok(emitted) => report.packets_emitted += emitted,
                Err((error, emitted)) => {
                    report.packets_emitted += emitted;
                    return Err(error);
                }
            }
            if batch.is_full() {
                return Ok(());
            }
            if self.pending_output_cursor == self.pending_output_samples {
                return Ok(());
            }
            let pushed = self.packetizer.accumulator.push_prefix(
                &self.converter_output[self.pending_output_cursor..self.pending_output_samples],
            );
            debug_assert!(pushed != 0);
            self.pending_output_cursor += pushed;
        }
    }

    fn fail(&mut self, error: TxError, progress: TxProcessReport) -> TxProcessOutcome {
        self.state = LiveState::Faulted;
        process_error(error, progress)
    }

    #[cfg(test)]
    fn storage_identity(&self) -> [(*const u8, usize); 5] {
        let accumulator = self.packetizer.accumulator.storage_identity();
        [
            (self.capture_input.as_ptr().cast(), self.capture_input.len()),
            (
                self.converter_output.as_ptr().cast(),
                self.converter_output.len(),
            ),
            (
                self.packetizer.pcm_frame.as_ptr().cast(),
                self.packetizer.pcm_frame.len(),
            ),
            (
                self.packetizer.encoded.as_ptr(),
                self.packetizer.encoded.len(),
            ),
            (accumulator.0.cast(), accumulator.1),
        ]
    }
}

/// Incomplete finite-packet handling policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalFramePolicy {
    /// Zero-pad exactly one final Opus frame and report valid/padded frames.
    ZeroPad,
    /// Require delay-compensated output to fill complete Opus frames.
    RequireComplete,
}

/// Complete finite-source conversion/packetization accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FiniteTxReport {
    /// Converter trimming and exact useful-output accounting.
    pub resampler: FiniteProcessReport,
    /// Packets emitted into the batch.
    pub packets_emitted: usize,
    /// Valid 48 kHz frames in a zero-padded final packet.
    pub final_valid_media_frames: usize,
    /// Zero frames appended only under [`FinalFramePolicy::ZeroPad`].
    pub zero_padded_media_frames: usize,
    /// Whether output capacity prevented conversion from starting.
    pub batch_full: bool,
    /// Total empty-batch capacity required for the requested finite operation.
    pub required_batch_capacity: usize,
}

/// A finite operation failure with unambiguous committed progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiniteTxError {
    /// Underlying validation, converter, codec, packet, or lifecycle failure.
    pub cause: TxError,
    /// Source frames definitely consumed before the failure.
    pub input_frames_consumed: usize,
    /// Packets successfully committed to the batch before the failure.
    pub packets_emitted: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FiniteState {
    Ready,
    Completed,
    Faulted,
}

/// Preallocated one-shot finite capture converter/packetizer.
///
/// Unlike [`TxWorker`], this uses [`FiniteFixedRatioConverter`] and therefore
/// drains filter tail and exposes exact leading/trailing trimming.
pub struct FiniteTxWorker {
    converter: FiniteFixedRatioConverter,
    workspace: Box<[f32]>,
    max_input_frames: usize,
    packetizer: Packetizer,
    state: FiniteState,
}

impl FiniteTxWorker {
    /// Preallocates for any finite input through `max_input_frames`.
    pub fn new(
        config: AudioPipelineConfig,
        stream: TxStreamConfig,
        max_input_frames: usize,
    ) -> Result<Self, TxBuildError> {
        let converter = FiniteFixedRatioConverter::new(
            config.capture_rate_hz(),
            48_000,
            usize::from(CHANNELS),
            config.capture_src_chunk_frames(),
        )
        .map_err(TxBuildError::Resampler)?;
        let req = converter
            .requirements(max_input_frames)
            .map_err(TxBuildError::Resampler)?;
        let workspace = boxed_zeroed(checked_samples(req.output_workspace_frames)?)?;
        Ok(Self {
            converter,
            workspace,
            max_input_frames,
            packetizer: Packetizer::new(config, stream)?,
            state: FiniteState::Ready,
        })
    }

    /// Converts and packetizes one complete finite source. The batch must be
    /// empty and large enough for all packets; otherwise no conversion occurs.
    pub fn process_finite(
        &mut self,
        input: &[f32],
        policy: FinalFramePolicy,
        batch: &mut PacketBatch,
    ) -> Result<FiniteTxReport, FiniteTxError> {
        match self.state {
            FiniteState::Completed => {
                return Err(finite_error(TxError::FiniteAlreadyProcessed, 0, 0));
            }
            FiniteState::Faulted => return Err(finite_error(TxError::Faulted, 0, 0)),
            FiniteState::Ready => {}
        }
        if !batch.is_empty() {
            return Err(finite_error(TxError::BatchNotEmpty, 0, 0));
        }
        if !input.len().is_multiple_of(usize::from(CHANNELS)) {
            return Err(finite_error(
                TxError::Resampler(ResampleError::InvalidInterleavedLength {
                    channels: usize::from(CHANNELS),
                    actual: input.len(),
                }),
                0,
                0,
            ));
        }
        if let Some(sample_index) = input.iter().position(|sample| !sample.is_finite()) {
            return Err(finite_error(
                TxError::Resampler(ResampleError::NonFiniteInput { sample_index }),
                0,
                0,
            ));
        }
        let input_frames = input.len() / usize::from(CHANNELS);
        if input_frames > self.max_input_frames {
            return Err(finite_error(
                TxError::Resampler(ResampleError::OutputBufferTooSmall {
                    required: input_frames,
                    actual: self.max_input_frames,
                }),
                0,
                0,
            ));
        }
        let req = self
            .converter
            .requirements(input_frames)
            .map_err(|error| finite_error(TxError::Resampler(error), 0, 0))?;
        let packet_frames = self
            .packetizer
            .encoder_config
            .frame_duration()
            .samples_per_channel();
        let full_packets = req.output_frames / packet_frames;
        let final_valid_media_frames = req.output_frames % packet_frames;
        let has_partial = final_valid_media_frames != 0;
        if has_partial && policy == FinalFramePolicy::RequireComplete {
            return Err(finite_error(
                TxError::IncompleteFinalOpusFrame {
                    valid_media_frames: final_valid_media_frames,
                    packet_frames,
                },
                0,
                0,
            ));
        }
        let needed = full_packets + usize::from(has_partial);
        if needed > batch.capacity() {
            return Ok(FiniteTxReport {
                resampler: empty_finite_report(),
                packets_emitted: 0,
                final_valid_media_frames: 0,
                zero_padded_media_frames: 0,
                batch_full: true,
                required_batch_capacity: needed,
            });
        }
        let report = match self
            .converter
            .process_interleaved(input, &mut self.workspace)
        {
            Ok(report) => report,
            Err(error @ (ResampleError::Backend | ResampleError::NonFiniteOutput { .. })) => {
                self.state = FiniteState::Faulted;
                return Err(finite_error(TxError::Resampler(error), 0, 0));
            }
            Err(error) => return Err(finite_error(TxError::Resampler(error), 0, 0)),
        };
        let range = report.valid_output_frame_range();
        let mut cursor = range.start * usize::from(CHANNELS);
        let end = range.end * usize::from(CHANNELS);
        let frame_samples = self.packetizer.pcm_frame.len();
        let mut emitted = 0;
        while end - cursor >= frame_samples {
            self.packetizer
                .pcm_frame
                .copy_from_slice(&self.workspace[cursor..cursor + frame_samples]);
            if let Err(error) = self.packetizer.encode_pcm(batch) {
                self.state = FiniteState::Faulted;
                return Err(finite_error(error, report.input_frames, emitted));
            }
            emitted += 1;
            cursor += frame_samples;
        }
        let remaining_samples = end - cursor;
        let valid_frames = remaining_samples / usize::from(CHANNELS);
        let mut padded = 0;
        if remaining_samples != 0 && policy == FinalFramePolicy::ZeroPad {
            self.packetizer.pcm_frame.fill(0.0);
            self.packetizer.pcm_frame[..remaining_samples]
                .copy_from_slice(&self.workspace[cursor..end]);
            padded = packet_frames - valid_frames;
            if let Err(error) = self.packetizer.encode_pcm(batch) {
                self.state = FiniteState::Faulted;
                return Err(finite_error(error, report.input_frames, emitted));
            }
            emitted += 1;
        }
        self.state = FiniteState::Completed;
        Ok(FiniteTxReport {
            resampler: report,
            packets_emitted: emitted,
            final_valid_media_frames: valid_frames,
            zero_padded_media_frames: padded,
            batch_full: false,
            required_batch_capacity: needed,
        })
    }
}

fn checked_samples(frames: usize) -> Result<usize, TxBuildError> {
    frames
        .checked_mul(usize::from(CHANNELS))
        .ok_or(TxBuildError::CapacityOverflow)
}
fn boxed_zeroed(len: usize) -> Result<Box<[f32]>, TxBuildError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| TxBuildError::AllocationFailed)?;
    values.resize(len, 0.0);
    Ok(values.into_boxed_slice())
}
fn boxed_zeroed_bytes(len: usize) -> Result<Box<[u8]>, TxBuildError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| TxBuildError::AllocationFailed)?;
    values.resize(len, 0);
    Ok(values.into_boxed_slice())
}
fn process_error(cause: TxError, progress: TxProcessReport) -> TxProcessOutcome {
    TxProcessOutcome::Error(TxProcessFailure { cause, progress })
}
fn finite_error(
    cause: TxError,
    input_frames_consumed: usize,
    packets_emitted: usize,
) -> FiniteTxError {
    FiniteTxError {
        cause,
        input_frames_consumed,
        packets_emitted,
    }
}
fn empty_finite_report() -> FiniteProcessReport {
    FiniteProcessReport {
        input_frames: 0,
        generated_output_frames: 0,
        output_frames: 0,
        leading_trim_frames: 0,
        trailing_trim_frames: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureInput, FinalFramePolicy, FiniteState, FiniteTxWorker, LiveState, PacketBatch,
        TxError, TxProcessFailure, TxProcessOutcome, TxStreamConfig, TxTimeline, TxWorker,
    };
    use crate::{
        AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate,
        ClockRecoveryConfig, EncoderPolicyV1, FrameDuration, InbandFec, MAX_PACKET_BYTES,
        MediaPacket, PacketLossPercent, PayloadType, RtpTimestamp, SequenceNumber, Ssrc,
    };
    use relay_resample::ResampleError;

    #[test]
    fn packet_batch_reuses_fixed_slots() {
        let mut batch = PacketBatch::new(2).expect("batch");
        let identity = batch.storage_identity();
        for sequence in 0..32 {
            let packet = MediaPacket::new(
                Ssrc::new(1),
                SequenceNumber::new(sequence),
                RtpTimestamp::new(u32::from(sequence)),
                PayloadType::new(111).expect("payload type"),
                &[1],
            )
            .expect("packet");
            batch.push(packet);
            assert_eq!(
                batch.take_next().expect("packet").sequence().get(),
                sequence
            );
            assert_eq!(batch.storage_identity(), identity);
        }
    }

    #[test]
    fn timeline_wraps_both_wire_fields_exactly() {
        let mut timeline = TxTimeline::new(
            SequenceNumber::new(u16::MAX),
            RtpTimestamp::new(u32::MAX - 239),
            240,
        );
        assert_eq!(timeline.current().0.get(), u16::MAX);
        timeline.advance();
        assert_eq!(
            timeline.current(),
            (SequenceNumber::new(0), RtpTimestamp::new(0))
        );
    }

    #[test]
    fn worker_buffers_keep_pointer_and_capacity_through_processing_and_reset() {
        let config = AudioPipelineConfig::new(AudioPipelineConfigInput {
            capture_rate_hz: 48_000,
            playback_rate_hz: 48_000,
            channels: 2,
            frame_duration: FrameDuration::Ms5,
            capture_src_chunk_frames: 480,
            capture_ring_samples: 960,
            playback_ring_samples: 16_000,
            tx_accumulator_samples: 16_000,
            reorder_capacity: 64,
            network_capacity: 8,
            network_due_batch_capacity: 4,
            packet_capacity: MAX_PACKET_BYTES,
            controller_cadence_frames: 480,
            clock_recovery: ClockRecoveryConfig::default(),
            adaptive_clock: AdaptiveClockConfig::default(),
        })
        .expect("config");
        let policy = EncoderPolicyV1::new(
            Bitrate::try_new(128_000).expect("bitrate"),
            InbandFec::Disabled,
            PacketLossPercent::ZERO,
        );
        let mut worker = TxWorker::new(
            config,
            TxStreamConfig {
                ssrc: Ssrc::new(1),
                payload_type: PayloadType::new(111).expect("payload type"),
                initial_sequence: SequenceNumber::new(0),
                initial_timestamp: RtpTimestamp::new(0),
                encoding_policy: policy,
            },
        )
        .expect("worker");
        let identity = worker.storage_identity();
        let input = vec![0.0; worker.capture_chunk_samples()];
        let mut batch = PacketBatch::new(1).expect("batch");
        for _ in 0..32 {
            assert!(matches!(
                worker.process_capture(CaptureInput::Chunk(&input), &mut batch),
                TxProcessOutcome::BatchFull(_) | TxProcessOutcome::Complete(_)
            ));
            batch.clear();
        }
        worker
            .reset(SequenceNumber::new(9), RtpTimestamp::new(10))
            .expect("reset");
        assert_eq!(worker.storage_identity(), identity);
    }

    fn test_config(
        capture_rate_hz: usize,
        duration: FrameDuration,
        chunk_frames: usize,
    ) -> AudioPipelineConfig {
        AudioPipelineConfig::new(AudioPipelineConfigInput {
            capture_rate_hz,
            playback_rate_hz: 48_000,
            channels: 2,
            frame_duration: duration,
            capture_src_chunk_frames: chunk_frames,
            capture_ring_samples: chunk_frames * 2,
            playback_ring_samples: 16_000,
            tx_accumulator_samples: 16_000,
            reorder_capacity: 64,
            network_capacity: 8,
            network_due_batch_capacity: 4,
            packet_capacity: MAX_PACKET_BYTES,
            controller_cadence_frames: 480,
            clock_recovery: ClockRecoveryConfig::default(),
            adaptive_clock: AdaptiveClockConfig::default(),
        })
        .expect("config")
    }

    fn test_stream() -> TxStreamConfig {
        TxStreamConfig {
            ssrc: Ssrc::new(1),
            payload_type: PayloadType::new(111).expect("payload type"),
            initial_sequence: SequenceNumber::new(10),
            initial_timestamp: RtpTimestamp::new(20),
            encoding_policy: EncoderPolicyV1::new(
                Bitrate::try_new(128_000).expect("bitrate"),
                InbandFec::Disabled,
                PacketLossPercent::ZERO,
            ),
        }
    }

    #[test]
    fn invalid_chunk_is_recoverable_without_draining_prior_output() {
        let mut worker = TxWorker::new(test_config(48_000, FrameDuration::Ms5, 480), test_stream())
            .expect("worker");
        let good = vec![0.0; worker.capture_chunk_samples()];
        let mut batch = PacketBatch::new(1).expect("batch");
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&good), &mut batch),
            TxProcessOutcome::BatchFull(_)
        ));
        batch.clear();
        let retained = worker.accumulated_media_frames();
        let mut bad = good.clone();
        bad[5] = f32::NAN;
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&bad), &mut batch),
            TxProcessOutcome::Error(TxProcessFailure {
                cause: TxError::Resampler(ResampleError::NonFiniteInput { sample_index: 5 }),
                ..
            })
        ));
        assert!(batch.is_empty());
        assert_eq!(worker.accumulated_media_frames(), retained);
        assert_eq!(
            worker.packetizer.timeline.current().0,
            SequenceNumber::new(11)
        );
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&good), &mut batch),
            TxProcessOutcome::BatchFull(report)
                if !report.input_pending
                    && report.capture_frames_consumed == 480
                    && report.packets_emitted == 1
        ));
    }

    #[test]
    fn backend_and_nonfinite_output_errors_fault_live_worker() {
        for injected in [
            ResampleError::Backend,
            ResampleError::NonFiniteOutput { sample_index: 7 },
        ] {
            let mut worker =
                TxWorker::new(test_config(44_100, FrameDuration::Ms10, 441), test_stream())
                    .expect("worker");
            worker.next_resampler_error = Some(injected);
            let input = vec![0.0; worker.capture_chunk_samples()];
            let mut batch = PacketBatch::new(4).expect("batch");
            assert!(matches!(
                worker.process_capture(CaptureInput::Chunk(&input), &mut batch),
                TxProcessOutcome::Error(TxProcessFailure {
                    cause: TxError::Resampler(error), ..
                }) if error == injected
            ));
            assert!(matches!(
                worker.process_capture(CaptureInput::Chunk(&input), &mut batch),
                TxProcessOutcome::Error(TxProcessFailure {
                    cause: TxError::Faulted,
                    ..
                })
            ));
        }
    }

    #[test]
    fn failed_encode_retains_pcm_and_does_not_advance_timeline() {
        let mut worker = TxWorker::new(test_config(48_000, FrameDuration::Ms5, 240), test_stream())
            .expect("worker");
        worker.packetizer.fail_encode_after = Some(0);
        let input = vec![0.0; worker.capture_chunk_samples()];
        let mut batch = PacketBatch::new(1).expect("batch");
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&input), &mut batch),
            TxProcessOutcome::Error(TxProcessFailure {
                cause: TxError::Opus(_),
                ..
            })
        ));
        assert_eq!(worker.accumulated_media_frames(), 240);
        assert_eq!(
            worker.packetizer.timeline.current(),
            (SequenceNumber::new(10), RtpTimestamp::new(20))
        );
        assert!(batch.is_empty());
    }

    #[test]
    fn failed_reset_is_staged_and_leaves_worker_faulted() {
        let mut worker =
            TxWorker::new(test_config(48_000, FrameDuration::Ms20, 480), test_stream())
                .expect("worker");
        let input = vec![0.0; worker.capture_chunk_samples()];
        let mut batch = PacketBatch::new(2).expect("batch");
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&input), &mut batch),
            TxProcessOutcome::Complete(_)
        ));
        let retained = worker.accumulated_media_frames();
        worker.packetizer.fail_next_reset = true;
        assert!(matches!(
            worker.reset(SequenceNumber::new(30), RtpTimestamp::new(40)),
            Err(TxError::Opus(_))
        ));
        assert_eq!(worker.accumulated_media_frames(), retained);
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&input), &mut batch),
            TxProcessOutcome::Error(TxProcessFailure {
                cause: TxError::Faulted,
                ..
            })
        ));
        worker
            .reset(SequenceNumber::new(30), RtpTimestamp::new(40))
            .expect("retry reset");
    }

    #[test]
    fn finite_encode_error_reports_partial_progress_and_faults_one_shot_worker() {
        let frames = 960;
        let mut worker = FiniteTxWorker::new(
            test_config(48_000, FrameDuration::Ms5, 480),
            test_stream(),
            frames,
        )
        .expect("worker");
        worker.packetizer.fail_encode_after = Some(1);
        let input = vec![0.0; frames * 2];
        let mut batch = PacketBatch::new(8).expect("batch");
        let error = worker
            .process_finite(&input, FinalFramePolicy::ZeroPad, &mut batch)
            .expect_err("second encode fails");
        assert!(matches!(error.cause, TxError::Opus(_)));
        assert_eq!(error.input_frames_consumed, frames);
        assert_eq!(error.packets_emitted, 1);
        assert_eq!(batch.len(), 1);
        assert_eq!(worker.state, FiniteState::Faulted);
        batch.clear();
        let repeated = worker
            .process_finite(&input, FinalFramePolicy::ZeroPad, &mut batch)
            .expect_err("fault is terminal");
        assert_eq!(repeated.cause, TxError::Faulted);
    }

    #[test]
    fn live_encode_error_reports_packets_committed_before_fault() {
        let mut worker = TxWorker::new(test_config(48_000, FrameDuration::Ms5, 960), test_stream())
            .expect("worker");
        let input = vec![0.0; worker.capture_chunk_samples()];
        let mut first_batch = PacketBatch::new(1).expect("batch");
        assert!(matches!(
            worker.process_capture(CaptureInput::Chunk(&input), &mut first_batch),
            TxProcessOutcome::BatchFull(_)
        ));
        first_batch.clear();

        worker.packetizer.fail_encode_after = Some(1);
        let mut batch = PacketBatch::new(4).expect("batch");
        let outcome = worker.process_capture(CaptureInput::Disconnected, &mut batch);
        let TxProcessOutcome::Error(failure) = outcome else {
            panic!("unexpected outcome: {outcome:?}");
        };
        assert!(matches!(failure.cause, TxError::Opus(_)));
        assert_eq!(failure.progress.packets_emitted, 1);
        assert_eq!(failure.progress.capture_frames_consumed, 0);
        assert_eq!(batch.len(), 1);
        assert!(matches!(worker.state, LiveState::Faulted));
    }
}
