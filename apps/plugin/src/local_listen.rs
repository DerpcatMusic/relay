//! Same-LAN browser listen: HTTP page + WebSocket PCM on :8787.
//!
//! Never touches Cloudflare. The public listen page redirects here when
//! WebRTC host candidates share a /24 with the plugin.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use relay_session::{DEFAULT_LINK_HTTP_PORT, SessionControl, normalize_slug};
use tungstenite::{Message, WebSocket};

type LocalSocket = WebSocket<TcpStream>;

pub struct LocalHub {
    clients: Mutex<Vec<LocalSocket>>,
    port: u16,
    accept_stop: Arc<AtomicBool>,
}

impl Drop for LocalHub {
    fn drop(&mut self) {
        self.accept_stop.store(true, Ordering::Release);
    }
}

impl LocalHub {
    pub fn start(control: Arc<SessionControl>, stop: Arc<AtomicBool>) -> Arc<Self> {
        let (listener, port) = bind_listen();
        let accept_stop = Arc::new(AtomicBool::new(false));
        let hub = Arc::new(Self {
            clients: Mutex::new(Vec::new()),
            port,
            accept_stop: Arc::clone(&accept_stop),
        });
        control.set_lan_http_port(port);
        if let Some(listener) = listener {
            let accept_hub = Arc::clone(&hub);
            let accept_control = Arc::clone(&control);
            thread::Builder::new()
                .name("relay-lan-http".into())
                .spawn(move || {
                    accept_loop(listener, accept_hub, accept_control, stop, accept_stop);
                })
                .expect("lan http thread");
        }
        hub
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn prune_and_count(&self) -> u32 {
        let Ok(mut clients) = self.clients.lock() else {
            return 0;
        };
        clients.retain_mut(alive);
        u32::try_from(clients.len()).unwrap_or(u32::MAX)
    }

    pub fn broadcast_bin(&self, bytes: &[u8]) {
        self.send_all(Message::Binary(bytes.to_vec().into()));
    }

    pub fn broadcast_text(&self, text: &str) {
        self.send_all(Message::Text(text.to_owned().into()));
    }

    fn send_all(&self, msg: Message) {
        let Ok(mut clients) = self.clients.lock() else {
            return;
        };
        clients.retain_mut(|ws| send_keep(ws, msg.clone()));
    }

    fn push(&self, mut ws: LocalSocket) {
        configure_socket(&mut ws);
        if let Ok(mut clients) = self.clients.lock() {
            clients.push(ws);
        }
    }
}

fn bind_listen() -> (Option<TcpListener>, u16) {
    let first = DEFAULT_LINK_HTTP_PORT;
    for _ in 0..20 {
        let addr = SocketAddr::from(([0, 0, 0, 0], first));
        if let Ok(listener) = TcpListener::bind(addr) {
            let _ = listener.set_nonblocking(true);
            return (Some(listener), first);
        }
        thread::sleep(Duration::from_millis(15));
    }
    for port in first + 1..=first + 12 {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        if let Ok(listener) = TcpListener::bind(addr) {
            let _ = listener.set_nonblocking(true);
            return (Some(listener), port);
        }
    }
    (None, 0)
}

fn accept_loop(
    listener: TcpListener,
    hub: Arc<LocalHub>,
    control: Arc<SessionControl>,
    stop: Arc<AtomicBool>,
    accept_stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) && !accept_stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let hub = Arc::clone(&hub);
                let control = Arc::clone(&control);
                thread::spawn(move || handle_client(stream, &hub, &control));
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(80)),
        }
    }
}

fn handle_client(stream: TcpStream, hub: &LocalHub, control: &SessionControl) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut peek = [0_u8; 2048];
    let n = match stream.peek(&mut peek) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };
    let head = String::from_utf8_lossy(&peek[..n]).into_owned();
    let first = head.lines().next().unwrap_or("").to_owned();
    let upgrade = head.to_ascii_lowercase().contains("upgrade: websocket");
    if upgrade {
        if let Ok(ws) = tungstenite::accept(stream) {
            hub.push(ws);
        }
        return;
    }
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let mut stream = stream;
    let _ = stream.read(&mut peek);
    match (method.as_str(), path.as_str()) {
        ("GET", "/health") | ("GET", "/probe") => {
            write_http(&mut stream, 200, "text/plain; charset=utf-8", b"ok");
        }
        ("GET", "/status") => {
            let name = control.session_name().unwrap_or_default();
            let body = format!(
                "{{\"ok\":true,\"name\":\"{}\",\"port\":{}}}",
                normalize_slug(&name),
                hub.port()
            );
            write_http(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                body.as_bytes(),
            );
        }
        ("GET", "/") => {
            let listing = control
                .session_name()
                .ok()
                .filter(|name| !name.is_empty())
                .map(|name| {
                    format!(
                        "<li><a href=\"/{name}\">{name}</a> · LAN {port}</li>",
                        port = hub.port()
                    )
                })
                .unwrap_or_default();
            write_http(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                index_html(&listing).as_bytes(),
            );
        }
        ("GET", request_path) => {
            let slug = normalize_slug(
                request_path
                    .trim_start_matches('/')
                    .trim_end_matches("/out"),
            );
            if slug.is_empty() {
                write_http(&mut stream, 404, "text/plain", b"missing name");
                return;
            }
            write_http(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                player_html(&slug).as_bytes(),
            );
        }
        _ => write_http(&mut stream, 405, "text/plain", b"method"),
    }
}

fn alive(ws: &mut LocalSocket) -> bool {
    match ws.read() {
        Ok(Message::Ping(payload)) => send_keep(ws, Message::Pong(payload)),
        Ok(Message::Close(_)) => false,
        Ok(_) => true,
        Err(tungstenite::Error::Io(err))
            if matches!(
                err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
            ) =>
        {
            true
        }
        Err(tungstenite::Error::AlreadyClosed) => false,
        Err(_) => false,
    }
}

fn send_keep(ws: &mut LocalSocket, msg: Message) -> bool {
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

fn configure_socket(ws: &mut LocalSocket) {
    let tcp = ws.get_mut();
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_nonblocking(true);
}

fn write_http(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn index_html(listing: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>RELAY</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Barlow:wght@500;600;700&display=swap" rel="stylesheet">
<style>:root{{--bg:#191919;--lane:#252525;--text:#fff;--muted:#b8b8b8;--accent:#00aaff;--ice:#25e7ff}}
*{{box-sizing:border-box}}html,body{{margin:0;min-height:100%;background:var(--bg);color:var(--text);font-family:Barlow,system-ui,sans-serif}}
body{{display:flex;justify-content:center;padding:28px 20px 48px}}.wrap{{width:min(360px,100%)}}
.nav{{display:flex;align-items:center;gap:10px;height:36px;margin:0 0 28px}}
.product{{font-size:13px;font-weight:700;letter-spacing:.16em}}h1{{font-size:22px;margin:0 0 6px;font-weight:600}}
.who{{margin:0 0 22px;color:var(--muted);font-size:13px}}a{{color:var(--ice)}}ul{{padding-left:1.1em}}</style>
</head><body><main class="wrap"><header class="nav"><span class="product">RELAY</span></header>
<h1>LAN</h1><p class="who">Same network — not the cloud.</p><ul>{listing}</ul></main></body></html>"#
    )
}

fn player_html(name: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>RELAY · {name}</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Barlow:wght@500;600;700&display=swap" rel="stylesheet">
<style>
:root{{--bg:#191919;--lane:#252525;--text:#fff;--muted:#b8b8b8;--accent:#00aaff;--ok:#5be8b3;--warn:#ffc75c;--hot:#ff7088}}
*{{box-sizing:border-box}}html,body{{margin:0;min-height:100%;background:var(--bg);color:var(--text);font-family:Barlow,system-ui,sans-serif}}
::selection{{background:var(--accent);color:#041018}}
body{{display:flex;justify-content:center;padding:28px 20px 48px}}
.wrap{{width:min(360px,100%)}}
.nav{{display:flex;align-items:center;gap:10px;height:36px;margin:0 0 28px}}
.product{{font-size:13px;font-weight:700;letter-spacing:.16em}}
h1{{font-size:22px;line-height:1.15;letter-spacing:-.02em;margin:0 0 6px;font-weight:600}}
.who{{margin:0 0 22px;color:var(--muted);font-size:13px;min-height:1.2em}}
.desk{{display:flex;align-items:stretch;gap:18px;height:148px}}
.meter{{flex:1;display:flex;align-items:center}}
.strip{{position:relative;width:100%;height:28px;border-radius:4px;overflow:hidden;background:linear-gradient(90deg,#3d8f6a 0%,#5be8b3 42%,#ffc75c 78%,#ff7088 100%)}}
.cover{{position:absolute;top:0;right:0;bottom:0;width:100%;background:var(--lane)}}
.hold{{position:absolute;top:0;bottom:0;left:0;width:2px;background:#fff}}
.vol{{display:flex;flex-direction:column;align-items:center;justify-content:space-between;gap:8px;width:28px;flex:none}}
.vol input{{appearance:slider-vertical;writing-mode:vertical-lr;direction:rtl;width:22px;flex:1;margin:0;accent-color:var(--accent);background:transparent}}
.vol span{{font-size:12px;color:var(--muted);font-variant-numeric:tabular-nums}}
.gate{{position:fixed;inset:0;display:grid;place-items:center;background:#191919f2;z-index:20}}
.gate button{{font:inherit;font-weight:700;font-size:16px;border:0;border-radius:8px;padding:12px 22px;background:var(--accent);color:#041018;cursor:pointer}}
</style></head>
<body>
<main class="wrap">
  <header class="nav"><span class="product">RELAY</span></header>
  <h1>{name}</h1>
  <p class="who" id="who">Waiting</p>
  <div class="desk">
    <div class="meter"><div class="strip" aria-label="Level"><div class="cover" id="cover"></div><div class="hold" id="hold"></div></div></div>
    <label class="vol">
      <input id="vol" type="range" min="0" max="2" step="0.01" value="1" orient="vertical" aria-label="Volume">
      <span id="voln">0 dB</span>
    </label>
  </div>
</main>
<div class="gate" id="gate"><div><button id="go" type="button">Listen</button></div></div>
<script>
const name = {name:?};
const who = document.getElementById('who');
const cover = document.getElementById('cover');
const holdEl = document.getElementById('hold');
const vol = document.getElementById('vol');
const voln = document.getElementById('voln');
const hold = {{ p: 0, a: 0 }};
function linToDb(v) {{ return v < 1e-6 ? -60 : Math.max(-60, Math.min(0, 20 * Math.log10(v))); }}
function dbToPos(db) {{ return Math.max(0, Math.min(1, (db + 48) / 48)); }}
function setMeter(samples) {{
  let peak = 0;
  for (let i = 0; i < samples.length; i++) peak = Math.max(peak, Math.abs(samples[i]));
  if (peak >= hold.p) {{ hold.p = peak; hold.a = 0; }}
  else {{ hold.a += 0.04; if (hold.a > 0.9) hold.p *= 0.82; }}
  cover.style.width = ((1 - dbToPos(linToDb(peak))) * 100) + '%';
  holdEl.style.left = (dbToPos(linToDb(hold.p)) * 100) + '%';
}}
function fadeIn(samples) {{
  const frames = samples.length >> 1;
  const n = Math.min(frames, 960);
  for (let i = 0; i < n; i++) {{
    const t = i / Math.max(1, n - 1);
    const g = 0.5 - 0.5 * Math.cos(Math.PI * t);
    samples[i * 2] *= g;
    samples[i * 2 + 1] *= g;
  }}
}}
document.getElementById('go').onclick = async () => {{
  const ctx = new AudioContext();
  await ctx.resume();
  const gain = ctx.createGain();
  gain.connect(ctx.destination);
  const applyVol = () => {{
    const v = Number(vol.value);
    const now = ctx.currentTime;
    gain.gain.cancelScheduledValues(now);
    gain.gain.setTargetAtTime(v, now, 0.012);
    voln.textContent = v <= 0 ? '−∞ dB' : (20 * Math.log10(v)).toFixed(1) + ' dB';
  }};
  vol.oninput = applyVol;
  applyVol();
  await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([`
    class P extends AudioWorkletProcessor {{
      constructor() {{
        super();
        this.q = []; this.i = 0; this.lastL = 0; this.lastR = 0;
        this.rel = Math.exp(-1 / (0.012 * sampleRate));
        this.xfade = 0; this.xfadeN = Math.max(32, Math.round(0.008 * sampleRate));
        this.drops = 0; this.empty = false;
        this.port.onmessage = (e) => {{
          if (e.data.clear) {{ this.q = []; this.i = 0; this.xfade = 0; return; }}
          const empty = !this.q.length;
          this.q.push(e.data);
          if (empty) this.xfade = this.xfadeN;
        }};
      }}
      process(_, outputs) {{
        const L = outputs[0][0], R = outputs[0][1] || outputs[0][0];
        for (let i = 0; i < L.length; i++) {{
          while (this.q.length && this.i >= this.q[0].l.length) {{ this.q.shift(); this.i = 0; }}
          if (!this.q.length) {{
            if (!this.empty) {{ this.empty = true; this.drops++; this.port.postMessage({{drops: this.drops}}); }}
            this.lastL *= this.rel; this.lastR *= this.rel;
            L[i] = this.lastL; R[i] = this.lastR; continue;
          }}
          this.empty = false;
          let sL = this.q[0].l[this.i], sR = this.q[0].r[this.i];
          if (this.xfade > 0) {{
            const g = 1 - this.xfade / this.xfadeN;
            sL = this.lastL * (1 - g) + sL * g;
            sR = this.lastR * (1 - g) + sR * g;
            this.xfade--;
          }}
          L[i] = sL; R[i] = sR; this.lastL = sL; this.lastR = sR; this.i++;
        }}
        return true;
      }}
    }}
    registerProcessor('p', P);
  `], {{type:'text/javascript'}})));
  const node = new AudioWorkletNode(ctx, 'p', {{numberOfInputs:0, numberOfOutputs:1, outputChannelCount:[2]}});
  let drops = 0;
  let room = {{ host: true, live: false, listeners: 0, dropouts: 0 }};
  const show = () => {{
    const bits = [];
    if (room.live) bits.push('Live');
    else if (room.host) bits.push('Ready');
    else bits.push('Waiting');
    if (room.listeners) bits.push(room.listeners + ' listening');
    const n = Math.max(drops, Number(room.dropouts) || 0);
    if (n) bits.push(n + ' dropouts');
    who.textContent = bits.join(' · ');
  }};
  node.port.onmessage = (e) => {{ if (e.data && e.data.drops !== undefined) {{ drops = e.data.drops; show(); }} }};
  node.connect(gain);
  document.getElementById('gate').style.display = 'none';
  who.textContent = 'Waiting';
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  let last = -1;
  let resume = false;
  let retryMs = 400;
  const openOut = () => {{
    const ws = new WebSocket(proto + '://' + location.host + '/' + name + '/out');
    ws.binaryType = 'arraybuffer';
    ws.onopen = () => {{ retryMs = 400; room.host = true; show(); }};
    ws.onclose = () => {{
      resume = true;
      last = -1;
      room.live = false;
      room.host = false;
      who.textContent = 'Reconnecting';
      const wait = retryMs;
      retryMs = Math.min(retryMs * 2, 8000);
      setTimeout(openOut, wait);
    }};
    ws.onmessage = (ev) => {{
      if (typeof ev.data === 'string') {{
        try {{
          const m = JSON.parse(ev.data);
          if (m.t === 'dtx') {{ resume = true; room.live = false; who.textContent = 'Asleep'; }}
          if (m.t === 'go') {{ room.live = true; show(); }}
          if (m.t === 'room' || m.t === 'stat') {{
            room.host = m.host !== false;
            room.live = !!m.live;
            room.listeners = Number(m.listeners || m.lan || 0);
            room.dropouts = Number(m.dropouts) || room.dropouts;
            show();
          }}
        }} catch (e) {{}}
        return;
      }}
      room.live = true;
      const bytes = new Uint8Array(ev.data);
      if (bytes.byteLength < 8) return;
      const seq = new DataView(bytes.buffer, bytes.byteOffset, 8).getUint32(4, true);
      if (last >= 0 && seq <= last && last - seq < 32) return;
      last = seq;
      const n = (bytes.byteLength - 8) >> 1;
      const pcm = new Float32Array(n);
      const view = new DataView(bytes.buffer, bytes.byteOffset + 8);
      for (let i = 0; i < n; i++) pcm[i] = view.getInt16(i * 2, true) / 32767;
      if (resume) {{ fadeIn(pcm); resume = false; }}
      who.textContent = 'Live';
      setMeter(pcm);
      const frames = n >> 1;
      const l = new Float32Array(frames), r = new Float32Array(frames);
      for (let i = 0; i < frames; i++) {{ l[i] = pcm[i*2]; r[i] = pcm[i*2+1]; }}
      const src = 48000, dst = ctx.sampleRate;
      if (src === dst) {{ node.port.postMessage({{l, r}}); return; }}
      const outN = Math.max(1, Math.round(frames * dst / src));
      const ol = new Float32Array(outN), or = new Float32Array(outN);
      for (let i = 0; i < outN; i++) {{
        const p = i * src / dst, j = Math.min(frames - 1, p | 0), f = p - j, j2 = Math.min(frames - 1, j + 1);
        ol[i] = l[j] * (1 - f) + l[j2] * f;
        or[i] = r[j] * (1 - f) + r[j2] * f;
      }}
      node.port.postMessage({{l: ol, r: or}});
    }};
  }};
  openOut();
}};
</script></body></html>"#
    )
}
