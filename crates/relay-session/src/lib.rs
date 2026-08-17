//! Session engine for RELAY Connect, Stream, and native P2P media.
//!
//! Networking and codec work stay off the host audio callback. The callback
//! only copies into preallocated rings and renders from the playback consumer.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod auth;
mod codec;
mod engine;
mod flac;
mod plane;
mod runtime;
mod wire;

pub use auth::{password_allows, password_hex, password_token, token_from_hex};
pub use codec::{
    CodecSettings, FlacSettings, OPUS_BITRATE_DEFAULT_KBPS, OpusSettings, PcmSettings, WIRE_BITS,
    WireCodec,
};
pub use engine::{
    CallbackFace, DriveReport, EngineBuildError, EngineCommand, MonitorMode, SessionConfig,
    SessionEngine, SessionSnapshot, SessionWorker,
};
pub use plane::{PlaneError, SocketRole};
pub use relay_domain::{ConnectionState, MediaRoute, SessionMode};
pub use runtime::{
    ControlLockError, SessionControl, SessionRole, SessionRuntime, lan_listen_url,
    local_ipv4_addrs, normalize_slug, same_ipv4_24,
};
pub use wire::{WireError, WirePacket};

/// Default UDP bind used by local tests and the standalone CLIs.
pub const DEFAULT_CONNECT_PORT: u16 = 17_492;
/// Default UDP bind for a local Stream hub.
pub const DEFAULT_STREAM_PORT: u16 = 17_493;
/// Default HTTP port for the local named-session listen page.
pub const DEFAULT_LINK_HTTP_PORT: u16 = 8_787;
/// Deployed named-session origin.
pub const PUBLIC_LINK_ORIGIN: &str = "https://relay.matari-audio.com";
