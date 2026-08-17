//! Wire codecs and their settings.
//!
//! The plugin and session worker share this vocabulary:
//! - [`WireCodec::Opus`] — default live path, 192 kbps stereo
//! - [`WireCodec::Flac`] — 16-bit lossless frames
//! - [`WireCodec::Pcm`] — uncompressed 16-bit integer

/// Media clock on the RELAY wire.
pub const WIRE_RATE_HZ: u32 = 48_000;
/// Stereo on the RELAY wire.
pub const WIRE_CHANNELS: u8 = 2;
/// Integer depth for FLAC and PCM (and for decoded web PCM).
pub const WIRE_BITS: u8 = 16;

/// Default live Opus bitrate.
pub const OPUS_BITRATE_DEFAULT_KBPS: u32 = 192;
/// Lowest selectable Opus bitrate.
pub const OPUS_BITRATE_MIN_KBPS: u32 = 64;
/// Highest selectable Opus bitrate.
pub const OPUS_BITRATE_MAX_KBPS: u32 = 256;
/// Default FLAC compression effort (libFLAC-style 0–8).
pub const FLAC_LEVEL_DEFAULT: u8 = 5;
/// Highest FLAC compression effort.
pub const FLAC_LEVEL_MAX: u8 = 8;

/// How media is packed on the LAN/plugin wire.
///
/// Web listen always receives decoded 48 kHz PCM; the browser resamples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WireCodec {
    /// Opus, default live path (typically 192 kbps).
    Opus = 0,
    /// 16-bit lossless FLAC frames.
    Flac = 1,
    /// Uncompressed 16-bit PCM.
    Pcm = 2,
}

impl WireCodec {
    /// Parses a stored codec byte.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Opus),
            1 => Some(Self::Flac),
            2 => Some(Self::Pcm),
            _ => None,
        }
    }

    /// Stable name for claims and the listen page.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Flac => "flac",
            Self::Pcm => "pcm",
        }
    }

    /// True when this codec sends uncompressed PCM.
    #[must_use]
    pub const fn is_pcm(self) -> bool {
        matches!(self, Self::Pcm)
    }

    /// Default settings for this codec.
    #[must_use]
    pub const fn default_settings(self) -> CodecSettings {
        match self {
            Self::Opus => CodecSettings::Opus(OpusSettings::live()),
            Self::Flac => CodecSettings::Flac(FlacSettings::standard()),
            Self::Pcm => CodecSettings::Pcm(PcmSettings::standard()),
        }
    }
}

/// Opus encoder settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpusSettings {
    bitrate_kbps: u32,
}

impl OpusSettings {
    /// Live default: 192 kbps stereo.
    #[must_use]
    pub const fn live() -> Self {
        Self {
            bitrate_kbps: OPUS_BITRATE_DEFAULT_KBPS,
        }
    }

    /// Clamps a user bitrate into the supported range.
    #[must_use]
    pub const fn new(bitrate_kbps: u32) -> Self {
        Self {
            bitrate_kbps: clamp_opus_kbps(bitrate_kbps),
        }
    }

    /// Target bitrate in kilobits per second.
    #[must_use]
    pub const fn bitrate_kbps(self) -> u32 {
        self.bitrate_kbps
    }

    /// Target bitrate in bits per second for libopus.
    #[must_use]
    pub const fn bitrate_bps(self) -> i32 {
        (self.bitrate_kbps as i32).saturating_mul(1_000)
    }
}

/// 16-bit FLAC encoder settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlacSettings {
    bits: u8,
    compression: u8,
}

impl FlacSettings {
    /// Standard 16-bit stereo FLAC at compression 5.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            bits: WIRE_BITS,
            compression: FLAC_LEVEL_DEFAULT,
        }
    }

    /// 16-bit FLAC at a libFLAC-style compression level (0 fast … 8 smallest).
    #[must_use]
    pub const fn new(compression: u8) -> Self {
        Self {
            bits: WIRE_BITS,
            compression: if compression > FLAC_LEVEL_MAX {
                FLAC_LEVEL_MAX
            } else {
                compression
            },
        }
    }

    /// Always 16. FLAC on this wire never sends 24-bit.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.bits
    }

    /// Compression effort, 0 through 8.
    #[must_use]
    pub const fn compression(self) -> u8 {
        self.compression
    }
}

/// Uncompressed PCM settings. Always 16-bit integer on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmSettings {
    bits: u8,
}

impl PcmSettings {
    /// 16-bit interleaved stereo PCM at 48 kHz.
    #[must_use]
    pub const fn standard() -> Self {
        Self { bits: WIRE_BITS }
    }

    /// Integer depth. Always 16.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.bits
    }
}

/// Selected codec plus the settings that belong to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecSettings {
    /// Lossy Opus.
    Opus(OpusSettings),
    /// 16-bit lossless FLAC.
    Flac(FlacSettings),
    /// Uncompressed 16-bit PCM.
    Pcm(PcmSettings),
}

impl CodecSettings {
    /// Default live path: Opus at 192 kbps.
    #[must_use]
    pub const fn live() -> Self {
        Self::Opus(OpusSettings::live())
    }

    /// Builds settings from the stored control cells.
    #[must_use]
    pub const fn from_parts(codec: WireCodec, bitrate_kbps: u32, flac_level: u8) -> Self {
        match codec {
            WireCodec::Opus => Self::Opus(OpusSettings::new(bitrate_kbps)),
            WireCodec::Flac => Self::Flac(FlacSettings::new(flac_level)),
            WireCodec::Pcm => Self::Pcm(PcmSettings::standard()),
        }
    }

    /// Codec family.
    #[must_use]
    pub const fn codec(self) -> WireCodec {
        match self {
            Self::Opus(_) => WireCodec::Opus,
            Self::Flac(_) => WireCodec::Flac,
            Self::Pcm(_) => WireCodec::Pcm,
        }
    }

    /// Integer depth on the wire. Always 16 for FLAC and PCM.
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Opus(_) => WIRE_BITS,
            Self::Flac(settings) => settings.bits(),
            Self::Pcm(settings) => settings.bits(),
        }
    }

    /// Opus bitrate, if this is Opus.
    #[must_use]
    pub const fn bitrate_kbps(self) -> Option<u32> {
        match self {
            Self::Opus(settings) => Some(settings.bitrate_kbps()),
            Self::Flac(_) | Self::Pcm(_) => None,
        }
    }

    /// FLAC compression, if this is FLAC.
    #[must_use]
    pub const fn flac_level(self) -> Option<u8> {
        match self {
            Self::Flac(settings) => Some(settings.compression()),
            Self::Opus(_) | Self::Pcm(_) => None,
        }
    }

    /// Opus bitrate in bps, or 192 kbps when this is not Opus.
    #[must_use]
    pub const fn opus_bitrate_bps(self) -> i32 {
        match self {
            Self::Opus(settings) => settings.bitrate_bps(),
            Self::Flac(_) | Self::Pcm(_) => OpusSettings::live().bitrate_bps(),
        }
    }
}

/// Quantizes a float sample to 16-bit PCM.
#[must_use]
pub fn quantize_s16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0) as i16
}

/// Writes interleaved f32 as little-endian 16-bit PCM.
pub fn write_s16le(pcm: &[f32], out: &mut [u8]) {
    let n = pcm.len().min(out.len() / 2);
    for (index, sample) in pcm.iter().take(n).enumerate() {
        let quant = quantize_s16(*sample);
        out[index * 2..index * 2 + 2].copy_from_slice(&quant.to_le_bytes());
    }
}

const fn clamp_opus_kbps(kbps: u32) -> u32 {
    if kbps < OPUS_BITRATE_MIN_KBPS {
        OPUS_BITRATE_MIN_KBPS
    } else if kbps > OPUS_BITRATE_MAX_KBPS {
        OPUS_BITRATE_MAX_KBPS
    } else {
        kbps
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodecSettings, FlacSettings, OPUS_BITRATE_DEFAULT_KBPS, OpusSettings, PcmSettings,
        WIRE_BITS, WireCodec,
    };

    #[test]
    fn live_default_is_opus_192() {
        let settings = CodecSettings::live();
        assert_eq!(settings.codec(), WireCodec::Opus);
        assert_eq!(settings.bitrate_kbps(), Some(OPUS_BITRATE_DEFAULT_KBPS));
        assert_eq!(OpusSettings::live().bitrate_bps(), 192_000);
    }

    #[test]
    fn flac_is_always_sixteen_bit() {
        let settings = FlacSettings::new(8);
        assert_eq!(settings.bits(), WIRE_BITS);
        assert_eq!(settings.compression(), 8);
        assert_eq!(FlacSettings::new(99).compression(), 8);
    }

    #[test]
    fn pcm_is_always_sixteen_bit() {
        assert_eq!(PcmSettings::standard().bits(), 16);
    }
}
