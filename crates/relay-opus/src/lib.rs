//! Safe, fixed-format Opus encoding and decoding for RELAY.
//!
//! The boundary intentionally supports only interleaved stereo `f32` PCM at
//! 48 kHz and 5, 10, or 20 ms frames. Codec state is allocated by libopus at
//! construction. Encoding, decoding, PLC, FEC decode, and encoder controls use
//! caller-owned buffers and perform no Rust allocation, logging, or locking.

#![forbid(unsafe_code)]

use core::fmt;

/// The only PCM sample rate supported by this boundary.
pub const SAMPLE_RATE_HZ: u32 = 48_000;
/// The only channel count supported by this boundary.
pub const CHANNELS: u8 = 2;
/// The maximum packet capacity passed to libopus.
///
/// The upstream API recommends 4000 bytes as the encoder packet buffer size.
pub const MAX_PACKET_BYTES: usize = 4_000;

/// A supported Opus frame duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDuration {
    Ms5,
    Ms10,
    Ms20,
}

impl FrameDuration {
    #[must_use]
    pub const fn milliseconds(self) -> u16 {
        match self {
            Self::Ms5 => 5,
            Self::Ms10 => 10,
            Self::Ms20 => 20,
        }
    }

    /// Number of samples in one channel at 48 kHz.
    #[must_use]
    pub const fn samples_per_channel(self) -> usize {
        match self {
            Self::Ms5 => 240,
            Self::Ms10 => 480,
            Self::Ms20 => 960,
        }
    }

    /// Number of samples in one interleaved stereo frame.
    #[must_use]
    pub const fn interleaved_samples(self) -> usize {
        self.samples_per_channel() * CHANNELS as usize
    }
}

impl TryFrom<u16> for FrameDuration {
    type Error = Error;

    fn try_from(milliseconds: u16) -> Result<Self, Self::Error> {
        match milliseconds {
            5 => Ok(Self::Ms5),
            10 => Ok(Self::Ms10),
            20 => Ok(Self::Ms20),
            value => Err(Error::UnsupportedFrameDuration(value)),
        }
    }
}

/// Opus encoder application selected by a policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Application {
    Voice,
    Audio,
    RestrictedLowDelay,
}

impl Application {
    const fn as_raw(self) -> i32 {
        match self {
            Self::Voice => relay_opus_sys::OPUS_APPLICATION_VOIP,
            Self::Audio => relay_opus_sys::OPUS_APPLICATION_AUDIO,
            Self::RestrictedLowDelay => relay_opus_sys::OPUS_APPLICATION_RESTRICTED_LOWDELAY,
        }
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            relay_opus_sys::OPUS_APPLICATION_VOIP => Ok(Self::Voice),
            relay_opus_sys::OPUS_APPLICATION_AUDIO => Ok(Self::Audio),
            relay_opus_sys::OPUS_APPLICATION_RESTRICTED_LOWDELAY => Ok(Self::RestrictedLowDelay),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// A concrete libopus bitrate in bits per second.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bitrate(i32);

impl Bitrate {
    pub const MIN_BPS: i32 = relay_opus_sys::MIN_BITRATE_BPS;
    pub const MAX_BPS: i32 = relay_opus_sys::MAX_BITRATE_BPS;

    /// Checks the meaningful concrete bitrate range documented by libopus 1.6.
    pub const fn try_new(bps: i32) -> Result<Self, Error> {
        if bps < Self::MIN_BPS || bps > Self::MAX_BPS {
            return Err(Error::InvalidBitrate(bps));
        }
        Ok(Self(bps))
    }

    #[must_use]
    pub const fn bps(self) -> i32 {
        self.0
    }
}

/// A libopus encoder complexity in the inclusive range 0 through 10.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Complexity(i32);

impl Complexity {
    pub const MIN: i32 = relay_opus_sys::MIN_COMPLEXITY;
    pub const MAX: i32 = relay_opus_sys::MAX_COMPLEXITY;
    pub const MAXIMUM: Self = Self(Self::MAX);

    pub const fn try_new(value: i32) -> Result<Self, Error> {
        if value < Self::MIN || value > Self::MAX {
            return Err(Error::InvalidComplexity(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// Libopus variable-bitrate mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VbrMode {
    Disabled,
    Enabled,
}

impl VbrMode {
    const fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Disabled),
            1 => Ok(Self::Enabled),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// Whether VBR may exceed the nominal bitrate over short intervals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VbrConstraint {
    Unconstrained,
    Constrained,
}

impl VbrConstraint {
    const fn constrained(self) -> bool {
        matches!(self, Self::Constrained)
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Unconstrained),
            1 => Ok(Self::Constrained),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// Encoder bandpass selection or a concrete band limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bandwidth {
    Auto,
    Narrowband,
    Mediumband,
    Wideband,
    Superwideband,
    Fullband,
}

impl Bandwidth {
    const fn as_raw(self) -> i32 {
        match self {
            Self::Auto => relay_opus_sys::OPUS_AUTO,
            Self::Narrowband => relay_opus_sys::OPUS_BANDWIDTH_NARROWBAND,
            Self::Mediumband => relay_opus_sys::OPUS_BANDWIDTH_MEDIUMBAND,
            Self::Wideband => relay_opus_sys::OPUS_BANDWIDTH_WIDEBAND,
            Self::Superwideband => relay_opus_sys::OPUS_BANDWIDTH_SUPERWIDEBAND,
            Self::Fullband => relay_opus_sys::OPUS_BANDWIDTH_FULLBAND,
        }
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            relay_opus_sys::OPUS_AUTO => Ok(Self::Auto),
            relay_opus_sys::OPUS_BANDWIDTH_NARROWBAND => Ok(Self::Narrowband),
            relay_opus_sys::OPUS_BANDWIDTH_MEDIUMBAND => Ok(Self::Mediumband),
            relay_opus_sys::OPUS_BANDWIDTH_WIDEBAND => Ok(Self::Wideband),
            relay_opus_sys::OPUS_BANDWIDTH_SUPERWIDEBAND => Ok(Self::Superwideband),
            relay_opus_sys::OPUS_BANDWIDTH_FULLBAND => Ok(Self::Fullband),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// Signal-type hint supplied to libopus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    Auto,
    Voice,
    Music,
}

impl Signal {
    const fn as_raw(self) -> i32 {
        match self {
            Self::Auto => relay_opus_sys::OPUS_AUTO,
            Self::Voice => relay_opus_sys::OPUS_SIGNAL_VOICE,
            Self::Music => relay_opus_sys::OPUS_SIGNAL_MUSIC,
        }
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            relay_opus_sys::OPUS_AUTO => Ok(Self::Auto),
            relay_opus_sys::OPUS_SIGNAL_VOICE => Ok(Self::Voice),
            relay_opus_sys::OPUS_SIGNAL_MUSIC => Ok(Self::Music),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// Discontinuous-transmission mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DtxMode {
    Disabled,
    Enabled,
}

impl DtxMode {
    const fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Disabled),
            1 => Ok(Self::Enabled),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// In-band FEC modes defined by libopus 1.6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InbandFec {
    Disabled,
    Enabled,
    /// Enable FEC without necessarily switching music to SILK.
    EnabledWithoutSilkSwitch,
}

impl InbandFec {
    const fn as_raw(self) -> i32 {
        match self {
            Self::Disabled => 0,
            Self::Enabled => 1,
            Self::EnabledWithoutSilkSwitch => 2,
        }
    }

    fn from_raw(value: i32) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Disabled),
            1 => Ok(Self::Enabled),
            2 => Ok(Self::EnabledWithoutSilkSwitch),
            _ => Err(Error::InvalidCodecResult),
        }
    }
}

/// A libopus expected packet-loss hint in the inclusive range 0 through 100.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketLossPercent(i32);

impl PacketLossPercent {
    pub const MIN: i32 = relay_opus_sys::MIN_PACKET_LOSS_PERCENT;
    pub const MAX: i32 = relay_opus_sys::MAX_PACKET_LOSS_PERCENT;
    pub const ZERO: Self = Self(0);

    pub const fn try_new(percent: i32) -> Result<Self, Error> {
        if percent < Self::MIN || percent > Self::MAX {
            return Err(Error::InvalidPacketLossPercent(percent));
        }
        Ok(Self(percent))
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// Canonical, versioned encoder policy for the RELAY V1 music master path.
///
/// Bitrate and loss protection are negotiated. All other controls are fixed
/// product decisions and are exposed by getters so none depend on libopus defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncoderPolicyV1 {
    bitrate: Bitrate,
    inband_fec: InbandFec,
    expected_packet_loss_percent: PacketLossPercent,
}

impl EncoderPolicyV1 {
    pub const APPLICATION: Application = Application::Audio;
    pub const COMPLEXITY: Complexity = Complexity::MAXIMUM;
    pub const VBR: VbrMode = VbrMode::Enabled;
    /// V1 deliberately chooses constrained VBR for bounded short-term rate excursions.
    pub const VBR_CONSTRAINT: VbrConstraint = VbrConstraint::Constrained;
    /// Let libopus select the active bandpass for the negotiated bitrate.
    pub const BANDWIDTH: Bandwidth = Bandwidth::Auto;
    /// Permit the automatic selector to use the full 48 kHz-input band.
    pub const MAX_BANDWIDTH: Bandwidth = Bandwidth::Fullband;
    pub const SIGNAL: Signal = Signal::Music;
    pub const DTX: DtxMode = DtxMode::Disabled;

    #[must_use]
    pub const fn new(
        bitrate: Bitrate,
        inband_fec: InbandFec,
        expected_packet_loss_percent: PacketLossPercent,
    ) -> Self {
        Self {
            bitrate,
            inband_fec,
            expected_packet_loss_percent,
        }
    }

    #[must_use]
    pub const fn application(self) -> Application {
        Self::APPLICATION
    }

    #[must_use]
    pub const fn bitrate(self) -> Bitrate {
        self.bitrate
    }

    #[must_use]
    pub const fn complexity(self) -> Complexity {
        Self::COMPLEXITY
    }

    #[must_use]
    pub const fn vbr(self) -> VbrMode {
        Self::VBR
    }

    #[must_use]
    pub const fn vbr_constraint(self) -> VbrConstraint {
        Self::VBR_CONSTRAINT
    }

    #[must_use]
    pub const fn bandwidth(self) -> Bandwidth {
        Self::BANDWIDTH
    }

    #[must_use]
    pub const fn max_bandwidth(self) -> Bandwidth {
        Self::MAX_BANDWIDTH
    }

    #[must_use]
    pub const fn signal(self) -> Signal {
        Self::SIGNAL
    }

    #[must_use]
    pub const fn dtx(self) -> DtxMode {
        Self::DTX
    }

    #[must_use]
    pub const fn inband_fec(self) -> InbandFec {
        self.inband_fec
    }

    #[must_use]
    pub const fn expected_packet_loss_percent(self) -> PacketLossPercent {
        self.expected_packet_loss_percent
    }
}

/// Validated V1 encoder construction settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncoderConfigV1 {
    frame_duration: FrameDuration,
    policy: EncoderPolicyV1,
}

impl EncoderConfigV1 {
    /// Validates the externally negotiated format before constructing a config.
    pub const fn try_new(
        sample_rate_hz: u32,
        channels: u8,
        frame_duration: FrameDuration,
        policy: EncoderPolicyV1,
    ) -> Result<Self, Error> {
        if sample_rate_hz != SAMPLE_RATE_HZ {
            return Err(Error::UnsupportedSampleRate(sample_rate_hz));
        }
        if channels != CHANNELS {
            return Err(Error::UnsupportedChannelCount(channels));
        }
        Ok(Self::stereo_48k(frame_duration, policy))
    }

    #[must_use]
    pub const fn stereo_48k(frame_duration: FrameDuration, policy: EncoderPolicyV1) -> Self {
        Self {
            frame_duration,
            policy,
        }
    }

    #[must_use]
    pub const fn frame_duration(self) -> FrameDuration {
        self.frame_duration
    }

    #[must_use]
    pub const fn policy(self) -> EncoderPolicyV1 {
        self.policy
    }
}

/// Validated decoder construction settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecoderConfig {
    frame_duration: FrameDuration,
}

impl DecoderConfig {
    /// Validates the externally negotiated format before constructing a config.
    ///
    /// # Errors
    /// Returns an error unless `sample_rate_hz == 48000` and `channels == 2`.
    pub const fn try_new(
        sample_rate_hz: u32,
        channels: u8,
        frame_duration: FrameDuration,
    ) -> Result<Self, Error> {
        if sample_rate_hz != SAMPLE_RATE_HZ {
            return Err(Error::UnsupportedSampleRate(sample_rate_hz));
        }
        if channels != CHANNELS {
            return Err(Error::UnsupportedChannelCount(channels));
        }
        Ok(Self::stereo_48k(frame_duration))
    }

    #[must_use]
    pub const fn stereo_48k(frame_duration: FrameDuration) -> Self {
        Self { frame_duration }
    }

    #[must_use]
    pub const fn frame_duration(self) -> FrameDuration {
        self.frame_duration
    }
}

/// A successful decode's initialized prefix lengths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedSamples {
    samples_per_channel: usize,
    interleaved_samples: usize,
}

impl DecodedSamples {
    #[must_use]
    pub const fn samples_per_channel(self) -> usize {
        self.samples_per_channel
    }

    #[must_use]
    pub const fn interleaved_samples(self) -> usize {
        self.interleaved_samples
    }
}

/// An error reported by libopus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CodecError {
    BadArgument,
    BufferTooSmall,
    Internal,
    InvalidPacket,
    Unimplemented,
    InvalidState,
    AllocationFailed,
    Unknown(i32),
}

impl From<i32> for CodecError {
    fn from(code: i32) -> Self {
        match code {
            relay_opus_sys::OPUS_BAD_ARG => Self::BadArgument,
            relay_opus_sys::OPUS_BUFFER_TOO_SMALL => Self::BufferTooSmall,
            relay_opus_sys::OPUS_INTERNAL_ERROR => Self::Internal,
            relay_opus_sys::OPUS_INVALID_PACKET => Self::InvalidPacket,
            relay_opus_sys::OPUS_UNIMPLEMENTED => Self::Unimplemented,
            relay_opus_sys::OPUS_INVALID_STATE => Self::InvalidState,
            relay_opus_sys::OPUS_ALLOC_FAIL => Self::AllocationFailed,
            value => Self::Unknown(value),
        }
    }
}

/// Safe boundary validation or codec failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    UnsupportedSampleRate(u32),
    UnsupportedChannelCount(u8),
    UnsupportedFrameDuration(u16),
    InvalidBitrate(i32),
    InvalidComplexity(i32),
    InvalidPacketLossPercent(i32),
    InvalidPcmLength { expected: usize, actual: usize },
    OutputTooSmall { required: usize, actual: usize },
    EmptyPacket,
    PacketTooLarge { maximum: usize, actual: usize },
    UnexpectedDecodedDuration { expected: usize, actual: usize },
    InvalidCodecResult,
    InvalidVersionString,
    UnsupportedLibopusVersion,
    EncoderPolicyNotApplied,
    Codec(CodecError),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSampleRate(value) => {
                write!(formatter, "unsupported sample rate {value}; expected 48000")
            }
            Self::UnsupportedChannelCount(value) => {
                write!(formatter, "unsupported channel count {value}; expected 2")
            }
            Self::UnsupportedFrameDuration(value) => {
                write!(formatter, "unsupported frame duration {value} ms")
            }
            Self::InvalidBitrate(value) => {
                write!(formatter, "invalid Opus bitrate {value} bps")
            }
            Self::InvalidComplexity(value) => {
                write!(formatter, "invalid Opus complexity {value}")
            }
            Self::InvalidPacketLossPercent(value) => {
                write!(formatter, "invalid packet loss percentage {value}")
            }
            Self::InvalidPcmLength { expected, actual } => {
                write!(
                    formatter,
                    "invalid PCM length {actual}; expected {expected}"
                )
            }
            Self::OutputTooSmall { required, actual } => {
                write!(
                    formatter,
                    "output length {actual}; requires at least {required}"
                )
            }
            Self::EmptyPacket => formatter.write_str("empty packet; use decode_plc for loss"),
            Self::PacketTooLarge { maximum, actual } => {
                write!(formatter, "packet length {actual}; maximum is {maximum}")
            }
            Self::UnexpectedDecodedDuration { expected, actual } => {
                write!(
                    formatter,
                    "decoded {actual} samples per channel; negotiated duration requires {expected}"
                )
            }
            Self::InvalidCodecResult => formatter.write_str("libopus returned an invalid value"),
            Self::InvalidVersionString => formatter.write_str("libopus version is not valid UTF-8"),
            Self::UnsupportedLibopusVersion => {
                formatter.write_str("linked libopus does not meet the V1 minimum version 1.6")
            }
            Self::EncoderPolicyNotApplied => {
                formatter.write_str("encoder policy is not completely applied; reset is required")
            }
            Self::Codec(error) => write!(formatter, "libopus error: {error:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// A single-stream canonical V1 Opus encoder.
///
/// Construct this off the real-time thread. Steady-state encoding and control
/// operations use no Rust allocation, locks, or logging.
pub struct Encoder {
    inner: relay_opus_sys::Encoder,
    config: EncoderConfigV1,
    policy_applied: bool,
    #[cfg(test)]
    injected_policy_failure_step: Option<usize>,
}

impl Encoder {
    /// Allocates libopus state and applies every V1 control explicitly.
    pub fn new(config: EncoderConfigV1) -> Result<Self, Error> {
        ensure_supported_libopus()?;
        let policy = config.policy;
        let inner = relay_opus_sys::Encoder::new(
            SAMPLE_RATE_HZ as i32,
            CHANNELS.into(),
            policy.application().as_raw(),
        )
        .map_err(codec_error)?;
        let mut encoder = Self {
            inner,
            config,
            policy_applied: false,
            #[cfg(test)]
            injected_policy_failure_step: None,
        };
        encoder.apply_current_policy()?;
        Ok(encoder)
    }

    #[must_use]
    pub const fn config(&self) -> EncoderConfigV1 {
        self.config
    }

    /// Resets codec history and reapplies the complete V1 policy.
    ///
    /// The encoder is poisoned before libopus is mutated. If reset or any
    /// policy control fails, encode and all control access return
    /// [`Error::EncoderPolicyNotApplied`] until a later complete reset succeeds.
    pub fn reset(&mut self) -> Result<(), Error> {
        self.policy_applied = false;
        self.inner.reset().map_err(codec_error)?;
        self.apply_current_policy()
    }

    pub fn application(&mut self) -> Result<Application, Error> {
        self.require_policy()?;
        Application::from_raw(self.inner.application().map_err(codec_error)?)
    }

    pub fn bitrate(&mut self) -> Result<Bitrate, Error> {
        self.require_policy()?;
        let value = self.inner.bitrate().map_err(codec_error)?;
        Bitrate::try_new(value).map_err(|_| Error::InvalidCodecResult)
    }

    pub fn complexity(&mut self) -> Result<Complexity, Error> {
        self.require_policy()?;
        let value = self.inner.complexity().map_err(codec_error)?;
        Complexity::try_new(value).map_err(|_| Error::InvalidCodecResult)
    }

    pub fn vbr(&mut self) -> Result<VbrMode, Error> {
        self.require_policy()?;
        VbrMode::from_raw(self.inner.vbr().map_err(codec_error)?)
    }

    pub fn vbr_constraint(&mut self) -> Result<VbrConstraint, Error> {
        self.require_policy()?;
        VbrConstraint::from_raw(self.inner.vbr_constraint().map_err(codec_error)?)
    }

    pub fn max_bandwidth(&mut self) -> Result<Bandwidth, Error> {
        self.require_policy()?;
        Bandwidth::from_raw(self.inner.max_bandwidth().map_err(codec_error)?)
    }

    /// Returns libopus's currently selected active bandwidth.
    ///
    /// This is a concrete value even when V1 configured automatic selection;
    /// use `config().policy().bandwidth()` to inspect the `Auto` decision.
    pub fn bandwidth(&mut self) -> Result<Bandwidth, Error> {
        self.require_policy()?;
        Bandwidth::from_raw(self.inner.bandwidth().map_err(codec_error)?)
    }

    pub fn signal(&mut self) -> Result<Signal, Error> {
        self.require_policy()?;
        Signal::from_raw(self.inner.signal().map_err(codec_error)?)
    }

    pub fn dtx(&mut self) -> Result<DtxMode, Error> {
        self.require_policy()?;
        DtxMode::from_raw(self.inner.dtx().map_err(codec_error)?)
    }

    pub fn inband_fec(&mut self) -> Result<InbandFec, Error> {
        self.require_policy()?;
        InbandFec::from_raw(self.inner.inband_fec().map_err(codec_error)?)
    }

    pub fn expected_packet_loss_percent(&mut self) -> Result<PacketLossPercent, Error> {
        self.require_policy()?;
        let value = self.inner.packet_loss_percent().map_err(codec_error)?;
        PacketLossPercent::try_new(value).map_err(|_| Error::InvalidCodecResult)
    }

    /// Encodes exactly one configured interleaved stereo frame.
    ///
    /// At most [`MAX_PACKET_BYTES`] of `output` is exposed to libopus. The
    /// returned length identifies the initialized packet prefix.
    pub fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, Error> {
        self.require_policy()?;
        let expected = self.config.frame_duration.interleaved_samples();
        if pcm.len() != expected {
            return Err(Error::InvalidPcmLength {
                expected,
                actual: pcm.len(),
            });
        }
        if output.is_empty() {
            return Err(Error::OutputTooSmall {
                required: 1,
                actual: 0,
            });
        }
        let bounded_len = output.len().min(MAX_PACKET_BYTES);
        let written = self
            .inner
            .encode_float(
                pcm,
                self.config.frame_duration.samples_per_channel() as i32,
                &mut output[..bounded_len],
            )
            .map_err(codec_error)?;
        if written > bounded_len {
            return Err(Error::InvalidCodecResult);
        }
        Ok(written)
    }

    /// Updates the negotiated bitrate without changing the canonical controls.
    pub fn set_bitrate(&mut self, bitrate: Bitrate) -> Result<(), Error> {
        self.require_policy()?;
        self.inner.set_bitrate(bitrate.bps()).map_err(codec_error)?;
        self.config.policy.bitrate = bitrate;
        Ok(())
    }

    /// Updates the negotiated in-band FEC mode without allocating.
    pub fn set_inband_fec(&mut self, mode: InbandFec) -> Result<(), Error> {
        self.require_policy()?;
        self.inner
            .set_inband_fec(mode.as_raw())
            .map_err(codec_error)?;
        self.config.policy.inband_fec = mode;
        Ok(())
    }

    /// Updates the negotiated expected packet-loss hint without allocating.
    pub fn set_expected_packet_loss_percent(
        &mut self,
        percent: PacketLossPercent,
    ) -> Result<(), Error> {
        self.require_policy()?;
        self.inner
            .set_packet_loss_percent(percent.get())
            .map_err(codec_error)?;
        self.config.policy.expected_packet_loss_percent = percent;
        Ok(())
    }

    fn require_policy(&self) -> Result<(), Error> {
        if self.policy_applied {
            Ok(())
        } else {
            Err(Error::EncoderPolicyNotApplied)
        }
    }

    fn apply_current_policy(&mut self) -> Result<(), Error> {
        self.policy_applied = false;
        #[cfg(test)]
        let fail_at = self.injected_policy_failure_step.take();
        #[cfg(not(test))]
        let fail_at = None;
        let result = Self::apply_policy_with_hook(&mut self.inner, self.config.policy, |step| {
            if fail_at == Some(step) {
                Err(Error::Codec(CodecError::Internal))
            } else {
                Ok(())
            }
        });
        if result.is_ok() {
            self.policy_applied = true;
        }
        result
    }

    fn apply_policy_with_hook(
        inner: &mut relay_opus_sys::Encoder,
        policy: EncoderPolicyV1,
        mut after_step: impl FnMut(usize) -> Result<(), Error>,
    ) -> Result<(), Error> {
        inner
            .set_application(policy.application().as_raw())
            .map_err(codec_error)?;
        after_step(1)?;
        inner
            .set_bitrate(policy.bitrate().bps())
            .map_err(codec_error)?;
        after_step(2)?;
        inner
            .set_complexity(policy.complexity().get())
            .map_err(codec_error)?;
        after_step(3)?;
        inner.set_vbr(policy.vbr().enabled()).map_err(codec_error)?;
        after_step(4)?;
        inner
            .set_vbr_constraint(policy.vbr_constraint().constrained())
            .map_err(codec_error)?;
        after_step(5)?;
        inner
            .set_max_bandwidth(policy.max_bandwidth().as_raw())
            .map_err(codec_error)?;
        after_step(6)?;
        inner
            .set_bandwidth(policy.bandwidth().as_raw())
            .map_err(codec_error)?;
        after_step(7)?;
        inner
            .set_signal(policy.signal().as_raw())
            .map_err(codec_error)?;
        after_step(8)?;
        inner.set_dtx(policy.dtx().enabled()).map_err(codec_error)?;
        after_step(9)?;
        inner
            .set_inband_fec(policy.inband_fec().as_raw())
            .map_err(codec_error)?;
        after_step(10)?;
        inner
            .set_packet_loss_percent(policy.expected_packet_loss_percent().get())
            .map_err(codec_error)?;
        after_step(11)
    }

    #[cfg(test)]
    fn inject_next_policy_failure(&mut self, step: usize) {
        assert!((1..=11).contains(&step));
        self.injected_policy_failure_step = Some(step);
    }
}

/// A single-stream Opus decoder.
///
/// Construct this off the real-time thread. Decode methods use caller-owned
/// output and allocate nothing in Rust. Packets must be supplied in stream order.
pub struct Decoder {
    inner: relay_opus_sys::Decoder,
    config: DecoderConfig,
}

impl Decoder {
    /// Allocates and initializes libopus state.
    ///
    /// # Errors
    /// Returns a codec error if state creation fails.
    pub fn new(config: DecoderConfig) -> Result<Self, Error> {
        let inner = relay_opus_sys::Decoder::new(SAMPLE_RATE_HZ as i32, CHANNELS.into())
            .map_err(codec_error)?;
        Ok(Self { inner, config })
    }

    #[must_use]
    pub const fn config(&self) -> DecoderConfig {
        self.config
    }

    /// Decodes the current packet normally.
    pub fn decode(&mut self, packet: &[u8], output: &mut [f32]) -> Result<DecodedSamples, Error> {
        self.decode_packet(packet, output, false)
    }

    /// Recovers the previous lost frame from this packet's in-band FEC data.
    ///
    /// After a packet is declared lost, call this with the *following* packet
    /// to produce the lost frame, then call [`Self::decode`] with that same
    /// following packet to produce the current frame. This one-packet-late,
    /// two-step order is required because FEC decode does not decode the
    /// following packet itself. Libopus performs PLC if it has no usable FEC.
    pub fn decode_fec(
        &mut self,
        following_packet: &[u8],
        output: &mut [f32],
    ) -> Result<DecodedSamples, Error> {
        self.decode_packet(following_packet, output, true)
    }

    /// Synthesizes one configured-duration frame for a missing packet (PLC).
    pub fn decode_plc(&mut self, output: &mut [f32]) -> Result<DecodedSamples, Error> {
        self.validate_output(output)?;
        let samples = self
            .inner
            .decode_float(
                None,
                output,
                self.config.frame_duration.samples_per_channel() as i32,
                false,
            )
            .map_err(codec_error)?;
        self.finish_decode(samples)
    }

    fn decode_packet(
        &mut self,
        packet: &[u8],
        output: &mut [f32],
        decode_fec: bool,
    ) -> Result<DecodedSamples, Error> {
        if packet.is_empty() {
            return Err(Error::EmptyPacket);
        }
        if packet.len() > MAX_PACKET_BYTES {
            return Err(Error::PacketTooLarge {
                maximum: MAX_PACKET_BYTES,
                actual: packet.len(),
            });
        }
        self.validate_output(output)?;
        let expected = self.config.frame_duration.samples_per_channel();
        if !decode_fec {
            let actual = relay_opus_sys::packet_samples_per_channel(packet, SAMPLE_RATE_HZ as i32)
                .map_err(codec_error)?;
            if actual != expected {
                return Err(Error::UnexpectedDecodedDuration { expected, actual });
            }
        }
        let samples = self
            .inner
            .decode_float(Some(packet), output, expected as i32, decode_fec)
            .map_err(codec_error)?;
        self.finish_decode(samples)
    }

    fn validate_output(&self, output: &[f32]) -> Result<(), Error> {
        let required = self.config.frame_duration.interleaved_samples();
        if output.len() < required {
            return Err(Error::OutputTooSmall {
                required,
                actual: output.len(),
            });
        }
        Ok(())
    }

    fn finish_decode(&self, samples_per_channel: usize) -> Result<DecodedSamples, Error> {
        let expected = self.config.frame_duration.samples_per_channel();
        if samples_per_channel != expected {
            return Err(Error::UnexpectedDecodedDuration {
                expected,
                actual: samples_per_channel,
            });
        }
        let Some(interleaved_samples) = samples_per_channel.checked_mul(CHANNELS as usize) else {
            return Err(Error::InvalidCodecResult);
        };
        Ok(DecodedSamples {
            samples_per_channel,
            interleaved_samples,
        })
    }
}

/// Returns the linked libopus version string without allocating.
///
/// # Errors
/// Returns an error only if the library reports non-UTF-8 bytes.
pub fn libopus_version() -> Result<&'static str, Error> {
    relay_opus_sys::version_string()
        .to_str()
        .map_err(|_| Error::InvalidVersionString)
}

fn ensure_supported_libopus() -> Result<(), Error> {
    let version = libopus_version()?;
    if libopus_meets_v1_floor(version) {
        Ok(())
    } else {
        Err(Error::UnsupportedLibopusVersion)
    }
}

fn libopus_meets_v1_floor(version: &str) -> bool {
    let Some(numeric) = version.strip_prefix("libopus ") else {
        return false;
    };
    let Some((major, remainder)) = numeric.split_once('.') else {
        return false;
    };
    let minor = remainder.split('.').next().unwrap_or(remainder);
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return false;
    };
    major > 1 || (major == 1 && minor >= 6)
}

fn codec_error(code: i32) -> Error {
    Error::Codec(code.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn test_policy(fec: InbandFec, loss_percent: i32) -> EncoderPolicyV1 {
        EncoderPolicyV1::new(
            Bitrate::try_new(192_000).expect("test bitrate is valid"),
            fec,
            PacketLossPercent::try_new(loss_percent).expect("test loss hint is valid"),
        )
    }

    fn codec_pair(duration: FrameDuration) -> (Encoder, Decoder) {
        let config = EncoderConfigV1::stereo_48k(duration, test_policy(InbandFec::Disabled, 0));
        let encoder = Encoder::new(config).expect("system libopus should create an encoder");
        let decoder = Decoder::new(DecoderConfig::stereo_48k(duration))
            .expect("system libopus should create a decoder");
        (encoder, decoder)
    }

    #[test]
    fn silence_round_trip_uses_caller_owned_buffers() {
        let (mut encoder, mut decoder) = codec_pair(FrameDuration::Ms20);
        let input = [0.0_f32; 1_920];
        let mut packet = [0_u8; MAX_PACKET_BYTES];
        let mut output = [f32::NAN; 1_920];

        let packet_len = encoder.encode(&input, &mut packet).expect("encode silence");
        let decoded = decoder
            .decode(&packet[..packet_len], &mut output)
            .expect("decode silence");

        assert_eq!(decoded.samples_per_channel(), 960);
        assert!(
            output[..decoded.interleaved_samples()]
                .iter()
                .all(|sample| sample.is_finite())
        );
    }

    #[test]
    fn impulse_round_trip_is_finite_and_nonzero() {
        let (mut encoder, mut decoder) = codec_pair(FrameDuration::Ms10);
        let mut input = [0.0_f32; 960];
        input[0] = 1.0;
        input[1] = 1.0;
        let mut packet = [0_u8; MAX_PACKET_BYTES];
        let mut output = [0.0_f32; 960];

        let packet_len = encoder.encode(&input, &mut packet).expect("encode impulse");
        let decoded = decoder
            .decode(&packet[..packet_len], &mut output)
            .expect("decode impulse");
        let decoded = &output[..decoded.interleaved_samples()];

        assert!(decoded.iter().all(|sample| sample.is_finite()));
        assert!(decoded.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn configs_reject_non_relay_formats_and_frame_durations() {
        assert_eq!(
            EncoderConfigV1::try_new(
                44_100,
                2,
                FrameDuration::Ms20,
                test_policy(InbandFec::Disabled, 0),
            ),
            Err(Error::UnsupportedSampleRate(44_100))
        );
        assert_eq!(
            DecoderConfig::try_new(48_000, 1, FrameDuration::Ms20),
            Err(Error::UnsupportedChannelCount(1))
        );
        assert_eq!(
            FrameDuration::try_from(40),
            Err(Error::UnsupportedFrameDuration(40))
        );
    }

    #[test]
    fn encode_and_decode_reject_too_small_buffers() {
        let (mut encoder, mut decoder) = codec_pair(FrameDuration::Ms5);
        let input = [0.0_f32; 480];
        let mut no_packet_space = [];
        let mut short_pcm = [0.0_f32; 479];

        assert_eq!(
            encoder.encode(&input, &mut no_packet_space),
            Err(Error::OutputTooSmall {
                required: 1,
                actual: 0,
            })
        );
        assert_eq!(
            decoder.decode_plc(&mut short_pcm),
            Err(Error::OutputTooSmall {
                required: 480,
                actual: 479,
            })
        );
    }

    #[test]
    fn plc_produces_one_bounded_frame() {
        let (_, mut decoder) = codec_pair(FrameDuration::Ms10);
        let mut output = [f32::NAN; 960];

        let decoded = decoder.decode_plc(&mut output).expect("decode PLC");

        assert_eq!(decoded.interleaved_samples(), 960);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    fn voiced_frame(duration: FrameDuration, frequency_hz: f32, amplitude: f32) -> Vec<f32> {
        let mut frame = vec![0.0; duration.interleaved_samples()];
        for (index, stereo) in frame.chunks_exact_mut(2).enumerate() {
            let phase =
                core::f32::consts::TAU * frequency_hz * index as f32 / SAMPLE_RATE_HZ as f32;
            let sample = amplitude * phase.sin();
            stereo[0] = sample;
            stereo[1] = sample * 0.8;
        }
        frame
    }

    fn signal_energy(samples: &[f32]) -> f32 {
        samples.iter().map(|sample| sample * sample).sum::<f32>()
    }

    #[test]
    fn cross_duration_packets_are_rejected_before_output_is_touched() {
        for encoded_duration in [FrameDuration::Ms5, FrameDuration::Ms10] {
            let mut encoder = Encoder::new(EncoderConfigV1::stereo_48k(
                encoded_duration,
                test_policy(InbandFec::Disabled, 0),
            ))
            .expect("create encoder");
            let mut decoder = Decoder::new(DecoderConfig::stereo_48k(FrameDuration::Ms20))
                .expect("create decoder");
            let input = vec![0.0; encoded_duration.interleaved_samples()];
            let mut packet = [0_u8; MAX_PACKET_BYTES];
            let packet_len = encoder.encode(&input, &mut packet).expect("encode");
            let mut output = [f32::NAN; 1_920];

            assert_eq!(
                decoder.decode(&packet[..packet_len], &mut output),
                Err(Error::UnexpectedDecodedDuration {
                    expected: 960,
                    actual: encoded_duration.samples_per_channel(),
                })
            );
            assert!(output.iter().all(|sample| sample.is_nan()));

            let mut valid_encoder = Encoder::new(EncoderConfigV1::stereo_48k(
                FrameDuration::Ms20,
                test_policy(InbandFec::Disabled, 0),
            ))
            .expect("create valid encoder");
            let valid_input = voiced_frame(FrameDuration::Ms20, 240.0, 0.25);
            let valid_len = valid_encoder
                .encode(&valid_input, &mut packet)
                .expect("encode valid frame");
            let mut fresh_decoder =
                Decoder::new(DecoderConfig::stereo_48k(FrameDuration::Ms20)).expect("decoder");
            let mut after_rejection = [0.0; 1_920];
            let mut from_fresh_state = [0.0; 1_920];
            decoder
                .decode(&packet[..valid_len], &mut after_rejection)
                .expect("decode after rejected mismatch");
            fresh_decoder
                .decode(&packet[..valid_len], &mut from_fresh_state)
                .expect("decode from fresh state");
            assert_eq!(after_rejection, from_fresh_state);
        }
    }

    #[test]
    fn fec_recovers_previous_voice_then_same_packet_decodes_current_voice() {
        let duration = FrameDuration::Ms20;
        let config = EncoderConfigV1::stereo_48k(duration, test_policy(InbandFec::Enabled, 35));
        let mut encoder = Encoder::new(config).expect("create FEC encoder");
        let mut decoder =
            Decoder::new(DecoderConfig::stereo_48k(duration)).expect("create decoder");
        let first = voiced_frame(duration, 180.0, 0.7);
        let second = voiced_frame(duration, 310.0, 0.35);
        let mut first_packet = [0_u8; MAX_PACKET_BYTES];
        let mut second_packet = [0_u8; MAX_PACKET_BYTES];
        let _first_len = encoder
            .encode(&first, &mut first_packet)
            .expect("encode first");
        let second_len = encoder
            .encode(&second, &mut second_packet)
            .expect("encode second");
        let second_packet = &second_packet[..second_len];
        let mut recovered = [f32::NAN; 1_920];
        let mut current = [f32::NAN; 1_920];

        let recovered_len = decoder
            .decode_fec(second_packet, &mut recovered)
            .expect("recover first from second packet");
        let current_len = decoder
            .decode(second_packet, &mut current)
            .expect("decode the same second packet normally");

        assert_eq!(recovered_len.samples_per_channel(), 960);
        assert_eq!(current_len.samples_per_channel(), 960);
        assert!(
            recovered
                .iter()
                .chain(&current)
                .all(|sample| sample.is_finite())
        );
        let recovered_energy = signal_energy(&recovered);
        let current_energy = signal_energy(&current);
        assert!(recovered_energy > 0.01, "FEC must recover non-silent audio");
        assert!(
            current_energy > 0.01,
            "normal decode must produce current audio"
        );
        let difference = recovered
            .iter()
            .zip(current)
            .map(|(left, right)| (left - right) * (left - right))
            .sum::<f32>();
        assert!(
            difference > 0.01,
            "lost and current frames must be distinct"
        );
    }

    #[test]
    fn decode_fec_without_fec_data_falls_back_to_plc() {
        let duration = FrameDuration::Ms20;
        let mut encoder = Encoder::new(EncoderConfigV1::stereo_48k(
            duration,
            test_policy(InbandFec::Disabled, 0),
        ))
        .expect("create encoder without FEC");
        let input = voiced_frame(duration, 220.0, 0.5);
        let mut packet = [0_u8; MAX_PACKET_BYTES];
        let packet_len = encoder.encode(&input, &mut packet).expect("encode");
        let mut fec_decoder = Decoder::new(DecoderConfig::stereo_48k(duration)).expect("decoder");
        let mut plc_decoder = Decoder::new(DecoderConfig::stereo_48k(duration)).expect("decoder");
        let mut fec_output = [f32::NAN; 1_920];
        let mut plc_output = [f32::NAN; 1_920];

        fec_decoder
            .decode_fec(&packet[..packet_len], &mut fec_output)
            .expect("FEC path falls back");
        plc_decoder
            .decode_plc(&mut plc_output)
            .expect("explicit PLC");

        assert!(
            fec_output
                .iter()
                .chain(&plc_output)
                .all(|sample| sample.is_finite())
        );
        assert_eq!(fec_output, plc_output);
    }

    #[test]
    fn v1_policy_config_getters_expose_every_explicit_decision() {
        let policy = test_policy(InbandFec::EnabledWithoutSilkSwitch, 17);
        let config = EncoderConfigV1::stereo_48k(FrameDuration::Ms10, policy);

        assert_eq!(config.frame_duration(), FrameDuration::Ms10);
        assert_eq!(config.policy(), policy);
        assert_eq!(policy.application(), Application::Audio);
        assert_eq!(policy.bitrate().bps(), 192_000);
        assert_eq!(policy.complexity(), Complexity::MAXIMUM);
        assert_eq!(policy.vbr(), VbrMode::Enabled);
        assert_eq!(policy.vbr_constraint(), VbrConstraint::Constrained);
        assert_eq!(policy.bandwidth(), Bandwidth::Auto);
        assert_eq!(policy.max_bandwidth(), Bandwidth::Fullband);
        assert_eq!(policy.signal(), Signal::Music);
        assert_eq!(policy.dtx(), DtxMode::Disabled);
        assert_eq!(policy.inband_fec(), InbandFec::EnabledWithoutSilkSwitch);
        assert_eq!(policy.expected_packet_loss_percent().get(), 17);
    }

    #[test]
    fn checked_value_types_reject_values_outside_libopus_ranges() {
        assert_eq!(
            Bitrate::try_new(Bitrate::MIN_BPS).map(Bitrate::bps),
            Ok(500)
        );
        assert_eq!(
            Bitrate::try_new(Bitrate::MAX_BPS).map(Bitrate::bps),
            Ok(512_000)
        );
        assert_eq!(
            Complexity::try_new(Complexity::MIN).map(Complexity::get),
            Ok(0)
        );
        assert_eq!(
            Complexity::try_new(Complexity::MAX).map(Complexity::get),
            Ok(10)
        );
        assert_eq!(
            PacketLossPercent::try_new(PacketLossPercent::MIN).map(PacketLossPercent::get),
            Ok(0)
        );
        assert_eq!(
            PacketLossPercent::try_new(PacketLossPercent::MAX).map(PacketLossPercent::get),
            Ok(100)
        );

        assert_eq!(Bitrate::try_new(-1), Err(Error::InvalidBitrate(-1)));
        assert_eq!(Bitrate::try_new(499), Err(Error::InvalidBitrate(499)));
        assert_eq!(
            Bitrate::try_new(512_001),
            Err(Error::InvalidBitrate(512_001))
        );
        assert_eq!(Complexity::try_new(-1), Err(Error::InvalidComplexity(-1)));
        assert_eq!(Complexity::try_new(11), Err(Error::InvalidComplexity(11)));
        assert_eq!(
            PacketLossPercent::try_new(-1),
            Err(Error::InvalidPacketLossPercent(-1))
        );
        assert_eq!(
            PacketLossPercent::try_new(101),
            Err(Error::InvalidPacketLossPercent(101))
        );
    }

    #[test]
    fn encoder_controls_round_trip_and_reset_reapplies_the_complete_policy() {
        let initial = EncoderConfigV1::stereo_48k(
            FrameDuration::Ms20,
            test_policy(InbandFec::EnabledWithoutSilkSwitch, 17),
        );
        let mut encoder = Encoder::new(initial).expect("create encoder");

        assert_eq!(encoder.application(), Ok(Application::Audio));
        assert_eq!(encoder.bitrate(), Ok(initial.policy().bitrate()));
        assert_eq!(encoder.complexity(), Ok(Complexity::MAXIMUM));
        assert_eq!(encoder.vbr(), Ok(VbrMode::Enabled));
        assert_eq!(encoder.vbr_constraint(), Ok(VbrConstraint::Constrained));
        assert_eq!(encoder.bandwidth(), Ok(Bandwidth::Fullband));
        assert_eq!(encoder.max_bandwidth(), Ok(Bandwidth::Fullband));
        assert_eq!(encoder.signal(), Ok(Signal::Music));
        assert_eq!(encoder.dtx(), Ok(DtxMode::Disabled));
        assert_eq!(
            encoder.inband_fec(),
            Ok(InbandFec::EnabledWithoutSilkSwitch)
        );
        assert_eq!(
            encoder.expected_packet_loss_percent(),
            Ok(PacketLossPercent::try_new(17).expect("valid loss hint"))
        );

        let updated_bitrate = Bitrate::try_new(128_000).expect("valid bitrate");
        let updated_loss = PacketLossPercent::try_new(31).expect("valid loss hint");
        encoder
            .set_bitrate(updated_bitrate)
            .expect("update bitrate");
        encoder
            .set_inband_fec(InbandFec::Enabled)
            .expect("update FEC");
        encoder
            .set_expected_packet_loss_percent(updated_loss)
            .expect("update loss hint");
        encoder.reset().expect("reset and reapply policy");

        assert_eq!(encoder.application(), Ok(Application::Audio));
        assert_eq!(encoder.bitrate(), Ok(updated_bitrate));
        assert_eq!(encoder.complexity(), Ok(Complexity::MAXIMUM));
        assert_eq!(encoder.vbr(), Ok(VbrMode::Enabled));
        assert_eq!(encoder.vbr_constraint(), Ok(VbrConstraint::Constrained));
        assert_eq!(encoder.bandwidth(), Ok(Bandwidth::Fullband));
        assert_eq!(encoder.max_bandwidth(), Ok(Bandwidth::Fullband));
        assert_eq!(encoder.signal(), Ok(Signal::Music));
        assert_eq!(encoder.dtx(), Ok(DtxMode::Disabled));
        assert_eq!(encoder.inband_fec(), Ok(InbandFec::Enabled));
        assert_eq!(encoder.expected_packet_loss_percent(), Ok(updated_loss));
        assert_eq!(encoder.config().policy().bitrate(), updated_bitrate);
    }

    #[test]
    fn failed_policy_reapplication_poison_rejects_use_until_complete_reset() {
        let config = EncoderConfigV1::stereo_48k(
            FrameDuration::Ms20,
            test_policy(InbandFec::EnabledWithoutSilkSwitch, 17),
        );
        let input = [0.0_f32; 1_920];
        let mut packet = [0_u8; MAX_PACKET_BYTES];
        let updated_bitrate = Bitrate::try_new(128_000).expect("valid bitrate");
        let updated_loss = PacketLossPercent::try_new(31).expect("valid loss hint");

        for step in 1..=11 {
            let mut encoder = Encoder::new(config).expect("create encoder");
            encoder.inject_next_policy_failure(step);
            assert_eq!(
                encoder.reset(),
                Err(Error::Codec(CodecError::Internal)),
                "injected step {step}"
            );

            assert_eq!(encoder.application(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.bitrate(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.complexity(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.vbr(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(
                encoder.vbr_constraint(),
                Err(Error::EncoderPolicyNotApplied)
            );
            assert_eq!(encoder.bandwidth(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.max_bandwidth(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.signal(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.dtx(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(encoder.inband_fec(), Err(Error::EncoderPolicyNotApplied));
            assert_eq!(
                encoder.expected_packet_loss_percent(),
                Err(Error::EncoderPolicyNotApplied)
            );
            assert_eq!(
                encoder.set_bitrate(updated_bitrate),
                Err(Error::EncoderPolicyNotApplied)
            );
            assert_eq!(
                encoder.set_inband_fec(InbandFec::Enabled),
                Err(Error::EncoderPolicyNotApplied)
            );
            assert_eq!(
                encoder.set_expected_packet_loss_percent(updated_loss),
                Err(Error::EncoderPolicyNotApplied)
            );
            assert_eq!(
                encoder.encode(&input, &mut packet),
                Err(Error::EncoderPolicyNotApplied)
            );

            encoder
                .reset()
                .expect("a complete reset recovers the encoder");
            assert_eq!(encoder.application(), Ok(Application::Audio));
            assert_eq!(encoder.vbr_constraint(), Ok(VbrConstraint::Constrained));
            assert_eq!(encoder.bandwidth(), Ok(Bandwidth::Fullband));
            assert_eq!(encoder.max_bandwidth(), Ok(Bandwidth::Fullband));
            assert!(encoder.encode(&input, &mut packet).is_ok());
        }
    }

    #[test]
    fn mode_2_fec_runs_a_practical_encode_loss_and_recovery_sequence() {
        let duration = FrameDuration::Ms20;
        let mut encoder = Encoder::new(EncoderConfigV1::stereo_48k(
            duration,
            test_policy(InbandFec::EnabledWithoutSilkSwitch, 35),
        ))
        .expect("create mode-2 FEC encoder");
        let mut decoder =
            Decoder::new(DecoderConfig::stereo_48k(duration)).expect("create decoder");
        let first = voiced_frame(duration, 170.0, 0.6);
        let lost = voiced_frame(duration, 260.0, 0.5);
        let following = voiced_frame(duration, 350.0, 0.4);
        let mut packet = [0_u8; MAX_PACKET_BYTES];
        let mut first_packet = [0_u8; MAX_PACKET_BYTES];
        let first_len = encoder
            .encode(&first, &mut first_packet)
            .expect("encode first");
        let _lost_len = encoder
            .encode(&lost, &mut packet)
            .expect("encode dropped packet");
        let following_len = encoder
            .encode(&following, &mut packet)
            .expect("encode following packet");
        let mut output = [f32::NAN; 1_920];

        decoder
            .decode(&first_packet[..first_len], &mut output)
            .expect("prime decoder with first packet");
        let recovered = decoder
            .decode_fec(&packet[..following_len], &mut output)
            .expect("recover dropped mode-2 frame");
        assert_eq!(
            recovered.samples_per_channel(),
            duration.samples_per_channel()
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(signal_energy(&output) > 0.01);
        let current = decoder
            .decode(&packet[..following_len], &mut output)
            .expect("decode following packet after FEC attempt");
        assert_eq!(
            current.samples_per_channel(),
            duration.samples_per_channel()
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(
            encoder.inband_fec(),
            Ok(InbandFec::EnabledWithoutSilkSwitch)
        );
    }

    #[test]
    fn all_fec_modes_are_compatible_with_v1_policy_and_loss_hint() {
        for mode in [
            InbandFec::Disabled,
            InbandFec::Enabled,
            InbandFec::EnabledWithoutSilkSwitch,
        ] {
            let config = EncoderConfigV1::stereo_48k(FrameDuration::Ms20, test_policy(mode, 20));
            let mut encoder = Encoder::new(config).expect("create encoder");

            assert_eq!(encoder.inband_fec(), Ok(mode));
            assert_eq!(
                encoder.expected_packet_loss_percent(),
                Ok(PacketLossPercent::try_new(20).expect("valid loss hint"))
            );
        }
    }

    #[test]
    fn v1_controls_and_packet_duration_hold_across_all_negotiated_durations() {
        for duration in [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20] {
            let config = EncoderConfigV1::stereo_48k(duration, test_policy(InbandFec::Disabled, 0));
            let mut encoder = Encoder::new(config).expect("create encoder");
            let input = vec![0.0_f32; duration.interleaved_samples()];
            let mut packet = [0_u8; MAX_PACKET_BYTES];
            let packet_len = encoder.encode(&input, &mut packet).expect("encode frame");

            assert_eq!(
                relay_opus_sys::packet_samples_per_channel(
                    &packet[..packet_len],
                    SAMPLE_RATE_HZ as i32,
                ),
                Ok(duration.samples_per_channel())
            );
            assert_eq!(encoder.application(), Ok(Application::Audio));
            assert_eq!(encoder.complexity(), Ok(Complexity::MAXIMUM));
            assert_eq!(encoder.vbr(), Ok(VbrMode::Enabled));
            assert_eq!(encoder.vbr_constraint(), Ok(VbrConstraint::Constrained));
            assert_eq!(encoder.bandwidth(), Ok(Bandwidth::Fullband));
            assert_eq!(encoder.max_bandwidth(), Ok(Bandwidth::Fullband));
            assert_eq!(encoder.signal(), Ok(Signal::Music));
            assert_eq!(encoder.dtx(), Ok(DtxMode::Disabled));
        }
    }

    #[cfg(not(debug_assertions))]
    #[test]
    #[ignore = "release-only steady-state throughput gate; run explicitly with --release --ignored"]
    fn release_steady_state_codec_gate() {
        let (mut encoder, mut decoder) = codec_pair(FrameDuration::Ms20);
        let input = voiced_frame(FrameDuration::Ms20, 220.0, 0.25);
        let mut packet = [0_u8; MAX_PACKET_BYTES];
        let mut output = [0.0_f32; 1_920];
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            let packet_len = encoder.encode(&input, &mut packet).expect("encode");
            decoder
                .decode(&packet[..packet_len], &mut output)
                .expect("decode");
        }
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "10k encode/decode iterations exceeded the release budget"
        );
    }

    #[test]
    fn malformed_inputs_return_errors_without_panicking() {
        let (mut encoder, mut decoder) = codec_pair(FrameDuration::Ms5);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut packet = [0_u8; 8];
            let mut output = [0.0_f32; 480];
            let encode_result = encoder.encode(&[], &mut packet);
            let empty_result = decoder.decode(&[], &mut output);
            let invalid_packet_result = decoder.decode(&[0xff], &mut output);
            (encode_result, empty_result, invalid_packet_result)
        }));

        let (encode_result, empty_result, invalid_packet_result) =
            outcome.expect("safe API must not panic on caller input");
        assert!(matches!(encode_result, Err(Error::InvalidPcmLength { .. })));
        assert_eq!(empty_result, Err(Error::EmptyPacket));
        assert!(invalid_packet_result.is_err());
    }

    #[test]
    fn linked_libopus_meets_the_v1_runtime_floor() {
        let version = libopus_version().expect("linked version is valid UTF-8");
        assert!(libopus_meets_v1_floor(version), "unsupported {version}");
        assert!(libopus_meets_v1_floor("libopus 1.6.0"));
        assert!(libopus_meets_v1_floor("libopus 1.7.0"));
        assert!(libopus_meets_v1_floor("libopus 2.0.0"));
        assert!(!libopus_meets_v1_floor("libopus 1.5.2"));
        assert!(!libopus_meets_v1_floor("unknown"));
    }

    #[test]
    #[ignore = "CI/artifact smoke: the locked distribution environment must link exact libopus 1.6.1"]
    fn linked_libopus_1_6_1_artifact_smoke() {
        assert_eq!(libopus_version(), Ok("libopus 1.6.1"));
    }

    #[test]
    fn owning_codec_wrappers_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Encoder>();
        assert_send::<Decoder>();
    }
}
