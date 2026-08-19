//! Narrow bindings to the system libdatachannel shared library.
//!
//! All FFI and unsafe code for the libdatachannel transport adapter is
//! quarantined here. Downstream crates receive owned handles and trait
//! callbacks; they do not need `unsafe`.

#![deny(unsafe_op_in_unsafe_fn)]

use core::ffi::{c_char, c_int, c_void};
use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CStr, CString};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

pub const RTC_ERR_SUCCESS: c_int = 0;
pub const RTC_ERR_INVALID: c_int = -1;
pub const RTC_ERR_FAILURE: c_int = -2;
pub const RTC_ERR_NOT_AVAIL: c_int = -3;
pub const RTC_ERR_TOO_SMALL: c_int = -4;

pub const RTC_NEW: c_int = 0;
pub const RTC_CONNECTING: c_int = 1;
pub const RTC_CONNECTED: c_int = 2;
pub const RTC_DISCONNECTED: c_int = 3;
pub const RTC_FAILED: c_int = 4;
pub const RTC_CLOSED: c_int = 5;

pub const RTC_GATHERING_NEW: c_int = 0;
pub const RTC_GATHERING_INPROGRESS: c_int = 1;
pub const RTC_GATHERING_COMPLETE: c_int = 2;

const RTC_CERTIFICATE_ECDSA: c_int = 1;
const RTC_TRANSPORT_POLICY_ALL: c_int = 0;
/// libdatachannel `RTC_CODEC_OPUS`.
pub const RTC_CODEC_OPUS: c_int = 128;
/// libdatachannel `RTC_DIRECTION_SENDONLY`.
pub const RTC_DIRECTION_SENDONLY: c_int = 1;
const OPUS_PAYLOAD_TYPE: u8 = 111;
const OPUS_CLOCK_RATE: u32 = 48_000;
const OPUS_SSRC: u32 = 0x5245_4C59;

#[repr(C)]
struct RtcConfiguration {
    ice_servers: *const *const c_char,
    ice_servers_count: c_int,
    proxy_server: *const c_char,
    bind_address: *const c_char,
    certificate_type: c_int,
    ice_transport_policy: c_int,
    enable_ice_tcp: bool,
    enable_ice_udp_mux: bool,
    disable_auto_negotiation: bool,
    force_media_transport: bool,
    port_range_begin: u16,
    port_range_end: u16,
    mtu: c_int,
    max_message_size: c_int,
}

#[repr(C)]
struct RtcTrackInit {
    direction: c_int,
    codec: c_int,
    payload_type: c_int,
    ssrc: u32,
    mid: *const c_char,
    name: *const c_char,
    msid: *const c_char,
    track_id: *const c_char,
    profile: *const c_char,
}

#[repr(C)]
struct RtcPacketizerInit {
    ssrc: u32,
    cname: *const c_char,
    payload_type: u8,
    clock_rate: u32,
    sequence_number: u16,
    timestamp: u32,
    max_fragment_size: u16,
    nal_separator: c_int,
    obu_packetization: c_int,
    playout_delay_id: u8,
    playout_delay_min: u16,
    playout_delay_max: u16,
    color_space_id: u8,
    color_chroma_siting_horz: u8,
    color_chroma_siting_vert: u8,
    color_range: u8,
    color_primaries: u8,
    color_transfer: u8,
    color_matrix: u8,
}

#[link(name = "datachannel")]
unsafe extern "C" {
    fn rtcPreload();
    fn rtcCreatePeerConnection(config: *const RtcConfiguration) -> c_int;
    fn rtcClosePeerConnection(pc: c_int) -> c_int;
    fn rtcDeletePeerConnection(pc: c_int) -> c_int;
    fn rtcSetUserPointer(id: c_int, ptr: *mut c_void);
    fn rtcSetLocalDescriptionCallback(pc: c_int, cb: RtcDescriptionCallback) -> c_int;
    fn rtcSetLocalCandidateCallback(pc: c_int, cb: RtcCandidateCallback) -> c_int;
    fn rtcSetStateChangeCallback(pc: c_int, cb: RtcStateCallback) -> c_int;
    fn rtcSetGatheringStateChangeCallback(pc: c_int, cb: RtcGatheringCallback) -> c_int;
    fn rtcSetDataChannelCallback(pc: c_int, cb: RtcDataChannelCallback) -> c_int;
    fn rtcSetLocalDescription(pc: c_int, ty: *const c_char) -> c_int;
    fn rtcSetRemoteDescription(pc: c_int, sdp: *const c_char, ty: *const c_char) -> c_int;
    fn rtcAddRemoteCandidate(pc: c_int, cand: *const c_char, mid: *const c_char) -> c_int;
    fn rtcCreateDataChannel(pc: c_int, label: *const c_char) -> c_int;
    fn rtcDeleteDataChannel(dc: c_int) -> c_int;
    fn rtcSetOpenCallback(id: c_int, cb: RtcOpenCallback) -> c_int;
    fn rtcSetClosedCallback(id: c_int, cb: RtcClosedCallback) -> c_int;
    fn rtcSetMessageCallback(id: c_int, cb: RtcMessageCallback) -> c_int;
    fn rtcSendMessage(id: c_int, data: *const c_char, size: c_int) -> c_int;
    fn rtcClose(id: c_int) -> c_int;
    fn rtcIsOpen(id: c_int) -> bool;
    fn rtcGetBufferedAmount(id: c_int) -> c_int;
    fn rtcSetBufferedAmountLowThreshold(id: c_int, amount: c_int) -> c_int;
    fn rtcSetBufferedAmountLowCallback(id: c_int, cb: RtcBufferedLowCallback) -> c_int;
    fn rtcAddTrackEx(pc: c_int, init: *const RtcTrackInit) -> c_int;
    fn rtcDeleteTrack(tr: c_int) -> c_int;
    fn rtcSetOpusPacketizer(tr: c_int, init: *const RtcPacketizerInit) -> c_int;
    fn rtcChainRtcpSrReporter(tr: c_int) -> c_int;
    fn rtcSetTrackRtpTimestamp(id: c_int, timestamp: u32) -> c_int;
    fn rtcGetCurrentTrackTimestamp(id: c_int, timestamp: *mut u32) -> c_int;
}

type RtcDescriptionCallback = Option<
    unsafe extern "C" fn(pc: c_int, sdp: *const c_char, ty: *const c_char, ptr: *mut c_void),
>;
type RtcCandidateCallback = Option<
    unsafe extern "C" fn(pc: c_int, cand: *const c_char, mid: *const c_char, ptr: *mut c_void),
>;
type RtcStateCallback = Option<unsafe extern "C" fn(pc: c_int, state: c_int, ptr: *mut c_void)>;
type RtcGatheringCallback = Option<unsafe extern "C" fn(pc: c_int, state: c_int, ptr: *mut c_void)>;
type RtcDataChannelCallback = Option<unsafe extern "C" fn(pc: c_int, dc: c_int, ptr: *mut c_void)>;
type RtcOpenCallback = Option<unsafe extern "C" fn(id: c_int, ptr: *mut c_void)>;
type RtcClosedCallback = Option<unsafe extern "C" fn(id: c_int, ptr: *mut c_void)>;
type RtcMessageCallback =
    Option<unsafe extern "C" fn(id: c_int, message: *const c_char, size: c_int, ptr: *mut c_void)>;
type RtcBufferedLowCallback = Option<unsafe extern "C" fn(id: c_int, ptr: *mut c_void)>;

/// ICE implementation linked into the loaded libdatachannel binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IceBackend {
    /// Default libjuice backend: STUN and TURN/UDP.
    Juice,
    /// libnice backend: TURN over TCP/TLS is available.
    Nice,
    /// Could not identify the linked ICE backend.
    Unknown,
}

/// Recoverable libdatachannel failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// A C string contained an interior NUL, or an argument was rejected.
    Invalid,
    /// The native library reported a runtime failure.
    Failure,
    /// The requested object is not available.
    NotAvail,
    /// An output buffer was too small.
    TooSmall,
}

/// Native events delivered on libdatachannel worker threads.
pub trait PeerCallbacks: Send + Sync {
    /// Local SDP is ready (`type` is `offer` or `answer`).
    fn on_local_description(&self, sdp: &str, ty: &str);
    /// A local trickle ICE candidate is ready.
    fn on_local_candidate(&self, candidate: &str, mid: &str);
    /// Peer connection state changed (`RTC_*` discriminant).
    fn on_state(&self, state: c_int);
    /// ICE gathering state changed (`RTC_GATHERING_*` discriminant).
    fn on_gathering(&self, state: c_int);
    /// A remote-created data channel arrived.
    fn on_data_channel(&self, dc: c_int);
    /// A data channel became open.
    fn on_open(&self, id: c_int);
    /// A data channel or peer closed.
    fn on_closed(&self, id: c_int);
    /// One complete binary or text message.
    fn on_message(&self, id: c_int, data: &[u8]);
    /// Buffered outbound bytes crossed the configured low-water mark.
    fn on_buffered_low(&self, id: c_int);
}

/// Construction options for one peer connection.
#[derive(Clone, Debug, Default)]
pub struct Configuration {
    /// libdatachannel ice-server URL strings (`stun:`, `turn:`, `turns:`).
    pub ice_servers: Vec<String>,
    /// Maximum data-channel message size in bytes. `0` uses the library default.
    pub max_message_size: i32,
    /// Advertise ICE-TCP candidates when the backend supports them.
    pub enable_ice_tcp: bool,
    /// Force DTLS-SRTP even before a track exists.
    pub force_media_transport: bool,
}

struct CallbackHolder {
    callbacks: Box<dyn PeerCallbacks>,
    live: AtomicBool,
}

/// Owns one libdatachannel peer connection.
///
/// Mutable methods are not required; the native object is internally
/// synchronized. The marker prevents `Sync` so a single owner remains explicit.
pub struct PeerConnection {
    id: c_int,
    holder: NonNull<CallbackHolder>,
    channels: Mutex<Vec<c_int>>,
    tracks: Mutex<Vec<c_int>>,
    _not_sync: PhantomData<Rc<()>>,
}

// SAFETY: libdatachannel peer ids have no thread affinity. `PeerConnection`
// uniquely owns the C object and the callback holder. Callbacks may run on
// library worker threads and only borrow the holder while `live` is true.
unsafe impl Send for PeerConnection {}

impl PeerConnection {
    /// Creates a peer connection and installs callbacks.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the native constructor rejects the configuration
    /// or an ice-server URL contains an interior NUL.
    pub fn create(
        config: &Configuration,
        callbacks: Box<dyn PeerCallbacks>,
    ) -> Result<Self, Error> {
        preload();
        let urls = config
            .ice_servers
            .iter()
            .map(|url| CString::new(url.as_str()).map_err(|_| Error::Invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let ptrs: Vec<*const c_char> = urls.iter().map(|url| url.as_ptr()).collect();
        let native = RtcConfiguration {
            ice_servers: if ptrs.is_empty() {
                core::ptr::null()
            } else {
                ptrs.as_ptr()
            },
            ice_servers_count: c_int::try_from(ptrs.len()).map_err(|_| Error::Invalid)?,
            proxy_server: core::ptr::null(),
            bind_address: core::ptr::null(),
            certificate_type: RTC_CERTIFICATE_ECDSA,
            ice_transport_policy: RTC_TRANSPORT_POLICY_ALL,
            enable_ice_tcp: config.enable_ice_tcp,
            enable_ice_udp_mux: false,
            disable_auto_negotiation: true,
            force_media_transport: config.force_media_transport,
            port_range_begin: 0,
            port_range_end: 0,
            mtu: 0,
            max_message_size: config.max_message_size,
        };
        // SAFETY: `native` and the ice-server C strings are valid for this call.
        // A non-negative return is a live peer-connection id owned by the caller.
        let id = unsafe { rtcCreatePeerConnection(&native) };
        if id < 0 {
            return Err(status(id));
        }
        let holder = Box::new(CallbackHolder {
            callbacks,
            live: AtomicBool::new(true),
        });
        let holder = NonNull::from(Box::leak(holder));
        // SAFETY: `holder` is a unique leaked allocation retained by `Self`.
        // libdatachannel stores the pointer and passes it back to trampolines
        // only while the peer id is alive. Trampolines check `live` first.
        unsafe {
            rtcSetUserPointer(id, holder.as_ptr().cast());
            let _ = rtcSetLocalDescriptionCallback(id, Some(on_local_description));
            let _ = rtcSetLocalCandidateCallback(id, Some(on_local_candidate));
            let _ = rtcSetStateChangeCallback(id, Some(on_state));
            let _ = rtcSetGatheringStateChangeCallback(id, Some(on_gathering));
            let _ = rtcSetDataChannelCallback(id, Some(on_data_channel));
        }
        Ok(Self {
            id,
            holder,
            channels: Mutex::new(Vec::new()),
            tracks: Mutex::new(Vec::new()),
            _not_sync: PhantomData,
        })
    }

    /// Returns the native peer-connection id.
    #[must_use]
    pub const fn id(&self) -> c_int {
        self.id
    }

    /// Creates or applies the local description of `ty` (`"offer"` / `"answer"`).
    pub fn set_local_description(&self, ty: &str) -> Result<(), Error> {
        let ty = CString::new(ty).map_err(|_| Error::Invalid)?;
        // SAFETY: `id` is live; `ty` is a NUL-terminated C string for the call.
        status_unit(unsafe { rtcSetLocalDescription(self.id, ty.as_ptr()) })
    }

    /// Installs the remote SDP of `ty`.
    pub fn set_remote_description(&self, sdp: &str, ty: &str) -> Result<(), Error> {
        let sdp = CString::new(sdp).map_err(|_| Error::Invalid)?;
        let ty = CString::new(ty).map_err(|_| Error::Invalid)?;
        // SAFETY: `id` is live; both C strings are valid for the call.
        status_unit(unsafe { rtcSetRemoteDescription(self.id, sdp.as_ptr(), ty.as_ptr()) })
    }

    /// Adds one remote trickle ICE candidate.
    pub fn add_remote_candidate(&self, candidate: &str, mid: Option<&str>) -> Result<(), Error> {
        let candidate = CString::new(candidate).map_err(|_| Error::Invalid)?;
        let mid = mid
            .map(CString::new)
            .transpose()
            .map_err(|_| Error::Invalid)?;
        let mid_ptr = mid
            .as_ref()
            .map_or(core::ptr::null(), |value| value.as_ptr());
        // SAFETY: `id` is live; `candidate` is a valid C string; `mid_ptr` is
        // either null (documented) or valid for the call.
        status_unit(unsafe { rtcAddRemoteCandidate(self.id, candidate.as_ptr(), mid_ptr) })
    }

    /// Creates a reliable ordered data channel and wires its callbacks.
    pub fn create_data_channel(&self, label: &str) -> Result<c_int, Error> {
        let label = CString::new(label).map_err(|_| Error::Invalid)?;
        // SAFETY: `id` is live; `label` is a valid C string. A non-negative
        // return is a data-channel id owned by this peer.
        let dc = unsafe { rtcCreateDataChannel(self.id, label.as_ptr()) };
        if dc < 0 {
            return Err(status(dc));
        }
        self.attach_channel(dc)?;
        Ok(dc)
    }

    /// Wires callbacks for a data channel created locally or by the remote.
    pub fn attach_channel(&self, dc: c_int) -> Result<(), Error> {
        if dc < 0 {
            return Err(Error::Invalid);
        }
        if let Ok(mut channels) = self.channels.lock()
            && !channels.contains(&dc)
        {
            channels.push(dc);
        }
        // SAFETY: `dc` is a live channel on this peer. The holder pointer is
        // the same allocation installed on the peer connection.
        unsafe {
            rtcSetUserPointer(dc, self.holder.as_ptr().cast());
            let _ = rtcSetOpenCallback(dc, Some(on_open));
            let _ = rtcSetClosedCallback(dc, Some(on_closed));
            let _ = rtcSetMessageCallback(dc, Some(on_message));
            let _ = rtcSetBufferedAmountLowCallback(dc, Some(on_buffered_low));
        }
        Ok(())
    }

    /// Sets the buffered-amount low-water mark for `dc`.
    pub fn set_buffered_amount_low(&self, dc: c_int, amount: i32) -> Result<(), Error> {
        // SAFETY: `dc` is a channel id previously returned to the caller.
        status_unit(unsafe { rtcSetBufferedAmountLowThreshold(dc, amount) })
    }

    /// Returns bytes currently queued to send on `dc`.
    #[must_use]
    pub fn buffered_amount(&self, dc: c_int) -> i32 {
        // SAFETY: `dc` is a channel id previously returned to the caller.
        unsafe { rtcGetBufferedAmount(dc) }
    }

    /// Reports whether `dc` is open.
    #[must_use]
    pub fn is_open(&self, dc: c_int) -> bool {
        // SAFETY: `dc` is a channel id previously returned to the caller.
        unsafe { rtcIsOpen(dc) }
    }

    /// Sends one complete binary message.
    pub fn send_binary(&self, dc: c_int, payload: &[u8]) -> Result<(), Error> {
        let size = i32::try_from(payload.len()).map_err(|_| Error::Invalid)?;
        // SAFETY: libdatachannel treats `size >= 0` as a binary payload of
        // exactly `size` bytes. `payload` is valid for that many bytes.
        status_unit(unsafe { rtcSendMessage(dc, payload.as_ptr().cast(), size) })
    }

    /// Adds a sendonly Opus audio track with an RTP packetizer.
    ///
    /// `rtcSendMessage` on the returned id expects encoded Opus frames, not PCM.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the native constructor or packetizer rejects the
    /// track.
    pub fn add_opus_sendonly_track(&self) -> Result<c_int, Error> {
        let mid = CString::new("0").map_err(|_| Error::Invalid)?;
        let name = CString::new("audio").map_err(|_| Error::Invalid)?;
        let msid = CString::new("relay").map_err(|_| Error::Invalid)?;
        let track_id = CString::new("audio").map_err(|_| Error::Invalid)?;
        let profile = CString::new("minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1")
            .map_err(|_| Error::Invalid)?;
        let cname = CString::new("relay").map_err(|_| Error::Invalid)?;
        let init = RtcTrackInit {
            direction: RTC_DIRECTION_SENDONLY,
            codec: RTC_CODEC_OPUS,
            payload_type: c_int::from(OPUS_PAYLOAD_TYPE),
            ssrc: OPUS_SSRC,
            mid: mid.as_ptr(),
            name: name.as_ptr(),
            msid: msid.as_ptr(),
            track_id: track_id.as_ptr(),
            profile: profile.as_ptr(),
        };
        // SAFETY: `init` and its C strings are valid for this call. A
        // non-negative return is a track id owned by this peer.
        let tr = unsafe { rtcAddTrackEx(self.id, &init) };
        if tr < 0 {
            return Err(status(tr));
        }
        let packetizer = RtcPacketizerInit {
            ssrc: OPUS_SSRC,
            cname: cname.as_ptr(),
            payload_type: OPUS_PAYLOAD_TYPE,
            clock_rate: OPUS_CLOCK_RATE,
            sequence_number: 0,
            timestamp: 0,
            max_fragment_size: 0,
            nal_separator: 0,
            obu_packetization: 0,
            playout_delay_id: 0,
            playout_delay_min: 0,
            playout_delay_max: 0,
            color_space_id: 0,
            color_chroma_siting_horz: 0,
            color_chroma_siting_vert: 0,
            color_range: 0,
            color_primaries: 0,
            color_transfer: 0,
            color_matrix: 0,
        };
        // SAFETY: `tr` is the live track from `rtcAddTrackEx`. `packetizer`
        // and `cname` are valid for the call.
        if let Err(error) = status_unit(unsafe { rtcSetOpusPacketizer(tr, &packetizer) }) {
            unsafe {
                let _ = rtcDeleteTrack(tr);
            }
            return Err(error);
        }
        // SAFETY: `tr` is still live; sender reports keep browser jitter
        // buffers from stalling on a sendonly stream.
        let _ = unsafe { rtcChainRtcpSrReporter(tr) };
        if let Ok(mut tracks) = self.tracks.lock()
            && !tracks.contains(&tr)
        {
            tracks.push(tr);
        }
        // SAFETY: track ids share the peer user pointer and open/closed
        // trampolines. The holder outlives the track.
        unsafe {
            rtcSetUserPointer(tr, self.holder.as_ptr().cast());
            let _ = rtcSetOpenCallback(tr, Some(on_open));
            let _ = rtcSetClosedCallback(tr, Some(on_closed));
        }
        Ok(tr)
    }

    /// Sends one encoded Opus frame on a sendonly track and advances RTP time.
    ///
    /// `timestamp_step` is 48 kHz samples in the frame (480 for 10 ms).
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the native send or timestamp update fails.
    pub fn send_opus_frame(
        &self,
        tr: c_int,
        payload: &[u8],
        timestamp_step: u32,
    ) -> Result<(), Error> {
        self.send_binary(tr, payload)?;
        let mut timestamp = 0_u32;
        // SAFETY: `tr` is a track id previously returned to the caller.
        let _ = unsafe { rtcGetCurrentTrackTimestamp(tr, &mut timestamp) };
        timestamp = timestamp.wrapping_add(timestamp_step);
        // SAFETY: `tr` is still live; timestamp is the RTP media clock.
        status_unit(unsafe { rtcSetTrackRtpTimestamp(tr, timestamp) })
    }

    /// Closes `dc` without deleting it.
    pub fn close_channel(&self, dc: c_int) -> Result<(), Error> {
        // SAFETY: `dc` is a channel id previously returned to the caller.
        status_unit(unsafe { rtcClose(dc) })
    }
}

impl Drop for PeerConnection {
    fn drop(&mut self) {
        // SAFETY: `holder` is the unique allocation created in `create`.
        // `live` is cleared first so trampolines no-op. Channel callbacks are
        // unset and ids deleted before the peer, matching the C API.
        // `rtcDeletePeerConnection` blocks until scheduled callbacks return.
        // `Drop` runs on the owner thread, never from a callback.
        unsafe {
            (*self.holder.as_ptr()).live.store(false, Ordering::Release);
            if let Ok(mut channels) = self.channels.lock() {
                for dc in channels.drain(..) {
                    let _ = rtcSetOpenCallback(dc, None);
                    let _ = rtcSetClosedCallback(dc, None);
                    let _ = rtcSetMessageCallback(dc, None);
                    let _ = rtcSetBufferedAmountLowCallback(dc, None);
                    let _ = rtcClose(dc);
                    let _ = rtcDeleteDataChannel(dc);
                }
            }
            if let Ok(mut tracks) = self.tracks.lock() {
                for tr in tracks.drain(..) {
                    let _ = rtcSetOpenCallback(tr, None);
                    let _ = rtcSetClosedCallback(tr, None);
                    let _ = rtcDeleteTrack(tr);
                }
            }
            let _ = rtcSetLocalDescriptionCallback(self.id, None);
            let _ = rtcSetLocalCandidateCallback(self.id, None);
            let _ = rtcSetStateChangeCallback(self.id, None);
            let _ = rtcSetGatheringStateChangeCallback(self.id, None);
            let _ = rtcSetDataChannelCallback(self.id, None);
            let _ = rtcClosePeerConnection(self.id);
            let _ = rtcDeletePeerConnection(self.id);
            drop(Box::from_raw(self.holder.as_ptr()));
        }
    }
}

/// Ensures the process-wide libdatachannel worker pool exists.
pub fn preload() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: `rtcPreload` is documented as optional process-wide init
        // and may be called from any non-callback thread.
        unsafe { rtcPreload() };
    });
}

/// Identifies the ICE backend of the loaded libdatachannel binary.
#[must_use]
pub fn ice_backend() -> IceBackend {
    preload();
    detect_ice_backend()
}

fn detect_ice_backend() -> IceBackend {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return IceBackend::Unknown;
    };
    let nice = maps.contains("libnice");
    let juice = maps.contains("libjuice");
    match (nice, juice) {
        (true, _) => IceBackend::Nice,
        (false, true) => IceBackend::Juice,
        (false, false) => IceBackend::Unknown,
    }
}

fn status(code: c_int) -> Error {
    match code {
        RTC_ERR_INVALID => Error::Invalid,
        RTC_ERR_NOT_AVAIL => Error::NotAvail,
        RTC_ERR_TOO_SMALL => Error::TooSmall,
        _ => Error::Failure,
    }
}

fn status_unit(code: c_int) -> Result<(), Error> {
    if code >= RTC_ERR_SUCCESS {
        Ok(())
    } else {
        Err(status(code))
    }
}

fn holder<'a>(ptr: *mut c_void) -> Option<&'a CallbackHolder> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the user pointer is the `CallbackHolder` installed in `create`
    // and is not freed until after `live` is cleared and the peer is deleted.
    let holder = unsafe { &*ptr.cast::<CallbackHolder>() };
    holder.live.load(Ordering::Acquire).then_some(holder)
}

fn cstr<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    // SAFETY: libdatachannel documents these callback strings as
    // NUL-terminated and valid for the duration of the callback.
    unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("")
}

unsafe extern "C" fn on_local_description(
    _pc: c_int,
    sdp: *const c_char,
    ty: *const c_char,
    ptr: *mut c_void,
) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_local_description(cstr(sdp), cstr(ty));
    }
}

unsafe extern "C" fn on_local_candidate(
    _pc: c_int,
    cand: *const c_char,
    mid: *const c_char,
    ptr: *mut c_void,
) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_local_candidate(cstr(cand), cstr(mid));
    }
}

unsafe extern "C" fn on_state(_pc: c_int, state: c_int, ptr: *mut c_void) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_state(state);
    }
}

unsafe extern "C" fn on_gathering(_pc: c_int, state: c_int, ptr: *mut c_void) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_gathering(state);
    }
}

unsafe extern "C" fn on_data_channel(_pc: c_int, dc: c_int, ptr: *mut c_void) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_data_channel(dc);
    }
}

unsafe extern "C" fn on_open(id: c_int, ptr: *mut c_void) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_open(id);
    }
}

unsafe extern "C" fn on_closed(id: c_int, ptr: *mut c_void) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_closed(id);
    }
}

unsafe extern "C" fn on_message(id: c_int, message: *const c_char, size: c_int, ptr: *mut c_void) {
    let Some(holder) = holder(ptr) else {
        return;
    };
    if message.is_null() {
        return;
    }
    // size >= 0: binary of that length. size < 0: UTF-8 including the NUL.
    let (ptr, len) = if size >= 0 {
        (message.cast::<u8>(), usize::try_from(size).unwrap_or(0))
    } else {
        let with_nul = usize::try_from(size.wrapping_neg()).unwrap_or(0);
        (message.cast::<u8>(), with_nul.saturating_sub(1))
    };
    // SAFETY: libdatachannel documents `message` as valid for the signed
    // `size` convention above for the duration of the callback.
    let data = unsafe { core::slice::from_raw_parts(ptr, len) };
    holder.callbacks.on_message(id, data);
}

unsafe extern "C" fn on_buffered_low(id: c_int, ptr: *mut c_void) {
    if let Some(holder) = holder(ptr) {
        holder.callbacks.on_buffered_low(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;

    struct Sink(Mutex<Vec<c_int>>);

    impl PeerCallbacks for Sink {
        fn on_local_description(&self, _sdp: &str, _ty: &str) {}
        fn on_local_candidate(&self, _candidate: &str, _mid: &str) {}
        fn on_state(&self, state: c_int) {
            self.0.lock().expect("sink").push(state);
        }
        fn on_gathering(&self, _state: c_int) {}
        fn on_data_channel(&self, _dc: c_int) {}
        fn on_open(&self, _id: c_int) {}
        fn on_closed(&self, _id: c_int) {}
        fn on_message(&self, _id: c_int, _data: &[u8]) {}
        fn on_buffered_low(&self, _id: c_int) {}
    }

    #[test]
    fn create_and_drop_peer_connection() {
        let sink = Arc::new(Sink(Mutex::new(Vec::new())));
        let peer = PeerConnection::create(
            &Configuration::default(),
            Box::new(CloneSink(Arc::clone(&sink))),
        );
        assert!(peer.is_ok(), "create peer");
        drop(peer);
    }

    struct CloneSink(Arc<Sink>);

    impl PeerCallbacks for CloneSink {
        fn on_local_description(&self, sdp: &str, ty: &str) {
            self.0.on_local_description(sdp, ty);
        }
        fn on_local_candidate(&self, candidate: &str, mid: &str) {
            self.0.on_local_candidate(candidate, mid);
        }
        fn on_state(&self, state: c_int) {
            self.0.on_state(state);
        }
        fn on_gathering(&self, state: c_int) {
            self.0.on_gathering(state);
        }
        fn on_data_channel(&self, dc: c_int) {
            self.0.on_data_channel(dc);
        }
        fn on_open(&self, id: c_int) {
            self.0.on_open(id);
        }
        fn on_closed(&self, id: c_int) {
            self.0.on_closed(id);
        }
        fn on_message(&self, id: c_int, data: &[u8]) {
            self.0.on_message(id, data);
        }
        fn on_buffered_low(&self, id: c_int) {
            self.0.on_buffered_low(id);
        }
    }

    #[test]
    fn native_struct_layouts_match_c() {
        assert_eq!(core::mem::size_of::<RtcTrackInit>(), 56);
        assert_eq!(core::mem::align_of::<RtcTrackInit>(), 8);
        assert_eq!(core::mem::size_of::<RtcPacketizerInit>(), 64);
        assert_eq!(core::mem::align_of::<RtcPacketizerInit>(), 8);
    }

    #[test]
    fn add_opus_sendonly_track_succeeds() {
        let sink = Arc::new(Sink(Mutex::new(Vec::new())));
        let peer = PeerConnection::create(
            &Configuration {
                force_media_transport: true,
                ..Configuration::default()
            },
            Box::new(CloneSink(Arc::clone(&sink))),
        )
        .expect("create peer");
        let track = peer.add_opus_sendonly_track();
        assert!(track.is_ok(), "add opus track: {track:?}");
        drop(peer);
    }

    #[test]
    fn ice_backend_is_identifiable_after_preload() {
        let backend = ice_backend();
        assert!(
            matches!(
                backend,
                IceBackend::Juice | IceBackend::Nice | IceBackend::Unknown
            ),
            "{backend:?}"
        );
    }
}
