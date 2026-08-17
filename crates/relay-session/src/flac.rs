//! 16-bit stereo FLAC frames for the RELAY wire.

use std::io::Cursor;

use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use flacenc::source::MemSource;

use crate::codec::{FlacSettings, WIRE_BITS, WIRE_CHANNELS, WIRE_RATE_HZ};

/// Encodes interleaved 16-bit stereo PCM as a standalone FLAC stream.
pub fn encode_s16le(pcm: &[i16], compression: u8) -> Result<Vec<u8>, ()> {
    encode_with(pcm, FlacSettings::new(compression))
}

/// Encodes interleaved 16-bit stereo PCM using explicit FLAC settings.
pub fn encode_with(pcm: &[i16], settings: FlacSettings) -> Result<Vec<u8>, ()> {
    if pcm.len() < 4 || !pcm.len().is_multiple_of(2) {
        return Err(());
    }
    let samples: Vec<i32> = pcm.iter().map(|sample| i32::from(*sample)).collect();
    let channels = usize::from(WIRE_CHANNELS);
    let frames = pcm.len() / channels;
    let block = frames.clamp(32, 4096);
    let mut config = flacenc::config::Encoder::default();
    config.block_size = block;
    config.multithread = false;
    apply_settings(&mut config, settings);
    let verified = config.into_verified().map_err(|_| ())?;
    let source = MemSource::from_samples(
        &samples,
        channels,
        usize::from(WIRE_BITS),
        WIRE_RATE_HZ as usize,
    );
    let stream = flacenc::encode_with_fixed_block_size(&verified, source, block).map_err(|_| ())?;
    let mut sink = ByteSink::new();
    stream.write(&mut sink).map_err(|_| ())?;
    Ok(sink.as_slice().to_vec())
}

/// Decodes a 16-bit stereo FLAC blob back to interleaved i16.
pub fn decode_s16le(bytes: &[u8]) -> Result<Vec<i16>, ()> {
    let mut reader = claxon::FlacReader::new(Cursor::new(bytes)).map_err(|_| ())?;
    let info = reader.streaminfo();
    if info.channels != u32::from(WIRE_CHANNELS) || info.bits_per_sample != u32::from(WIRE_BITS) {
        return Err(());
    }
    let mut pcm = Vec::new();
    for sample in reader.samples() {
        let value = sample.map_err(|_| ())?;
        pcm.push(i16::try_from(value).unwrap_or(0));
    }
    if pcm.len() < 4 || !pcm.len().is_multiple_of(2) {
        return Err(());
    }
    Ok(pcm)
}

/// Maps a libFLAC-style 0–8 level onto flacenc knobs.
fn apply_settings(config: &mut flacenc::config::Encoder, settings: FlacSettings) {
    let level = settings.compression();
    let stereo = &mut config.stereo_coding;
    let sub = &mut config.subframe_coding;
    match level {
        0 => {
            stereo.use_leftside = false;
            stereo.use_rightside = false;
            stereo.use_midside = false;
            sub.use_lpc = false;
            sub.use_fixed = true;
            sub.fixed.max_order = 0;
        }
        1 => {
            stereo.use_leftside = false;
            stereo.use_rightside = false;
            stereo.use_midside = false;
            sub.use_lpc = false;
            sub.use_fixed = true;
            sub.fixed.max_order = 2;
        }
        2 => {
            stereo.use_midside = false;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 4;
        }
        3 => {
            stereo.use_midside = true;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 6;
        }
        4 => {
            stereo.use_midside = true;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 8;
        }
        5 => {
            stereo.use_midside = true;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 10;
        }
        6 => {
            stereo.use_midside = true;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 12;
        }
        7 => {
            stereo.use_midside = true;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 12;
            sub.qlpc.quant_precision = 15;
        }
        _ => {
            stereo.use_midside = true;
            sub.use_lpc = true;
            sub.qlpc.lpc_order = 12;
            sub.qlpc.quant_precision = 15;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_s16le, encode_s16le, encode_with};
    use crate::codec::FlacSettings;

    #[test]
    fn sixteen_bit_stereo_round_trips() {
        let mut pcm = vec![0_i16; 480];
        for (index, sample) in pcm.iter_mut().enumerate() {
            *sample = (index as i16).wrapping_mul(17);
        }
        let encoded = encode_s16le(&pcm, 5).expect("encode");
        assert!(encoded.starts_with(b"fLaC"));
        let decoded = decode_s16le(&encoded).expect("decode");
        assert_eq!(decoded, pcm);
    }

    #[test]
    fn fast_and_max_compression_stay_sixteen_bit() {
        let mut pcm = vec![0_i16; 64];
        for (index, sample) in pcm.iter_mut().enumerate() {
            *sample = if index % 2 == 0 { 1_234 } else { -2_345 };
        }
        for level in [0_u8, 8] {
            let encoded = encode_with(&pcm, FlacSettings::new(level)).expect("encode");
            let decoded = decode_s16le(&encoded).expect("decode");
            assert_eq!(decoded, pcm, "level {level}");
        }
    }
}
