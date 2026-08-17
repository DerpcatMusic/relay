//! Narrow bindings to the system libopus shared library.
//!
//! All FFI and unsafe code for `relay-opus` is quarantined here. The public
//! surface preserves the pointer, slice, lifetime, and single-owner invariants
//! required by libopus so downstream crates do not need unsafe code.

#![deny(unsafe_op_in_unsafe_fn)]

use core::ffi::{c_char, c_int};
use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::CStr;
use std::rc::Rc;

pub const OPUS_OK: c_int = 0;
pub const OPUS_BAD_ARG: c_int = -1;
pub const OPUS_BUFFER_TOO_SMALL: c_int = -2;
pub const OPUS_INTERNAL_ERROR: c_int = -3;
pub const OPUS_INVALID_PACKET: c_int = -4;
pub const OPUS_UNIMPLEMENTED: c_int = -5;
pub const OPUS_INVALID_STATE: c_int = -6;
pub const OPUS_ALLOC_FAIL: c_int = -7;

pub const OPUS_APPLICATION_VOIP: c_int = 2048;
pub const OPUS_APPLICATION_AUDIO: c_int = 2049;
pub const OPUS_APPLICATION_RESTRICTED_LOWDELAY: c_int = 2051;

const OPUS_SET_APPLICATION_REQUEST: c_int = 4000;
const OPUS_GET_APPLICATION_REQUEST: c_int = 4001;
const OPUS_SET_BITRATE_REQUEST: c_int = 4002;
const OPUS_GET_BITRATE_REQUEST: c_int = 4003;
const OPUS_SET_MAX_BANDWIDTH_REQUEST: c_int = 4004;
const OPUS_GET_MAX_BANDWIDTH_REQUEST: c_int = 4005;
const OPUS_SET_VBR_REQUEST: c_int = 4006;
const OPUS_GET_VBR_REQUEST: c_int = 4007;
const OPUS_SET_BANDWIDTH_REQUEST: c_int = 4008;
const OPUS_GET_BANDWIDTH_REQUEST: c_int = 4009;
const OPUS_SET_COMPLEXITY_REQUEST: c_int = 4010;
const OPUS_GET_COMPLEXITY_REQUEST: c_int = 4011;
const OPUS_SET_INBAND_FEC_REQUEST: c_int = 4012;
const OPUS_GET_INBAND_FEC_REQUEST: c_int = 4013;
const OPUS_SET_PACKET_LOSS_PERC_REQUEST: c_int = 4014;
const OPUS_GET_PACKET_LOSS_PERC_REQUEST: c_int = 4015;
const OPUS_SET_DTX_REQUEST: c_int = 4016;
const OPUS_GET_DTX_REQUEST: c_int = 4017;
const OPUS_SET_VBR_CONSTRAINT_REQUEST: c_int = 4020;
const OPUS_GET_VBR_CONSTRAINT_REQUEST: c_int = 4021;
const OPUS_SET_SIGNAL_REQUEST: c_int = 4024;
const OPUS_GET_SIGNAL_REQUEST: c_int = 4025;
const OPUS_RESET_STATE: c_int = 4028;

pub const OPUS_AUTO: c_int = -1000;
pub const OPUS_BANDWIDTH_NARROWBAND: c_int = 1101;
pub const OPUS_BANDWIDTH_MEDIUMBAND: c_int = 1102;
pub const OPUS_BANDWIDTH_WIDEBAND: c_int = 1103;
pub const OPUS_BANDWIDTH_SUPERWIDEBAND: c_int = 1104;
pub const OPUS_BANDWIDTH_FULLBAND: c_int = 1105;
pub const OPUS_SIGNAL_VOICE: c_int = 3001;
pub const OPUS_SIGNAL_MUSIC: c_int = 3002;

pub const MIN_BITRATE_BPS: c_int = 500;
pub const MAX_BITRATE_BPS: c_int = 512_000;
pub const MIN_COMPLEXITY: c_int = 0;
pub const MAX_COMPLEXITY: c_int = 10;
pub const MIN_PACKET_LOSS_PERCENT: c_int = 0;
pub const MAX_PACKET_LOSS_PERCENT: c_int = 100;

#[repr(C)]
struct OpusEncoder {
    _private: [u8; 0],
}

#[repr(C)]
struct OpusDecoder {
    _private: [u8; 0],
}

#[link(name = "opus")]
unsafe extern "C" {
    fn opus_encoder_create(
        sample_rate: i32,
        channels: c_int,
        application: c_int,
        error: *mut c_int,
    ) -> *mut OpusEncoder;
    fn opus_encoder_destroy(encoder: *mut OpusEncoder);
    fn opus_encode_float(
        encoder: *mut OpusEncoder,
        pcm: *const f32,
        frame_size: c_int,
        data: *mut u8,
        max_data_bytes: i32,
    ) -> i32;
    fn opus_encoder_ctl(encoder: *mut OpusEncoder, request: c_int, ...) -> c_int;

    fn opus_decoder_create(
        sample_rate: i32,
        channels: c_int,
        error: *mut c_int,
    ) -> *mut OpusDecoder;
    fn opus_decoder_destroy(decoder: *mut OpusDecoder);
    fn opus_decode_float(
        decoder: *mut OpusDecoder,
        data: *const u8,
        len: i32,
        pcm: *mut f32,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;
    fn opus_packet_get_nb_samples(data: *const u8, len: i32, sample_rate: i32) -> c_int;

    fn opus_get_version_string() -> *const c_char;
}

/// Owns one libopus encoder state.
///
/// Mutable methods prevent concurrent access. The marker prevents `Sync`, so a
/// state cannot be shared between threads without caller-provided synchronization.
pub struct Encoder {
    ptr: NonNull<OpusEncoder>,
    channels: usize,
    _not_sync: PhantomData<Rc<()>>,
}

impl Encoder {
    pub fn new(sample_rate: i32, channels: c_int, application: c_int) -> Result<Self, c_int> {
        let channels_usize = usize::try_from(channels).map_err(|_| OPUS_BAD_ARG)?;
        let mut error = OPUS_OK;
        // SAFETY: `error` is writable for the duration of the call. The other
        // arguments are plain values validated by libopus. Ownership of a
        // successful non-null allocation is transferred to `Self`.
        let raw = unsafe { opus_encoder_create(sample_rate, channels, application, &mut error) };
        let Some(ptr) = NonNull::new(raw) else {
            return Err(if error == OPUS_OK {
                OPUS_ALLOC_FAIL
            } else {
                error
            });
        };
        if error != OPUS_OK {
            // SAFETY: `ptr` came from `opus_encoder_create` and is not retained.
            unsafe { opus_encoder_destroy(ptr.as_ptr()) };
            return Err(error);
        }
        Ok(Self {
            ptr,
            channels: channels_usize,
            _not_sync: PhantomData,
        })
    }

    pub fn encode_float(
        &mut self,
        pcm: &[f32],
        frame_size: c_int,
        output: &mut [u8],
    ) -> Result<usize, c_int> {
        let frame_size_usize = usize::try_from(frame_size).map_err(|_| OPUS_BAD_ARG)?;
        let required = frame_size_usize
            .checked_mul(self.channels)
            .ok_or(OPUS_BAD_ARG)?;
        if pcm.len() != required || output.is_empty() {
            return Err(if output.is_empty() {
                OPUS_BUFFER_TOO_SMALL
            } else {
                OPUS_BAD_ARG
            });
        }
        let max_data_bytes = i32::try_from(output.len()).map_err(|_| OPUS_BAD_ARG)?;
        // SAFETY: this state is live and uniquely borrowed. The exact PCM
        // length was checked for `frame_size * channels`; output is non-empty,
        // writable for `max_data_bytes`, and does not overlap `pcm` in safe Rust.
        let result = unsafe {
            opus_encode_float(
                self.ptr.as_ptr(),
                pcm.as_ptr(),
                frame_size,
                output.as_mut_ptr(),
                max_data_bytes,
            )
        };
        nonnegative(result)
    }

    pub fn set_application(&mut self, application: c_int) -> Result<(), c_int> {
        if !matches!(
            application,
            OPUS_APPLICATION_VOIP | OPUS_APPLICATION_AUDIO | OPUS_APPLICATION_RESTRICTED_LOWDELAY
        ) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_APPLICATION_REQUEST, application)
    }

    pub fn application(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_APPLICATION_REQUEST)
    }

    pub fn set_bitrate(&mut self, bitrate_bps: c_int) -> Result<(), c_int> {
        if !(MIN_BITRATE_BPS..=MAX_BITRATE_BPS).contains(&bitrate_bps) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_BITRATE_REQUEST, bitrate_bps)
    }

    pub fn bitrate(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_BITRATE_REQUEST)
    }

    pub fn set_complexity(&mut self, complexity: c_int) -> Result<(), c_int> {
        if !(MIN_COMPLEXITY..=MAX_COMPLEXITY).contains(&complexity) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_COMPLEXITY_REQUEST, complexity)
    }

    pub fn complexity(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_COMPLEXITY_REQUEST)
    }

    pub fn set_vbr(&mut self, enabled: bool) -> Result<(), c_int> {
        self.set_int(OPUS_SET_VBR_REQUEST, c_int::from(enabled))
    }

    pub fn vbr(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_VBR_REQUEST)
    }

    pub fn set_vbr_constraint(&mut self, constrained: bool) -> Result<(), c_int> {
        self.set_int(OPUS_SET_VBR_CONSTRAINT_REQUEST, c_int::from(constrained))
    }

    pub fn vbr_constraint(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_VBR_CONSTRAINT_REQUEST)
    }

    pub fn set_max_bandwidth(&mut self, bandwidth: c_int) -> Result<(), c_int> {
        if !valid_concrete_bandwidth(bandwidth) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_MAX_BANDWIDTH_REQUEST, bandwidth)
    }

    pub fn max_bandwidth(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_MAX_BANDWIDTH_REQUEST)
    }

    pub fn set_bandwidth(&mut self, bandwidth: c_int) -> Result<(), c_int> {
        if bandwidth != OPUS_AUTO && !valid_concrete_bandwidth(bandwidth) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_BANDWIDTH_REQUEST, bandwidth)
    }

    pub fn bandwidth(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_BANDWIDTH_REQUEST)
    }

    pub fn set_signal(&mut self, signal: c_int) -> Result<(), c_int> {
        if !matches!(signal, OPUS_AUTO | OPUS_SIGNAL_VOICE | OPUS_SIGNAL_MUSIC) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_SIGNAL_REQUEST, signal)
    }

    pub fn signal(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_SIGNAL_REQUEST)
    }

    pub fn set_dtx(&mut self, enabled: bool) -> Result<(), c_int> {
        self.set_int(OPUS_SET_DTX_REQUEST, c_int::from(enabled))
    }

    pub fn dtx(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_DTX_REQUEST)
    }

    /// Values 0, 1, and 2 are the modes defined by libopus 1.6.
    pub fn set_inband_fec(&mut self, mode: c_int) -> Result<(), c_int> {
        if !(0..=2).contains(&mode) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_INBAND_FEC_REQUEST, mode)
    }

    pub fn inband_fec(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_INBAND_FEC_REQUEST)
    }

    pub fn set_packet_loss_percent(&mut self, percent: c_int) -> Result<(), c_int> {
        if !(MIN_PACKET_LOSS_PERCENT..=MAX_PACKET_LOSS_PERCENT).contains(&percent) {
            return Err(OPUS_BAD_ARG);
        }
        self.set_int(OPUS_SET_PACKET_LOSS_PERC_REQUEST, percent)
    }

    pub fn packet_loss_percent(&mut self) -> Result<c_int, c_int> {
        self.get_int(OPUS_GET_PACKET_LOSS_PERC_REQUEST)
    }

    pub fn reset(&mut self) -> Result<(), c_int> {
        // SAFETY: the state is live and uniquely borrowed; this request takes
        // no variadic argument.
        status(unsafe { opus_encoder_ctl(self.ptr.as_ptr(), OPUS_RESET_STATE) })
    }

    fn set_int(&mut self, request: c_int, value: c_int) -> Result<(), c_int> {
        // SAFETY: the state is live and uniquely borrowed. Every caller uses a
        // request whose single argument is exactly an `opus_int32`/C `int`.
        status(unsafe { opus_encoder_ctl(self.ptr.as_ptr(), request, value) })
    }

    fn get_int(&mut self, request: c_int) -> Result<c_int, c_int> {
        let mut value: c_int = 0;
        // SAFETY: the state is live and uniquely borrowed. Every caller uses a
        // request whose single argument is a writable `opus_int32`/C `int` pointer.
        status(unsafe { opus_encoder_ctl(self.ptr.as_ptr(), request, &mut value) })?;
        Ok(value)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: `Self` exclusively owns this state and destruction occurs once.
        unsafe { opus_encoder_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: libopus encoder states have no thread affinity and `&mut self` is
// required for every stateful operation. `Encoder` is deliberately not `Sync`.
unsafe impl Send for Encoder {}

/// Owns one libopus decoder state.
///
/// Mutable methods prevent concurrent access. The marker prevents `Sync`.
pub struct Decoder {
    ptr: NonNull<OpusDecoder>,
    channels: usize,
    _not_sync: PhantomData<Rc<()>>,
}

impl Decoder {
    pub fn new(sample_rate: i32, channels: c_int) -> Result<Self, c_int> {
        let channels_usize = usize::try_from(channels).map_err(|_| OPUS_BAD_ARG)?;
        let mut error = OPUS_OK;
        // SAFETY: `error` is writable for the duration of the call. Ownership
        // of a successful non-null allocation is transferred to `Self`.
        let raw = unsafe { opus_decoder_create(sample_rate, channels, &mut error) };
        let Some(ptr) = NonNull::new(raw) else {
            return Err(if error == OPUS_OK {
                OPUS_ALLOC_FAIL
            } else {
                error
            });
        };
        if error != OPUS_OK {
            // SAFETY: `ptr` came from `opus_decoder_create` and is not retained.
            unsafe { opus_decoder_destroy(ptr.as_ptr()) };
            return Err(error);
        }
        Ok(Self {
            ptr,
            channels: channels_usize,
            _not_sync: PhantomData,
        })
    }

    pub fn decode_float(
        &mut self,
        packet: Option<&[u8]>,
        output: &mut [f32],
        frame_size: c_int,
        decode_fec: bool,
    ) -> Result<usize, c_int> {
        let frame_size_usize = usize::try_from(frame_size).map_err(|_| OPUS_BAD_ARG)?;
        let required = frame_size_usize
            .checked_mul(self.channels)
            .ok_or(OPUS_BAD_ARG)?;
        if output.len() < required {
            return Err(OPUS_BUFFER_TOO_SMALL);
        }
        let (data, len) = match packet {
            Some(packet) if !packet.is_empty() => (
                packet.as_ptr(),
                i32::try_from(packet.len()).map_err(|_| OPUS_BAD_ARG)?,
            ),
            Some(_) => return Err(OPUS_BAD_ARG),
            None => (core::ptr::null(), 0),
        };
        let fec: c_int = decode_fec.into();
        // SAFETY: this state is live and uniquely borrowed. `data` is either
        // null with length zero (the documented PLC sentinel) or valid for
        // `len` bytes. Output was checked for `frame_size * channels` floats.
        let result = unsafe {
            opus_decode_float(
                self.ptr.as_ptr(),
                data,
                len,
                output.as_mut_ptr(),
                frame_size,
                fec,
            )
        };
        nonnegative(result)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: `Self` exclusively owns this state and destruction occurs once.
        unsafe { opus_decoder_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: libopus decoder states have no thread affinity and `&mut self` is
// required for every stateful operation. `Decoder` is deliberately not `Sync`.
unsafe impl Send for Decoder {}

/// Returns the duration encoded in one packet without mutating decoder state.
pub fn packet_samples_per_channel(packet: &[u8], sample_rate: i32) -> Result<usize, c_int> {
    if packet.is_empty() {
        return Err(OPUS_BAD_ARG);
    }
    let len = i32::try_from(packet.len()).map_err(|_| OPUS_BAD_ARG)?;
    // SAFETY: `packet` is non-empty and valid for `len` bytes. This libopus
    // packet inspection function retains no pointer and has no codec state.
    let result = unsafe { opus_packet_get_nb_samples(packet.as_ptr(), len, sample_rate) };
    nonnegative(result)
}

/// Returns libopus's process-lifetime version string without allocating.
pub fn version_string() -> &'static CStr {
    // SAFETY: libopus documents this as a static, NUL-terminated version
    // string. It remains valid for the process lifetime.
    unsafe { CStr::from_ptr(opus_get_version_string()) }
}

fn valid_concrete_bandwidth(value: c_int) -> bool {
    matches!(
        value,
        OPUS_BANDWIDTH_NARROWBAND
            | OPUS_BANDWIDTH_MEDIUMBAND
            | OPUS_BANDWIDTH_WIDEBAND
            | OPUS_BANDWIDTH_SUPERWIDEBAND
            | OPUS_BANDWIDTH_FULLBAND
    )
}

fn nonnegative(value: c_int) -> Result<usize, c_int> {
    if value < 0 {
        Err(value)
    } else {
        usize::try_from(value).map_err(|_| OPUS_INTERNAL_ERROR)
    }
}

fn status(value: c_int) -> Result<(), c_int> {
    if value == OPUS_OK { Ok(()) } else { Err(value) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoder() -> Encoder {
        Encoder::new(48_000, 2, OPUS_APPLICATION_AUDIO).expect("create test encoder")
    }

    #[test]
    fn libopus_1_6_control_constants_match_the_header() {
        assert_eq!(OPUS_SET_APPLICATION_REQUEST, 4000);
        assert_eq!(OPUS_GET_APPLICATION_REQUEST, 4001);
        assert_eq!(OPUS_SET_BITRATE_REQUEST, 4002);
        assert_eq!(OPUS_GET_BITRATE_REQUEST, 4003);
        assert_eq!(OPUS_SET_MAX_BANDWIDTH_REQUEST, 4004);
        assert_eq!(OPUS_GET_MAX_BANDWIDTH_REQUEST, 4005);
        assert_eq!(OPUS_SET_VBR_REQUEST, 4006);
        assert_eq!(OPUS_GET_VBR_REQUEST, 4007);
        assert_eq!(OPUS_SET_BANDWIDTH_REQUEST, 4008);
        assert_eq!(OPUS_GET_BANDWIDTH_REQUEST, 4009);
        assert_eq!(OPUS_SET_COMPLEXITY_REQUEST, 4010);
        assert_eq!(OPUS_GET_COMPLEXITY_REQUEST, 4011);
        assert_eq!(OPUS_SET_INBAND_FEC_REQUEST, 4012);
        assert_eq!(OPUS_GET_INBAND_FEC_REQUEST, 4013);
        assert_eq!(OPUS_SET_PACKET_LOSS_PERC_REQUEST, 4014);
        assert_eq!(OPUS_GET_PACKET_LOSS_PERC_REQUEST, 4015);
        assert_eq!(OPUS_SET_DTX_REQUEST, 4016);
        assert_eq!(OPUS_GET_DTX_REQUEST, 4017);
        assert_eq!(OPUS_SET_VBR_CONSTRAINT_REQUEST, 4020);
        assert_eq!(OPUS_GET_VBR_CONSTRAINT_REQUEST, 4021);
        assert_eq!(OPUS_SET_SIGNAL_REQUEST, 4024);
        assert_eq!(OPUS_GET_SIGNAL_REQUEST, 4025);
        assert_eq!(OPUS_RESET_STATE, 4028);
        assert_eq!(OPUS_BANDWIDTH_FULLBAND, 1105);
        assert_eq!(OPUS_SIGNAL_MUSIC, 3002);
    }

    #[test]
    fn checked_controls_reject_values_outside_libopus_ranges() {
        let mut encoder = encoder();

        assert_eq!(encoder.set_bitrate(499), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_bitrate(512_001), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_complexity(-1), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_complexity(11), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_application(2_050), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_max_bandwidth(OPUS_AUTO), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_max_bandwidth(1_106), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_bandwidth(1_106), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_signal(3_003), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_inband_fec(-1), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_inband_fec(3), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_packet_loss_percent(-1), Err(OPUS_BAD_ARG));
        assert_eq!(encoder.set_packet_loss_percent(101), Err(OPUS_BAD_ARG));

        for bitrate in [MIN_BITRATE_BPS, MAX_BITRATE_BPS] {
            assert_eq!(encoder.set_bitrate(bitrate), Ok(()));
        }
        for complexity in [MIN_COMPLEXITY, MAX_COMPLEXITY] {
            assert_eq!(encoder.set_complexity(complexity), Ok(()));
        }
        for percent in [MIN_PACKET_LOSS_PERCENT, MAX_PACKET_LOSS_PERCENT] {
            assert_eq!(encoder.set_packet_loss_percent(percent), Ok(()));
        }
    }

    #[test]
    fn set_and_get_controls_use_the_expected_integer_types() {
        let mut encoder = encoder();

        encoder
            .set_application(OPUS_APPLICATION_AUDIO)
            .expect("set application");
        encoder.set_bitrate(192_000).expect("set bitrate");
        encoder.set_complexity(10).expect("set complexity");
        encoder.set_vbr(true).expect("set VBR");
        encoder
            .set_vbr_constraint(true)
            .expect("set constrained VBR");
        encoder
            .set_max_bandwidth(OPUS_BANDWIDTH_FULLBAND)
            .expect("set maximum bandwidth");
        encoder.set_bandwidth(OPUS_AUTO).expect("set bandwidth");
        encoder.set_signal(OPUS_SIGNAL_MUSIC).expect("set signal");
        encoder.set_dtx(false).expect("set DTX");
        encoder.set_inband_fec(2).expect("set FEC");
        encoder.set_packet_loss_percent(25).expect("set loss hint");

        assert_eq!(encoder.application(), Ok(OPUS_APPLICATION_AUDIO));
        assert_eq!(encoder.bitrate(), Ok(192_000));
        assert_eq!(encoder.complexity(), Ok(10));
        assert_eq!(encoder.vbr(), Ok(1));
        assert_eq!(encoder.vbr_constraint(), Ok(1));
        assert_eq!(encoder.max_bandwidth(), Ok(OPUS_BANDWIDTH_FULLBAND));
        assert_eq!(encoder.bandwidth(), Ok(OPUS_BANDWIDTH_FULLBAND));
        assert_eq!(encoder.signal(), Ok(OPUS_SIGNAL_MUSIC));
        assert_eq!(encoder.dtx(), Ok(0));
        assert_eq!(encoder.inband_fec(), Ok(2));
        assert_eq!(encoder.packet_loss_percent(), Ok(25));
        encoder.reset().expect("reset encoder");
    }
    #[test]
    fn encoder_and_decoder_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Encoder>();
        assert_send::<Decoder>();
    }
}
