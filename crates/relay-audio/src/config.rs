use core::fmt;

use relay_clock::{ClockError, ClockRecovery, ClockRecoveryConfig};
use relay_opus::{CHANNELS, FrameDuration, MAX_PACKET_BYTES};
use relay_resample::{
    AdaptiveClockConfig, AdaptiveClockConverter, FixedRatioConverter, FrameRequirements,
    ResampleError, SUPPORTED_SAMPLE_RATES, WorkerResampler,
};

use crate::{
    DeterministicNetwork, DueBatch, DueBatchError, MediaPacket, NetworkConfigError, PacketError,
    PayloadType, RtpTimestamp, SequenceNumber, Ssrc,
};

const SEQUENCE_HALF_RANGE: usize = 1 << 15;
const MEDIA_RATE_HZ: usize = 48_000;

/// Untrusted construction values for [`AudioPipelineConfig`].
///
/// Ring and accumulator capacities are scalar interleaved sample counts. This
/// representation deliberately makes channel alignment a checked boundary
/// condition instead of silently rounding a caller's request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioPipelineConfigInput {
    /// Capture-device sample rate.
    pub capture_rate_hz: usize,
    /// Playback-device sample rate.
    pub playback_rate_hz: usize,
    /// Interleaved channel count; V1 requires stereo.
    pub channels: usize,
    /// Negotiated, fixed Opus packet duration.
    pub frame_duration: FrameDuration,
    /// Input frames requested for each live capture-to-media SRC transaction.
    pub capture_src_chunk_frames: usize,
    /// Capture-ring capacity in scalar interleaved samples.
    pub capture_ring_samples: usize,
    /// Playback-ring capacity in scalar interleaved samples.
    pub playback_ring_samples: usize,
    /// 48 kHz TX-accumulator capacity in scalar interleaved samples.
    pub tx_accumulator_samples: usize,
    /// Packet capacity of the reorder window.
    pub reorder_capacity: usize,
    /// Scheduled-copy capacity of the deterministic network.
    pub network_capacity: usize,
    /// Maximum packets returned by one deterministic-network advance.
    pub network_due_batch_capacity: usize,
    /// Maximum encoded payload length accepted for this pipeline.
    pub packet_capacity: usize,
    /// Number of local playback frames between controller updates.
    pub controller_cadence_frames: usize,
    /// Clock-recovery policy validated together with controller cadence.
    pub clock_recovery: ClockRecoveryConfig,
    /// Adaptive playback-SRC policy validated against clock-recovery output.
    pub adaptive_clock: AdaptiveClockConfig,
}

/// A fully validated, immutable V1 audio-pipeline shape.
///
/// Construction is a control/worker-thread operation: it constructs the fixed
/// and adaptive converters once to obtain their authoritative requirements,
/// then drops those temporary instances. The retained shape establishes one
/// complete capture, append-before-drain TX, and playback publication
/// transaction before workers or rings are created.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioPipelineConfig {
    input: AudioPipelineConfigInput,
    opus_packet_samples: usize,
    fixed_requirements: FrameRequirements,
    adaptive_requirements: FrameRequirements,
    minimum_capture_ring_samples: usize,
    minimum_tx_accumulator_samples: usize,
    minimum_playback_ring_samples: usize,
}

impl AudioPipelineConfig {
    /// Validates an externally supplied pipeline shape off the realtime path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for unsupported media settings, invalid clock or
    /// SRC policy, incompatible cadence/correction bounds, insufficient fixed
    /// transaction storage, or checked arithmetic overflow.
    pub fn new(input: AudioPipelineConfigInput) -> Result<Self, ConfigError> {
        validate_rate("capture_rate_hz", input.capture_rate_hz)?;
        validate_rate("playback_rate_hz", input.playback_rate_hz)?;
        if input.channels != usize::from(CHANNELS) {
            return Err(ConfigError::UnsupportedChannelCount(input.channels));
        }
        validate_nonzero("capture_src_chunk_frames", input.capture_src_chunk_frames)?;

        let opus_packet_samples = input.frame_duration.interleaved_samples();
        let opus_packet_frames = opus_packet_samples / input.channels;

        let fixed = FixedRatioConverter::new(
            input.capture_rate_hz,
            MEDIA_RATE_HZ,
            input.channels,
            input.capture_src_chunk_frames,
        )
        .map_err(ConfigError::InvalidFixedResamplerConfiguration)?;
        let fixed_requirements = fixed.requirements();

        ClockRecovery::new(input.clock_recovery)
            .map_err(ConfigError::InvalidClockRecoveryConfiguration)?;
        let adaptive = AdaptiveClockConverter::new(
            MEDIA_RATE_HZ,
            input.playback_rate_hz,
            input.channels,
            opus_packet_frames,
            input.adaptive_clock,
        )
        .map_err(ConfigError::InvalidAdaptiveResamplerConfiguration)?;
        let adaptive_requirements = adaptive.requirements();

        if input.adaptive_clock.max_correction_ppm < input.clock_recovery.max_abs_correction_ppm {
            return Err(ConfigError::AdaptiveCorrectionRangeTooSmall {
                adaptive_ppm: input.adaptive_clock.max_correction_ppm,
                recovery_ppm: input.clock_recovery.max_abs_correction_ppm,
            });
        }

        validate_nonzero("controller_cadence_frames", input.controller_cadence_frames)?;
        if !cadence_fits_exactly(
            input.controller_cadence_frames,
            input.playback_rate_hz,
            input.clock_recovery.max_update_interval_seconds,
        ) {
            return Err(ConfigError::ControllerCadenceExceedsRecoveryMaximum {
                cadence_frames: input.controller_cadence_frames,
                playback_rate_hz: input.playback_rate_hz,
                maximum_seconds: input.clock_recovery.max_update_interval_seconds,
            });
        }

        let minimum_capture_ring_samples = checked_samples(
            "capture_ring_samples",
            fixed_requirements.input_frames_next,
            input.channels,
        )?;
        let fixed_output_samples = checked_samples(
            "tx_accumulator_samples",
            fixed_requirements.output_frames_max,
            input.channels,
        )?;
        let maximum_residual = opus_packet_samples
            .checked_sub(input.channels)
            .ok_or(ConfigError::CapacityOverflow("tx_accumulator_samples"))?;
        let minimum_tx_accumulator_samples = maximum_residual
            .checked_add(fixed_output_samples)
            .ok_or(ConfigError::CapacityOverflow("tx_accumulator_samples"))?;
        let minimum_playback_ring_samples = checked_samples(
            "playback_ring_samples",
            adaptive_requirements.output_frames_max,
            input.channels,
        )?;

        validate_transaction_capacity(
            "capture_ring_samples",
            input.capture_ring_samples,
            input.channels,
            minimum_capture_ring_samples,
        )?;
        validate_transaction_capacity(
            "tx_accumulator_samples",
            input.tx_accumulator_samples,
            input.channels,
            minimum_tx_accumulator_samples,
        )?;
        validate_transaction_capacity(
            "playback_ring_samples",
            input.playback_ring_samples,
            input.channels,
            minimum_playback_ring_samples,
        )?;

        validate_nonzero("reorder_capacity", input.reorder_capacity)?;
        if input.reorder_capacity >= SEQUENCE_HALF_RANGE {
            return Err(ConfigError::ReorderCapacityAtOrAboveHalfRange(
                input.reorder_capacity,
            ));
        }
        validate_nonzero("network_capacity", input.network_capacity)?;
        validate_nonzero(
            "network_due_batch_capacity",
            input.network_due_batch_capacity,
        )?;
        if input.network_due_batch_capacity > input.network_capacity {
            return Err(ConfigError::DueBatchExceedsNetworkCapacity {
                batch: input.network_due_batch_capacity,
                network: input.network_capacity,
            });
        }
        validate_nonzero("packet_capacity", input.packet_capacity)?;
        if input.packet_capacity > MAX_PACKET_BYTES {
            return Err(ConfigError::PacketCapacityTooLarge {
                maximum: MAX_PACKET_BYTES,
                actual: input.packet_capacity,
            });
        }

        // These are the exact scalar allocations owned by later ring builders.
        // Network and due-batch layout checks deliberately remain in their
        // owning constructors, reached through the factories below.
        checked_bytes::<f32>("capture_ring_samples", input.capture_ring_samples)?;
        checked_bytes::<f32>("playback_ring_samples", input.playback_ring_samples)?;
        checked_bytes::<f32>("tx_accumulator_samples", input.tx_accumulator_samples)?;

        Ok(Self {
            input,
            opus_packet_samples,
            fixed_requirements,
            adaptive_requirements,
            minimum_capture_ring_samples,
            minimum_tx_accumulator_samples,
            minimum_playback_ring_samples,
        })
    }

    /// Constructs the configured capture-to-48 kHz converter off-thread.
    pub fn create_fixed_resampler(self) -> Result<FixedRatioConverter, ResampleError> {
        FixedRatioConverter::new(
            self.capture_rate_hz(),
            MEDIA_RATE_HZ,
            self.channels(),
            self.capture_src_chunk_frames(),
        )
    }

    /// Constructs the configured adaptive 48 kHz-to-playback converter off-thread.
    pub fn create_adaptive_resampler(self) -> Result<AdaptiveClockConverter, ResampleError> {
        AdaptiveClockConverter::new(
            MEDIA_RATE_HZ,
            self.playback_rate_hz(),
            self.channels(),
            self.opus_packet_samples / self.channels(),
            self.input.adaptive_clock,
        )
    }

    /// Constructs the validated clock-recovery controller off-thread.
    pub fn create_clock_recovery(self) -> Result<ClockRecovery, ClockError> {
        ClockRecovery::new(self.input.clock_recovery)
    }

    /// Constructs exact scheduled storage using the owning network constructor.
    pub fn create_deterministic_network(self) -> Result<DeterministicNetwork, NetworkConfigError> {
        DeterministicNetwork::new(self.network_capacity(), self.network_due_batch_capacity())
    }

    /// Constructs exact due storage using the owning batch constructor.
    pub fn create_due_batch(self) -> Result<DueBatch, DueBatchError> {
        DueBatch::new(self.network_due_batch_capacity())
    }

    /// Creates one typed packet while enforcing this pipeline's payload bound.
    pub fn create_media_packet(
        self,
        ssrc: Ssrc,
        sequence: SequenceNumber,
        timestamp: RtpTimestamp,
        payload_type: PayloadType,
        payload: &[u8],
    ) -> Result<MediaPacket, PacketError> {
        MediaPacket::new_with_max_payload(
            ssrc,
            sequence,
            timestamp,
            payload_type,
            payload,
            self.packet_capacity(),
        )
    }

    /// Validates raw wire fields and enforces this pipeline's payload bound.
    pub fn try_create_media_packet(
        self,
        ssrc: u32,
        sequence: u16,
        timestamp: u32,
        payload_type: u8,
        payload: &[u8],
    ) -> Result<MediaPacket, PacketError> {
        let payload_type = PayloadType::new(payload_type)
            .map_err(|error| PacketError::InvalidPayloadType(error.0))?;
        self.create_media_packet(
            Ssrc::new(ssrc),
            SequenceNumber::new(sequence),
            RtpTimestamp::new(timestamp),
            payload_type,
            payload,
        )
    }

    /// Validates an existing packet against this pipeline's payload bound.
    pub fn validate_media_packet(self, packet: &MediaPacket) -> Result<(), PacketError> {
        if packet.payload_len() > self.packet_capacity() {
            Err(PacketError::PayloadTooLarge {
                maximum: self.packet_capacity(),
                actual: packet.payload_len(),
            })
        } else {
            Ok(())
        }
    }

    /// Returns the capture-device sample rate.
    #[must_use]
    pub const fn capture_rate_hz(self) -> usize {
        self.input.capture_rate_hz
    }

    /// Returns the playback-device sample rate.
    #[must_use]
    pub const fn playback_rate_hz(self) -> usize {
        self.input.playback_rate_hz
    }

    /// Returns the fixed stereo channel count.
    #[must_use]
    pub const fn channels(self) -> usize {
        self.input.channels
    }

    /// Returns the negotiated Opus duration.
    #[must_use]
    pub const fn frame_duration(self) -> FrameDuration {
        self.input.frame_duration
    }

    /// Returns the requested capture SRC input chunk size.
    #[must_use]
    pub const fn capture_src_chunk_frames(self) -> usize {
        self.input.capture_src_chunk_frames
    }

    /// Returns the capture-ring scalar-sample capacity.
    #[must_use]
    pub const fn capture_ring_samples(self) -> usize {
        self.input.capture_ring_samples
    }

    /// Returns the playback-ring scalar-sample capacity.
    #[must_use]
    pub const fn playback_ring_samples(self) -> usize {
        self.input.playback_ring_samples
    }

    /// Returns the TX-accumulator scalar-sample capacity.
    #[must_use]
    pub const fn tx_accumulator_samples(self) -> usize {
        self.input.tx_accumulator_samples
    }

    /// Returns the bounded reorder-window packet capacity.
    #[must_use]
    pub const fn reorder_capacity(self) -> usize {
        self.input.reorder_capacity
    }

    /// Returns the deterministic-network scheduled-copy capacity.
    #[must_use]
    pub const fn network_capacity(self) -> usize {
        self.input.network_capacity
    }

    /// Returns the per-advance due-packet bound.
    #[must_use]
    pub const fn network_due_batch_capacity(self) -> usize {
        self.input.network_due_batch_capacity
    }

    /// Returns the accepted encoded-payload capacity.
    #[must_use]
    pub const fn packet_capacity(self) -> usize {
        self.input.packet_capacity
    }

    /// Returns the controller cadence measured in local playback frames.
    #[must_use]
    pub const fn controller_cadence_frames(self) -> usize {
        self.input.controller_cadence_frames
    }

    /// Returns the validated clock-recovery policy.
    #[must_use]
    pub const fn clock_recovery_config(self) -> ClockRecoveryConfig {
        self.input.clock_recovery
    }

    /// Returns the validated adaptive SRC policy.
    #[must_use]
    pub const fn adaptive_clock_config(self) -> AdaptiveClockConfig {
        self.input.adaptive_clock
    }

    /// Returns scalar 48 kHz stereo samples in one Opus packet.
    #[must_use]
    pub const fn opus_packet_samples(self) -> usize {
        self.opus_packet_samples
    }

    /// Returns authoritative fixed capture-SRC buffer requirements.
    #[must_use]
    pub const fn fixed_resampler_requirements(self) -> FrameRequirements {
        self.fixed_requirements
    }

    /// Returns authoritative adaptive playback-SRC buffer requirements.
    #[must_use]
    pub const fn adaptive_resampler_requirements(self) -> FrameRequirements {
        self.adaptive_requirements
    }

    /// Returns the minimum complete capture transaction in scalar samples.
    #[must_use]
    pub const fn minimum_capture_ring_samples(self) -> usize {
        self.minimum_capture_ring_samples
    }

    /// Returns the residual-plus-fixed-output TX minimum in scalar samples.
    #[must_use]
    pub const fn minimum_tx_accumulator_samples(self) -> usize {
        self.minimum_tx_accumulator_samples
    }

    /// Returns the minimum all-or-drop playback publication in scalar samples.
    #[must_use]
    pub const fn minimum_playback_ring_samples(self) -> usize {
        self.minimum_playback_ring_samples
    }
}

/// Why raw pipeline settings could not become an [`AudioPipelineConfig`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConfigError {
    /// A device rate is not supported by the Phase-1 resampler boundary.
    UnsupportedSampleRate {
        /// Name of the rejected field.
        name: &'static str,
        /// Rejected rate.
        rate_hz: usize,
    },
    /// V1 media is interleaved stereo only.
    UnsupportedChannelCount(usize),
    /// A named capacity/cadence was zero.
    ZeroValue(&'static str),
    /// A scalar-sample capacity ended partway through an interleaved frame.
    IncompleteInterleavedFrame {
        /// Name of the rejected field.
        name: &'static str,
        /// Rejected scalar-sample count.
        samples: usize,
        /// Required channel alignment.
        channels: usize,
    },
    /// A buffer could not hold one complete derived worker transaction.
    CapacityTooSmall {
        /// Name of the rejected field.
        name: &'static str,
        /// Smallest accepted scalar-sample count.
        minimum: usize,
        /// Rejected scalar-sample count.
        actual: usize,
    },
    /// Sequence ordering is ambiguous at and above the 16-bit half range.
    ReorderCapacityAtOrAboveHalfRange(usize),
    /// A due batch cannot exceed the queue from which it drains.
    DueBatchExceedsNetworkCapacity {
        /// Rejected batch capacity.
        batch: usize,
        /// Network queue capacity.
        network: usize,
    },
    /// An encoded-payload capacity exceeded fixed inline packet storage.
    PacketCapacityTooLarge {
        /// Fixed inline maximum.
        maximum: usize,
        /// Rejected requested capacity.
        actual: usize,
    },
    /// Clock recovery rejected its numeric policy.
    InvalidClockRecoveryConfiguration(ClockError),
    /// The fixed capture converter rejected its construction policy.
    InvalidFixedResamplerConfiguration(ResampleError),
    /// The adaptive playback converter rejected its construction policy.
    InvalidAdaptiveResamplerConfiguration(ResampleError),
    /// The adaptive SRC clamp did not contain every recovery command.
    AdaptiveCorrectionRangeTooSmall {
        /// Configured adaptive SRC clamp.
        adaptive_ppm: f64,
        /// Maximum absolute controller output.
        recovery_ppm: f64,
    },
    /// The configured cadence exceeds the recovery controller's trusted gap.
    ControllerCadenceExceedsRecoveryMaximum {
        /// Rejected playback-frame cadence.
        cadence_frames: usize,
        /// Local playback rate defining the interval.
        playback_rate_hz: usize,
        /// Recovery controller's maximum interval.
        maximum_seconds: f64,
    },
    /// Derived frame/sample/byte arithmetic exceeded `usize`.
    CapacityOverflow(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ConfigError {}

fn validate_rate(name: &'static str, rate_hz: usize) -> Result<(), ConfigError> {
    if SUPPORTED_SAMPLE_RATES.contains(&rate_hz) {
        Ok(())
    } else {
        Err(ConfigError::UnsupportedSampleRate { name, rate_hz })
    }
}

fn validate_nonzero(name: &'static str, value: usize) -> Result<(), ConfigError> {
    if value == 0 {
        Err(ConfigError::ZeroValue(name))
    } else {
        Ok(())
    }
}

fn validate_transaction_capacity(
    name: &'static str,
    samples: usize,
    channels: usize,
    minimum: usize,
) -> Result<(), ConfigError> {
    validate_nonzero(name, samples)?;
    if !samples.is_multiple_of(channels) {
        return Err(ConfigError::IncompleteInterleavedFrame {
            name,
            samples,
            channels,
        });
    }
    if samples < minimum {
        Err(ConfigError::CapacityTooSmall {
            name,
            minimum,
            actual: samples,
        })
    } else {
        Ok(())
    }
}

fn checked_samples(
    name: &'static str,
    frames: usize,
    channels: usize,
) -> Result<usize, ConfigError> {
    frames
        .checked_mul(channels)
        .ok_or(ConfigError::CapacityOverflow(name))
}

fn checked_bytes<T>(name: &'static str, count: usize) -> Result<(), ConfigError> {
    count
        .checked_mul(core::mem::size_of::<T>())
        .map(|_| ())
        .ok_or(ConfigError::CapacityOverflow(name))
}

// Compare `cadence_frames / rate_hz <= maximum_seconds` against the exact
// dyadic rational represented by the positive finite f64, without rounding the
// frame boundary through floating-point division or multiplication.
fn cadence_fits_exactly(cadence_frames: usize, rate_hz: usize, maximum_seconds: f64) -> bool {
    let bits = maximum_seconds.to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, exponent) = if exponent_bits == 0 {
        (u128::from(fraction), -1074)
    } else {
        (
            u128::from((1_u64 << 52) | fraction),
            exponent_bits - 1023 - 52,
        )
    };
    let right_base = (rate_hz as u128) * significand;
    if exponent >= 0 {
        let Ok(shift) = u32::try_from(exponent) else {
            return true;
        };
        let Some(factor) = 1_u128.checked_shl(shift) else {
            return true;
        };
        right_base
            .checked_mul(factor)
            .is_none_or(|right| cadence_frames as u128 <= right)
    } else {
        let Ok(shift) = u32::try_from(-exponent) else {
            return false;
        };
        let Some(factor) = 1_u128.checked_shl(shift) else {
            return false;
        };
        (cadence_frames as u128)
            .checked_mul(factor)
            .is_some_and(|left| left <= right_base)
    }
}
