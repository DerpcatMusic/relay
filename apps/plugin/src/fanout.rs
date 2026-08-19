//! Public named-session claim and P2P signaling to `relay.matari-audio.com`.
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
        .spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(Arc::clone(&control), Arc::clone(&stop));
                }));
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if again.is_err() {
                    control.set_last_error("listen thread restarted");
                }
                thread::sleep(Duration::from_millis(80));
            }
        })
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
/// 20 ms raised-cosine fade across DTX edges (stereo 48 kHz).
const FADE_SAMPLES: usize = WEB_BATCH_SAMPLES;
/// 20 × 20 ms of already-quiet audio before we stop sending. Short gaps must
/// not fade a still-audible tail — that is the click on silence / unsilence.
const HANGOVER_FRAMES: u32 = 20;
/// Protocol pings keep the Cloudflare `/in` socket from being dropped idle.
const PING_EVERY: Duration = Duration::from_secs(12);
/// Host-callback starvation (DAW suspended / no PCM) after this many empty takes.
const STARVE_EMPTY: u32 = 12;

fn run(control: Arc<SessionControl>, stop: Arc<AtomicBool>) {
    let hub = LocalHub::start(Arc::clone(&control), Arc::clone(&stop));
    let mut p2p = crate::p2p::Hub::new();
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
    let mut last_ping = Instant::now();
    let mut empty_runs = 0_u32;
    let mut last_l = 0.0_f32;
    let mut last_r = 0.0_f32;
    let mut last_room = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut last_stat = String::new();
    while !stop.load(Ordering::Acquire) {
        let lan_n = hub.prune_and_count();
        control.set_lan_listeners(lan_n);
        let announce = last_room.elapsed() >= Duration::from_millis(400);
        if announce {
            hub.broadcast_text(&room_json(&control, lan_n, hub.port()));
            last_room = Instant::now();
        }
        if !control.linked() || !is_sender(control.role()) {
            control.set_web_ok(false);
            control.set_web_silent(false);
            control.set_web_listeners(0);
            socket = None;
            last_claim.clear();
            last_cfg.clear();
            last_stat.clear();
            listeners = 0;
            p2p.clear();
            dtx = Dtx::default();
            delay = DelayLine::default();
            fader = Fader::default();
            empty_runs = 0;
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
        let port = control
            .snapshot()
            .local_port
            .unwrap_or_else(|| control.bind_port());
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
                    last_cfg = cfg_json(settings, port, hub.port());
                    socket = None;
                    claim_backoff = Duration::from_secs(2);
                } else {
                    claim_backoff = (claim_backoff * 2).min(Duration::from_secs(60));
                    thread::sleep(claim_backoff.min(Duration::from_millis(80)));
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
                let cfg = cfg_json(settings, port, hub.port());
                if cfg != last_cfg && send_keep(ws, Message::Text(cfg.clone().into())) {
                    last_cfg = cfg;
                }
            }
            let sending_pcm = listeners > 0 && !control.web_silent();
            let mut inbound = Vec::new();
            if !keep_socket(
                &mut socket,
                &mut listeners,
                &mut last_ping,
                sending_pcm,
                &mut inbound,
            ) {
                socket = None;
                last_cfg.clear();
                last_stat.clear();
            }
            flush_p2p(
                &mut p2p,
                &mut socket,
                &inbound,
                &mut last_cfg,
                &mut last_stat,
            );
            listeners = p2p.peer_count();
            control.set_web_listeners(listeners);
            control.set_web_ok(socket.is_some());
            if let Some(ws) = socket.as_mut() {
                let stat = stat_json(&control, &p2p, lan_n);
                if stat != last_stat && send_keep(ws, Message::Text(stat.clone().into())) {
                    last_stat = stat;
                }
            }
        } else {
            socket = None;
            last_claim.clear();
            last_cfg.clear();
            last_stat.clear();
            listeners = 0;
            p2p.clear();
            control.set_web_ok(false);
            control.set_web_silent(false);
            control.set_web_listeners(0);
        }
        let wake = control.take_web_wake();
        match control.take_pcm_live(WEB_BATCH_SAMPLES, KEEP_SAMPLES) {
            Ok(pcm) if pcm.len() >= 480 => {
                empty_runs = 0;
                let Some(outgoing) = delay.push(pcm) else {
                    continue;
                };
                remember_tail(&outgoing, &mut last_l, &mut last_r);
                if lan_n > 0 {
                    lan_seq = lan_seq.wrapping_add(1);
                    hub.broadcast_bin(&encode_frame(lan_seq, &outgoing));
                }
                if listeners == 0 || !control.web_wanted() {
                    dtx = Dtx::default();
                    fader = Fader::default();
                    control.set_web_silent(false);
                } else {
                    p2p.push_pcm(&outgoing, control.bitrate_kbps());
                    control.set_web_silent(false);
                }
                control.set_web_ok(socket.is_some());
            }
            _ => {
                empty_runs = empty_runs.saturating_add(1);
                if listeners > 0
                    && control.web_wanted()
                    && empty_runs >= STARVE_EMPTY
                    && !dtx.held
                    && (last_l.abs() > SILENCE_PEAK
                        || last_r.abs() > SILENCE_PEAK
                        || !dtx.is_idle())
                {
                    let mut outgoing = fade_from_last(last_l, last_r);
                    last_l = 0.0;
                    last_r = 0.0;
                    fader.start_out();
                    fader.apply(&mut outgoing);
                    send_pcm(&mut socket, &mut seq, &outgoing);
                    if let Some(ws) = socket.as_mut() {
                        let _ = send_keep(ws, Message::Text(r#"{"t":"dtx"}"#.into()));
                    }
                    dtx.phase = DtxPhase::Held;
                    dtx.held = true;
                    dtx.silent_run = HANGOVER_FRAMES;
                    control.set_web_silent(true);
                } else if wake && dtx.held {
                    dtx.force_wake();
                    if let Some(ws) = socket.as_mut() {
                        let _ = send_keep(ws, Message::Text(r#"{"t":"go"}"#.into()));
                    }
                    control.set_web_silent(false);
                }
                let mut inbound = Vec::new();
                if !keep_socket(
                    &mut socket,
                    &mut listeners,
                    &mut last_ping,
                    false,
                    &mut inbound,
                ) {
                    socket = None;
                    last_cfg.clear();
                    last_stat.clear();
                }
                flush_p2p(
                    &mut p2p,
                    &mut socket,
                    &inbound,
                    &mut last_cfg,
                    &mut last_stat,
                );
                control.set_web_ok(socket.is_some());
                thread::sleep(Duration::from_millis(8));
            }
        }
    }
}

fn apply_web_frame(
    socket: &mut Option<CloudSocket>,
    seq: &mut u32,
    dtx: &mut Dtx,
    fader: &mut Fader,
    outgoing: Vec<f32>,
    ahead_silent: bool,
    wake: bool,
) {
    let event = dtx.push(is_silent(&outgoing), ahead_silent, fader.is_done(), wake);
    let mut frame = outgoing;
    match event {
        DtxEvent::FadeOut => fader.start_out(),
        DtxEvent::FadeIn => {
            if let Some(ws) = socket.as_mut() {
                let _ = send_keep(ws, Message::Text(r#"{"t":"go"}"#.into()));
            }
            if fader.is_fading_out() {
                fader.reverse();
            } else {
                fader.start_in();
            }
        }
        DtxEvent::Hold | DtxEvent::Speak => {}
    }
    if event != DtxEvent::Hold {
        fader.apply(&mut frame);
        send_pcm(socket, seq, &frame);
        if dtx.phase == DtxPhase::FadingOut && fader.is_done() {
            if let Some(ws) = socket.as_mut() {
                let _ = send_keep(ws, Message::Text(r#"{"t":"dtx"}"#.into()));
            }
            dtx.phase = DtxPhase::Held;
            dtx.held = true;
        }
    }
}

fn flush_p2p(
    p2p: &mut crate::p2p::Hub,
    socket: &mut Option<CloudSocket>,
    inbound: &[String],
    last_cfg: &mut String,
    last_stat: &mut String,
) {
    let mut outbound = Vec::new();
    p2p.apply_all(inbound, &mut outbound);
    p2p.drive(&mut outbound);
    let Some(ws) = socket.as_mut() else {
        return;
    };
    for msg in outbound {
        if !send_keep(ws, Message::Text(msg.into())) {
            *socket = None;
            last_cfg.clear();
            last_stat.clear();
            return;
        }
    }
}

fn keep_socket(
    socket: &mut Option<CloudSocket>,
    listeners: &mut u32,
    last_ping: &mut Instant,
    sending_pcm: bool,
    inbound: &mut Vec<String>,
) -> bool {
    let Some(ws) = socket.as_mut() else {
        return false;
    };
    if !sending_pcm && last_ping.elapsed() >= PING_EVERY {
        if !send_keep(ws, Message::Ping(Vec::new().into())) {
            return false;
        }
        *last_ping = Instant::now();
    }
    service_socket(ws, listeners, inbound)
}

fn remember_tail(pcm: &[f32], last_l: &mut f32, last_r: &mut f32) {
    if pcm.len() >= 2 {
        *last_l = pcm[pcm.len() - 2];
        *last_r = pcm[pcm.len() - 1];
    }
}

fn fade_from_last(last_l: f32, last_r: f32) -> Vec<f32> {
    let frames = WEB_BATCH_SAMPLES / 2;
    let mut out = vec![0.0; WEB_BATCH_SAMPLES];
    for i in 0..frames {
        let t = (i + 1) as f32 / frames as f32;
        let shaped = 0.5 - 0.5 * (core::f32::consts::PI * t).cos();
        let gain = 1.0 - shaped;
        out[i * 2] = last_l * gain;
        out[i * 2 + 1] = last_r * gain;
    }
    out
}

fn claim_key(
    slug: &str,
    _port: u16,
    _settings: CodecSettings,
    pass: &str,
    lan_http: u16,
) -> String {
    format!("{slug}|{lan_http}|{pass}")
}

fn cfg_json(settings: CodecSettings, port: u16, lan_http: u16) -> String {
    format!(
        "{{\"t\":\"cfg\",\"codec\":\"{}\",\"bitrate\":{},\"bits\":{},\"compression\":{},\"rate\":48000,\"port\":{port},\"lanHttp\":{lan_http}}}",
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
    silent_run: u32,
}

impl Dtx {
    fn is_idle(&self) -> bool {
        matches!(self.phase, DtxPhase::Live) && self.silent_run == 0
    }

    fn force_wake(&mut self) {
        self.phase = DtxPhase::Live;
        self.held = false;
        self.silent_run = 0;
    }

    /// `now` is the delayed frame about to go on the wire. `ahead` is true
    /// when the next ~40 ms is also below the silence floor.
    fn push(
        &mut self,
        now_silent: bool,
        ahead_silent: bool,
        fade_done: bool,
        wake: bool,
    ) -> DtxEvent {
        if now_silent {
            self.silent_run = self.silent_run.saturating_add(1);
        } else {
            self.silent_run = 0;
        }
        match self.phase {
            DtxPhase::Live => {
                if wake {
                    return DtxEvent::Speak;
                }
                if now_silent && ahead_silent && self.silent_run >= HANGOVER_FRAMES {
                    self.phase = DtxPhase::FadingOut;
                    return DtxEvent::FadeOut;
                }
                DtxEvent::Speak
            }
            DtxPhase::FadingOut => {
                if wake || !now_silent {
                    self.phase = DtxPhase::FadingIn;
                    self.held = false;
                    return DtxEvent::FadeIn;
                }
                if fade_done {
                    self.phase = DtxPhase::Held;
                    self.held = true;
                    return DtxEvent::Hold;
                }
                DtxEvent::Speak
            }
            DtxPhase::Held => {
                if wake || !now_silent {
                    self.phase = DtxPhase::FadingIn;
                    self.held = false;
                    return DtxEvent::FadeIn;
                }
                DtxEvent::Hold
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

    fn is_fading_out(&self) -> bool {
        self.dir < 0 && self.remaining > 0
    }

    fn reverse(&mut self) {
        if self.dir >= 0 || self.remaining <= 0 {
            return;
        }
        self.remaining = self.total.saturating_sub(self.remaining);
        self.dir = 1;
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
fn service_socket(ws: &mut CloudSocket, listeners: &mut u32, inbound: &mut Vec<String>) -> bool {
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
                if is_rtc_signal(text.as_str()) {
                    inbound.push(text.to_string());
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

fn room_json(control: &SessionControl, lan_n: u32, lan_http: u16) -> String {
    let snap = control.snapshot();
    let live = lan_n > 0 || snap.peers > 0 || control.web_listeners() > 0;
    format!(
        "{{\"t\":\"room\",\"host\":true,\"live\":{live},\"silent\":{},\"listeners\":{lan_n},\"peers\":{},\"dropouts\":{},\"port\":{lan_http},\"asleep\":{}}}",
        control.web_silent(),
        snap.peers,
        snap.dropouts,
        control.web_silent()
    )
}

fn stat_json(control: &SessionControl, p2p: &crate::p2p::Hub, lan_n: u32) -> String {
    let snap = control.snapshot();
    format!(
        "{{\"t\":\"stat\",\"dropouts\":{},\"peers\":{},\"lan\":{lan_n},\"web\":{},\"ready\":{},\"sent\":{},\"peak\":{},\"port\":{}}}",
        snap.dropouts,
        snap.peers,
        p2p.peer_count(),
        p2p.ready_count(),
        p2p.frames_sent(),
        format!("{:.3}", p2p.last_peak()),
        snap.local_port.unwrap_or(0)
    )
}

fn is_rtc_signal(body: &str) -> bool {
    body.contains("\"t\":\"want\"")
        || body.contains("\"t\":\"answer\"")
        || body.contains("\"t\":\"ice\"")
        || body.contains("\"t\":\"bye\"")
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
    #[ignore = "hits production relay.matari-audio.com"]
    fn live_cloud_claim_and_in_socket() {
        let slug = format!(
            "diag-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(1)
        );
        let settings = relay_session::CodecSettings::live();
        super::claim(
            &ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(4))
                .user_agent("Mozilla/5.0 RELAY/diag")
                .build(),
            &slug,
            17_492,
            settings,
            "",
            48_000,
            128,
            8_787,
        )
        .expect("ureq claim against relay.matari-audio.com");
        let socket = super::open_in(&slug);
        assert!(
            socket.is_some(),
            "tungstenite rustls /in must open (same crates as the plugin fan-out)"
        );
    }

    #[test]
    fn encode_frame_has_magic_and_seq() {
        let bytes = super::encode_frame(7, &[0.0, 0.5]);
        assert_eq!(&bytes[..4], b"RLY1");
        assert_eq!(&bytes[4..8], 7_u32.to_le_bytes());
        assert_eq!(bytes.len(), 12);
    }

    #[test]
    fn claim_key_is_room_identity() {
        let settings = relay_session::CodecSettings::live();
        let a = super::claim_key("mix", 17492, settings, "", 8787);
        let b = super::claim_key("mix", 18000, settings, "", 8787);
        assert_eq!(a, b);
        assert!(!a.contains("17492"));
        assert!(!a.contains("opus"));
        assert!(!a.contains("192"));
    }

    #[test]
    fn silence_is_below_threshold() {
        assert!(super::is_silent(&[0.0; 64]));
        assert!(super::is_silent(&[0.0004, -0.0004]));
        assert!(!super::is_silent(&[0.02, 0.0]));
    }

    #[test]
    fn dtx_ignores_short_quiet_gaps() {
        let mut dtx = super::Dtx::default();
        assert_eq!(dtx.push(false, false, true, false), super::DtxEvent::Speak);
        for _ in 0..super::HANGOVER_FRAMES - 1 {
            assert_eq!(dtx.push(true, true, true, false), super::DtxEvent::Speak);
        }
        assert_eq!(dtx.push(false, false, true, false), super::DtxEvent::Speak);
    }

    #[test]
    fn dtx_fades_out_after_hangover() {
        let mut dtx = super::Dtx::default();
        for _ in 0..super::HANGOVER_FRAMES - 1 {
            assert_eq!(dtx.push(true, true, true, false), super::DtxEvent::Speak);
        }
        assert_eq!(dtx.push(true, true, true, false), super::DtxEvent::FadeOut);
        assert_eq!(dtx.push(true, true, false, false), super::DtxEvent::Speak);
        assert_eq!(dtx.push(true, true, true, false), super::DtxEvent::Hold);
        assert_eq!(dtx.push(true, true, true, false), super::DtxEvent::Hold);
        assert_eq!(dtx.push(false, false, true, false), super::DtxEvent::FadeIn);
        assert_eq!(dtx.push(false, false, false, false), super::DtxEvent::Speak);
        assert_eq!(dtx.push(false, false, true, false), super::DtxEvent::Speak);
    }

    #[test]
    fn dtx_aborts_fade_out_when_audio_returns() {
        let mut dtx = super::Dtx::default();
        for _ in 0..super::HANGOVER_FRAMES {
            let _ = dtx.push(true, true, true, false);
        }
        assert_eq!(dtx.phase, super::DtxPhase::FadingOut);
        assert_eq!(
            dtx.push(false, false, false, false),
            super::DtxEvent::FadeIn
        );
    }

    #[test]
    fn dtx_wake_leaves_hold() {
        let mut dtx = super::Dtx::default();
        dtx.phase = super::DtxPhase::Held;
        dtx.held = true;
        assert_eq!(dtx.push(true, true, true, true), super::DtxEvent::FadeIn);
        assert!(!dtx.held);
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
    fn fader_reverse_continues_from_current_gain() {
        let mut fader = super::Fader::default();
        fader.start_out();
        let mut first = vec![1.0_f32; 64];
        fader.apply(&mut first);
        let at_reverse = first[63];
        assert!(fader.is_fading_out());
        fader.reverse();
        let mut next = vec![1.0_f32; 64];
        fader.apply(&mut next);
        assert!(next[0] > 0.0);
        assert!((next[0] - at_reverse).abs() < 0.15);
        assert!(next[63] > next[0]);
    }

    #[test]
    fn fade_from_last_starts_near_tail() {
        let pcm = super::fade_from_last(0.8, -0.4);
        assert!((pcm[0] - 0.8).abs() < 0.05);
        assert!((pcm[1] + 0.4).abs() < 0.05);
        assert!(pcm[pcm.len() - 2].abs() < 0.05);
        assert!(pcm[pcm.len() - 1].abs() < 0.05);
    }

    #[test]
    fn cfg_json_names_codec() {
        let json = super::cfg_json(relay_session::CodecSettings::live(), 17_492, 8787);
        assert!(json.contains("\"t\":\"cfg\""));
        assert!(json.contains("\"codec\":\"opus\""));
        assert!(json.contains("\"port\":17492"));
        assert!(!json.contains("deviceRate"));
        assert!(!json.contains("\"block\""));
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
