//! Public named-session claim and PCM upload to `relay.matari-audio.com`.
//! Same-LAN browsers are served from [`crate::local_listen`] instead.

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::local_listen::LocalHub;
use relay_session::{
    CodecSettings, PUBLIC_LINK_ORIGIN, SessionControl, SessionRole, WIRE_BITS, local_ipv4_addrs,
    normalize_slug,
};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

type CloudSocket = WebSocket<MaybeTlsStream<std::net::TcpStream>>;

pub fn spawn(control: Arc<SessionControl>, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("relay-link-fanout".into())
        .spawn(move || run(control, stop))
        .expect("link fanout thread")
}

fn is_sender(role: SessionRole) -> bool {
    matches!(
        role,
        SessionRole::ConnectListen | SessionRole::StreamHub | SessionRole::StreamPublish
    )
}

/// 20 ms of 48 kHz stereo — one incoming DO message, lowest practical listen latency.
const WEB_BATCH_SAMPLES: usize = 48_000 / 50 * 2;
/// Keep at most 80 ms queued so a stall jumps to live instead of playing late.
const KEEP_SAMPLES: usize = 48_000 / 5 * 2;
/// ~−60 dBFS. Below this we treat a batch as silence.
const SILENCE_PEAK: f32 = 0.001;
/// Two 20 ms frames — delay send so silence/unsilence is visible 40 ms early.
const LOOKAHEAD_FRAMES: usize = 2;
/// 30 ms raised-cosine fade across DTX edges (stereo 48 kHz).
const FADE_SAMPLES: usize = 48_000 / 1000 * 30 * 2;

fn run(control: Arc<SessionControl>, stop: Arc<AtomicBool>) {
    let hub = LocalHub::start(Arc::clone(&control));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .user_agent("Mozilla/5.0 RELAY/0.1")
        .build();
    let mut last_claim = String::new();
    let mut last_cfg = String::new();
    let mut seq = 0_u32;
    let mut lan_seq = 0_u32;
    let mut socket: Option<CloudSocket> = None;
    let mut last_ws_try = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .unwrap_or_else(Instant::now);
    let mut ws_backoff = Duration::from_secs(1);
    let mut claim_backoff = Duration::from_secs(2);
    let mut dtx = Dtx::default();
    let mut delay = DelayLine::default();
    let mut fader = Fader::default();
    let mut listeners = 0_u32;
    while !stop.load(Ordering::Acquire) {
        let lan_n = hub.prune_and_count();
        control.set_lan_listeners(lan_n);
        if !control.linked() || !is_sender(control.role()) {
            control.set_web_ok(false);
            control.set_web_silent(false);
            control.set_web_listeners(0);
            socket = None;
            last_claim.clear();
            last_cfg.clear();
            listeners = 0;
            dtx = Dtx::default();
            delay = DelayLine::default();
            fader = Fader::default();
            thread::sleep(Duration::from_millis(80));
            continue;
        }
        let Ok(name) = control.session_name() else {
            thread::sleep(Duration::from_millis(80));
            continue;
        };
        let slug = normalize_slug(&name);
        if slug.is_empty() {
            thread::sleep(Duration::from_millis(80));
            continue;
        }
        let port = control.bind_port();
        let settings = control.codec_settings();
        let pass = control.password_hex();
        let device_rate = control.device_rate_hz();
        let block = control.block_frames();
        if control.web_wanted() {
            let key = claim_key(&slug, port, settings, &pass, hub.port());
            if key != last_claim {
                if claim(
                    &agent,
                    &slug,
                    port,
                    settings,
                    &pass,
                    device_rate,
                    block,
                    hub.port(),
                )
                .is_ok()
                {
                    last_claim = key;
                    last_cfg = cfg_json(settings, device_rate, block);
                    socket = None;
                    claim_backoff = Duration::from_secs(2);
                } else {
                    thread::sleep(claim_backoff);
                    claim_backoff = (claim_backoff * 2).min(Duration::from_secs(60));
                    continue;
                }
            }
            if socket.is_none() && last_ws_try.elapsed() >= ws_backoff {
                socket = open_in(&slug);
                last_ws_try = Instant::now();
                if socket.is_some() {
                    ws_backoff = Duration::from_secs(1);
                } else {
                    ws_backoff = (ws_backoff * 2).min(Duration::from_secs(30));
                }
            }
            if let Some(ws) = socket.as_mut() {
                let cfg = cfg_json(settings, device_rate, block);
                if cfg != last_cfg && send_keep(ws, Message::Text(cfg.clone().into())) {
                    last_cfg = cfg;
                }
            }
            if let Some(ws) = socket.as_mut() {
                if !service_socket(ws, &mut listeners) {
                    socket = None;
                }
            }
            control.set_web_listeners(listeners);
            control.set_web_ok(socket.is_some());
        } else {
            socket = None;
            last_claim.clear();
            listeners = 0;
            control.set_web_ok(false);
            control.set_web_silent(false);
            control.set_web_listeners(0);
        }
        match control.take_pcm_live(WEB_BATCH_SAMPLES, KEEP_SAMPLES) {
            Ok(pcm) if pcm.len() >= 480 => {
                let Some(outgoing) = delay.push(pcm) else {
                    continue;
                };
                if lan_n > 0 {
                    lan_seq = lan_seq.wrapping_add(1);
                    hub.broadcast_bin(&encode_frame(lan_seq, &outgoing));
                }
                if listeners == 0 || !control.web_wanted() {
                    dtx = Dtx::default();
                    fader = Fader::default();
                    control.set_web_silent(listeners == 0);
                } else {
                    let event =
                        dtx.push(is_silent(&outgoing), delay.future_silent(), fader.is_done());
                    let mut frame = outgoing;
                    match event {
                        DtxEvent::FadeOut => fader.start_out(),
                        DtxEvent::FadeIn => {
                            if let Some(ws) = socket.as_mut() {
                                let _ = send_keep(ws, Message::Text(r#"{"t":"go"}"#.into()));
                            }
                            fader.start_in();
                        }
                        DtxEvent::Hold => {}
                        DtxEvent::Speak => {}
                    }
                    if event != DtxEvent::Hold {
                        fader.apply(&mut frame);
                        send_pcm(&mut socket, &mut seq, &frame);
                        if dtx.phase == DtxPhase::FadingOut && fader.is_done() {
                            if let Some(ws) = socket.as_mut() {
                                let _ = send_keep(ws, Message::Text(r#"{"t":"dtx"}"#.into()));
                            }
                            dtx.phase = DtxPhase::Held;
                            dtx.held = true;
                        }
                    }
                    control.set_web_silent(dtx.held);
                }
                control.set_web_ok(socket.is_some());
            }
            _ => thread::sleep(Duration::from_millis(8)),
        }
    }
}

fn claim_key(slug: &str, port: u16, settings: CodecSettings, pass: &str, lan_http: u16) -> String {
    format!(
        "{slug}|{port}|{lan_http}|{}|{}|{}|{pass}",
        settings.codec().as_str(),
        settings.bitrate_kbps().unwrap_or(0),
        settings.flac_level().unwrap_or(0)
    )
}

fn cfg_json(settings: CodecSettings, device_rate: u32, block: u32) -> String {
    format!(
        "{{\"t\":\"cfg\",\"codec\":\"{}\",\"bitrate\":{},\"bits\":{},\"compression\":{},\"rate\":48000,\"deviceRate\":{device_rate},\"block\":{block}}}",
        settings.codec().as_str(),
        settings.bitrate_kbps().unwrap_or(0),
        settings.bits().max(WIRE_BITS),
        settings.flac_level().unwrap_or(0)
    )
}

fn is_silent(pcm: &[f32]) -> bool {
    pcm.iter().all(|s| s.abs() < SILENCE_PEAK)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DtxEvent {
    Speak,
    FadeOut,
    Hold,
    FadeIn,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DtxPhase {
    #[default]
    Live,
    FadingOut,
    Held,
    FadingIn,
}

#[derive(Default)]
struct Dtx {
    phase: DtxPhase,
    held: bool,
}

impl Dtx {
    /// `now` is the delayed frame about to go on the wire. `ahead` is true
    /// when the next ~40 ms is also below the silence floor.
    fn push(&mut self, now_silent: bool, ahead_silent: bool, fade_done: bool) -> DtxEvent {
        match self.phase {
            DtxPhase::Live => {
                if ahead_silent {
                    self.phase = DtxPhase::FadingOut;
                    return DtxEvent::FadeOut;
                }
                DtxEvent::Speak
            }
            DtxPhase::FadingOut => {
                if fade_done {
                    self.phase = DtxPhase::Held;
                    self.held = true;
                    return DtxEvent::Hold;
                }
                DtxEvent::Speak
            }
            DtxPhase::Held => {
                if now_silent {
                    return DtxEvent::Hold;
                }
                self.phase = DtxPhase::FadingIn;
                self.held = false;
                DtxEvent::FadeIn
            }
            DtxPhase::FadingIn => {
                if fade_done {
                    self.phase = DtxPhase::Live;
                }
                DtxEvent::Speak
            }
        }
    }
}

/// Raised-cosine gain ramp that spans multiple 20 ms batches.
struct Fader {
    remaining: i32,
    total: i32,
    dir: i8,
}

impl Default for Fader {
    fn default() -> Self {
        Self {
            remaining: 0,
            total: FADE_SAMPLES as i32,
            dir: 0,
        }
    }
}

impl Fader {
    fn start_out(&mut self) {
        self.total = FADE_SAMPLES as i32;
        self.remaining = self.total;
        self.dir = -1;
    }

    fn start_in(&mut self) {
        self.total = FADE_SAMPLES as i32;
        self.remaining = self.total;
        self.dir = 1;
    }

    fn is_done(&self) -> bool {
        self.dir == 0 || self.remaining <= 0
    }

    #[cfg(test)]
    fn is_muted(&self) -> bool {
        self.dir <= 0 && self.remaining <= 0
    }

    fn apply(&mut self, pcm: &mut [f32]) {
        if self.dir == 0 {
            return;
        }
        let total = self.total.max(1) as f32;
        for sample in pcm {
            if self.remaining <= 0 {
                if self.dir < 0 {
                    *sample = 0.0;
                }
                continue;
            }
            let t = 1.0 - self.remaining as f32 / total;
            let shaped = 0.5 - 0.5 * (core::f32::consts::PI * t).cos();
            let gain = if self.dir > 0 { shaped } else { 1.0 - shaped };
            *sample *= gain;
            self.remaining -= 1;
        }
        if self.remaining <= 0 {
            self.dir = 0;
        }
    }
}

#[derive(Default)]
struct DelayLine {
    frames: VecDeque<Vec<f32>>,
}

impl DelayLine {
    fn push(&mut self, pcm: Vec<f32>) -> Option<Vec<f32>> {
        self.frames.push_back(pcm);
        if self.frames.len() <= LOOKAHEAD_FRAMES {
            return None;
        }
        self.frames.pop_front()
    }

    fn future_silent(&self) -> bool {
        !self.frames.is_empty() && self.frames.iter().all(|frame| is_silent(frame))
    }
}

fn send_keep(ws: &mut CloudSocket, msg: Message) -> bool {
    match ws.send(msg) {
        Ok(()) => true,
        Err(tungstenite::Error::WriteBufferFull(_)) => true,
        Err(tungstenite::Error::Io(err))
            if matches!(
                err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
            ) =>
        {
            true
        }
        Err(_) => false,
    }
}

fn send_pcm(socket: &mut Option<CloudSocket>, seq: &mut u32, pcm: &[f32]) {
    let Some(ws) = socket.as_mut() else {
        return;
    };
    *seq = seq.wrapping_add(1);
    let bytes = encode_frame(*seq, pcm);
    if !send_keep(ws, Message::Binary(bytes.into())) {
        *socket = None;
    }
}

fn open_in(slug: &str) -> Option<CloudSocket> {
    let host = PUBLIC_LINK_ORIGIN
        .strip_prefix("https://")
        .unwrap_or(PUBLIC_LINK_ORIGIN);
    let url = format!("wss://{host}/{slug}/in");
    let (mut ws, _) = tungstenite::connect(url).ok()?;
    configure_socket(&mut ws);
    Some(ws)
}

fn configure_socket(ws: &mut CloudSocket) {
    match ws.get_mut() {
        MaybeTlsStream::Plain(tcp) => {
            let _ = tcp.set_nodelay(true);
            let _ = tcp.set_nonblocking(true);
        }
        MaybeTlsStream::Rustls(stream) => {
            let tcp = stream.get_mut();
            let _ = tcp.set_nodelay(true);
            let _ = tcp.set_nonblocking(true);
        }
        _ => {}
    }
}

/// Drain incoming frames. Listener count arrives as room events — no HTTP.
fn service_socket(ws: &mut CloudSocket, listeners: &mut u32) -> bool {
    loop {
        match ws.read() {
            Ok(Message::Ping(payload)) => {
                if !send_keep(ws, Message::Pong(payload)) {
                    return false;
                }
            }
            Ok(Message::Text(text)) => {
                if let Some(n) = parse_listeners(text.as_str()) {
                    *listeners = n;
                }
            }
            Ok(Message::Close(_)) => return false,
            Ok(_) => {}
            Err(tungstenite::Error::Io(err))
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                return true;
            }
            Err(tungstenite::Error::AlreadyClosed) => return false,
            Err(_) => return false,
        }
    }
}

fn claim_body(
    slug: &str,
    port: u16,
    settings: CodecSettings,
    pass: &str,
    device_rate: u32,
    block: u32,
    lan_http: u16,
) -> String {
    let lan = local_ipv4_addrs();
    let codec = settings.codec().as_str();
    let bitrate = settings.bitrate_kbps().unwrap_or(0);
    let compression = settings.flac_level().unwrap_or(0);
    let bits = settings.bits().max(WIRE_BITS);
    format!(
        "{{\"name\":\"{slug}\",\"port\":{port},\"lan\":{lan:?},\"lanHttp\":{lan_http},\"mode\":\"{codec}\",\"codec\":\"{codec}\",\"rate\":48000,\"deviceRate\":{device_rate},\"block\":{block},\"bitrate\":{bitrate},\"bits\":{bits},\"compression\":{compression},\"pass\":\"{pass}\"}}"
    )
}

fn claim(
    agent: &ureq::Agent,
    slug: &str,
    port: u16,
    settings: CodecSettings,
    pass: &str,
    device_rate: u32,
    block: u32,
    lan_http: u16,
) -> Result<(), ()> {
    let body = claim_body(slug, port, settings, pass, device_rate, block, lan_http);
    agent
        .post(&format!("{PUBLIC_LINK_ORIGIN}/api/claim"))
        .set("content-type", "application/json")
        .send_string(&body)
        .map(|_| ())
        .map_err(|_| ())
}

fn encode_frame(seq: u32, pcm: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + pcm.len() * 2);
    bytes.extend_from_slice(b"RLY1");
    bytes.extend_from_slice(&seq.to_le_bytes());
    for sample in pcm {
        let quant = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
        bytes.extend_from_slice(&quant.to_le_bytes());
    }
    bytes
}

fn parse_listeners(body: &str) -> Option<u32> {
    let key = "\"listeners\":";
    let rest = body.split(key).nth(1)?;
    let digits: String = rest
        .chars()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_listeners_from_info() {
        let body = r#"{"claim":{"name":"mix"},"listeners":4,"waiting":1}"#;
        assert_eq!(super::parse_listeners(body), Some(4));
        assert_eq!(super::parse_listeners("{}"), None);
    }

    #[test]
    fn encode_frame_has_magic_and_seq() {
        let bytes = super::encode_frame(7, &[0.0, 0.5]);
        assert_eq!(&bytes[..4], b"RLY1");
        assert_eq!(&bytes[4..8], 7_u32.to_le_bytes());
        assert_eq!(bytes.len(), 12);
    }

    #[test]
    fn claim_key_ignores_host_block() {
        let settings = relay_session::CodecSettings::live();
        let a = super::claim_key("mix", 17492, settings, "", 8787);
        let b = super::claim_key("mix", 17492, settings, "", 8787);
        assert_eq!(a, b);
        assert!(!a.contains("128"));
        assert!(!a.contains("44100"));
    }

    #[test]
    fn silence_is_below_threshold() {
        assert!(super::is_silent(&[0.0; 64]));
        assert!(super::is_silent(&[0.0004, -0.0004]));
        assert!(!super::is_silent(&[0.02, 0.0]));
    }

    #[test]
    fn dtx_fades_out_when_silence_is_ahead() {
        let mut dtx = super::Dtx::default();
        assert_eq!(dtx.push(false, false, true), super::DtxEvent::Speak);
        assert_eq!(dtx.push(false, true, true), super::DtxEvent::FadeOut);
        assert_eq!(dtx.push(true, true, false), super::DtxEvent::Speak);
        assert_eq!(dtx.push(true, true, true), super::DtxEvent::Hold);
        assert_eq!(dtx.push(true, true, true), super::DtxEvent::Hold);
        assert_eq!(dtx.push(false, false, true), super::DtxEvent::FadeIn);
        assert_eq!(dtx.push(false, false, false), super::DtxEvent::Speak);
        assert_eq!(dtx.push(false, false, true), super::DtxEvent::Speak);
    }

    #[test]
    fn delay_line_holds_lookahead_frames() {
        let mut delay = super::DelayLine::default();
        assert!(delay.push(vec![0.0; 4]).is_none());
        assert!(delay.push(vec![0.1; 4]).is_none());
        let first = delay.push(vec![0.2; 4]).expect("primed");
        assert_eq!(first[0], 0.0);
        assert!(!delay.future_silent());
    }

    #[test]
    fn fader_in_starts_near_zero() {
        let mut fader = super::Fader::default();
        fader.start_in();
        let mut pcm = vec![1.0_f32; 64];
        fader.apply(&mut pcm);
        assert!(pcm[0].abs() < 0.05);
        assert!(pcm[63] > pcm[0]);
    }

    #[test]
    fn fader_out_ends_near_zero() {
        let mut fader = super::Fader::default();
        fader.start_out();
        let mut pcm = vec![1.0_f32; super::FADE_SAMPLES];
        fader.apply(&mut pcm);
        assert!(pcm[0] > 0.9);
        assert!(pcm[pcm.len() - 1].abs() < 0.05);
        assert!(fader.is_muted());
    }

    #[test]
    fn cfg_json_names_codec() {
        let json = super::cfg_json(relay_session::CodecSettings::live(), 48_000, 256);
        assert!(json.contains("\"t\":\"cfg\""));
        assert!(json.contains("\"codec\":\"opus\""));
        assert!(json.contains("\"block\":256"));
    }

    #[test]
    fn claim_body_includes_daw_rate_and_block() {
        let body = super::claim_body(
            "mix",
            17492,
            relay_session::CodecSettings::live(),
            "",
            44_100,
            128,
            8787,
        );
        assert!(body.contains("\"rate\":48000"));
        assert!(body.contains("\"deviceRate\":44100"));
        assert!(body.contains("\"block\":128"));
        assert!(body.contains("\"lanHttp\":8787"));
    }
}
