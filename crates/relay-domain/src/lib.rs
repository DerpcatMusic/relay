#![forbid(unsafe_code)]

/// Product surface used to create or join a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionMode {
    Connect,
    Link,
    Stream,
}

/// Path currently carrying media.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaRoute {
    Direct,
    TurnRelay,
    Sfu,
}

/// Whether a session may fall back to a paid media route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaidFallbackPolicy {
    Never,
    Ask,
    Auto,
}

/// High-level lifecycle state of a session connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Idle,
    Creating,
    Signaling,
    Connecting,
    Connected,
    Recovering,
    Closing,
    Closed,
    Failed,
}

/// Supported encoded audio frame durations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDuration {
    Ms5,
    Ms10,
    Ms20,
}

impl FrameDuration {
    #[must_use]
    pub const fn microseconds(self) -> u32 {
        match self {
            Self::Ms5 => 5_000,
            Self::Ms10 => 10_000,
            Self::Ms20 => 20_000,
        }
    }
}

/// In-band forward-error-correction preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FecPolicy {
    Disabled,
    Enabled,
    Adaptive,
}

/// Concrete audio encoding settings selected for a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioProfile {
    sample_rate_hz: u32,
    channels: u8,
    bitrate_bps: u32,
    frame_duration: FrameDuration,
    fec: FecPolicy,
    dtx: bool,
}

impl AudioProfile {
    pub const NETWORK_SAMPLE_RATE_HZ: u32 = 48_000;
    /// Builds a profile after checking the V1 media constraints.
    pub fn new(
        sample_rate_hz: u32,
        channels: u8,
        bitrate_bps: u32,
        frame_duration: FrameDuration,
        fec: FecPolicy,
        dtx: bool,
    ) -> Result<Self, AudioProfileError> {
        let profile = Self {
            sample_rate_hz,
            channels,
            bitrate_bps,
            frame_duration,
            fec,
            dtx,
        };
        profile.validate()?;
        Ok(profile)
    }

    #[must_use]
    pub const fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    #[must_use]
    pub const fn channels(&self) -> u8 {
        self.channels
    }

    #[must_use]
    pub const fn bitrate_bps(&self) -> u32 {
        self.bitrate_bps
    }

    #[must_use]
    pub const fn frame_duration(&self) -> FrameDuration {
        self.frame_duration
    }

    #[must_use]
    pub const fn fec(&self) -> FecPolicy {
        self.fec
    }

    #[must_use]
    pub const fn dtx(&self) -> bool {
        self.dtx
    }

    /// Validates a profile at an external-data boundary.
    pub fn validate(&self) -> Result<(), AudioProfileError> {
        if self.sample_rate_hz != Self::NETWORK_SAMPLE_RATE_HZ {
            return Err(AudioProfileError::UnsupportedSampleRate(
                self.sample_rate_hz,
            ));
        }
        if self.channels != 2 {
            return Err(AudioProfileError::UnsupportedChannelCount(self.channels));
        }
        if self.bitrate_bps == 0 {
            return Err(AudioProfileError::ZeroBitrate);
        }
        if self.dtx {
            return Err(AudioProfileError::DtxUnsupported);
        }
        Ok(())
    }
}

/// Why an [`AudioProfile`] is not valid for V1 media.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioProfileError {
    UnsupportedSampleRate(u32),
    UnsupportedChannelCount(u8),
    ZeroBitrate,
    DtxUnsupported,
}

impl core::fmt::Display for AudioProfileError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedSampleRate(rate) => {
                write!(formatter, "unsupported sample rate: {rate} Hz")
            }
            Self::UnsupportedChannelCount(channels) => {
                write!(formatter, "unsupported channel count: {channels}")
            }
            Self::ZeroBitrate => formatter.write_str("bitrate must be greater than zero"),
            Self::DtxUnsupported => formatter.write_str("DTX is not supported by the V1 profile"),
        }
    }
}

impl std::error::Error for AudioProfileError {}

/// Product-level quality selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityProfile {
    UltraLowLatency,
    Balanced,
    Stable,
    Custom(AudioProfile),
}
