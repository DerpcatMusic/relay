//! Local unpaid Link: `relay-link` then open `http://127.0.0.1:8787/<name>`.

use std::collections::BTreeMap;
use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use relay_audio::FrameDuration;
use relay_session::{
    ConnectionState, DEFAULT_LINK_HTTP_PORT, EngineCommand, MonitorMode, SessionConfig,
    SessionEngine, SessionMode, normalize_slug,
};

struct Registry {
    names: BTreeMap<String, SocketAddr>,
    joined: Option<SocketAddr>,
    last: Option<SocketAddr>,
}

struct Shared {
    registry: Mutex<Registry>,
    decoder: Mutex<SessionEngine>,
    pcm: Mutex<Vec<f32>>,
    report: Mutex<String>,
}

fn main() -> ExitCode {
    let http_port = env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_LINK_HTTP_PORT);
    let decoder = match SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x4c49_4e4b,
        monitor: MonitorMode::Remote,
        lan: true,
    }) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("prepare failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let shared = Arc::new(Shared {
        registry: Mutex::new(Registry {
            names: BTreeMap::new(),
            joined: None,
            last: None,
        }),
        decoder: Mutex::new(decoder),
        pcm: Mutex::new(vec![0.0; 960]),
        report: Mutex::new(String::new()),
    });
    let worker = Arc::clone(&shared);
    thread::spawn(move || decoder_loop(&worker));

    let bind = SocketAddr::from(([0, 0, 0, 0], http_port));
    let listener = match TcpListener::bind(bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("http bind {bind} failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("relay-link http://127.0.0.1:{http_port}/<session-name>");
    println!("claim: POST /api/claim  (name\\n127.0.0.1:17492\\n)");
    for incoming in listener.incoming() {
        let Ok(stream) = incoming else {
            continue;
        };
        let shared = Arc::clone(&shared);
        thread::spawn(move || handle_client(stream, &shared));
    }
    ExitCode::SUCCESS
}

fn decoder_loop(shared: &Shared) {
    let mut output = vec![0.0_f32; 960];
    loop {
        if let (Ok(mut registry), Ok(mut decoder)) = (shared.registry.lock(), shared.decoder.lock())
        {
            if let Some(target) = registry
                .last
                .or_else(|| registry.names.values().next().copied())
            {
                let state = decoder.snapshot().state;
                let needs_join = registry.joined != Some(target)
                    || matches!(
                        state,
                        ConnectionState::Idle | ConnectionState::Failed | ConnectionState::Closed
                    );
                if needs_join && decoder.apply(EngineCommand::Join(target)).is_ok() {
                    registry.joined = Some(target);
                }
            }
            let report = decoder.drive();
            let rendered = decoder.render(&mut output, &[]);
            if rendered.rendered_samples > 0
                && let Ok(mut pcm) = shared.pcm.lock()
            {
                pcm.clone_from(&output);
            }
            if let Ok(mut slot) = shared.report.lock() {
                *slot = format!("drive={report:?} render={rendered:?}");
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn handle_client(mut stream: TcpStream, shared: &Shared) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0_u8; 4096];
    let Ok(read) = stream.read(&mut buf) else {
        return;
    };
    let request = String::from_utf8_lossy(&buf[..read]);
    let mut lines = request.lines();
    let Some(first) = lines.next() else {
        return;
    };
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    match (method, path) {
        ("POST", "/api/claim") => {
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let mut name = String::new();
            let mut target = String::new();
            for (index, line) in body.lines().enumerate() {
                if index == 0 {
                    name = normalize_slug(line);
                }
                if index == 1 {
                    target = line.trim().to_string();
                }
            }
            let ok = parse_addr(&target).is_some() && !name.is_empty();
            if ok
                && let (Some(addr), Ok(mut registry)) =
                    (parse_addr(&target), shared.registry.lock())
            {
                registry.names.insert(name, addr);
                registry.last = Some(addr);
            }
            write_response(
                &mut stream,
                200,
                "text/plain",
                if ok { b"ok" } else { b"bad" },
            );
        }
        ("GET", "/") => {
            let listing = match shared.registry.lock() {
                Ok(registry) => registry
                    .names
                    .iter()
                    .map(|(name, addr)| format!("<li><a href=\"/{name}\">{name}</a> → {addr}</li>"))
                    .collect::<Vec<_>>()
                    .join(""),
                Err(_) => String::new(),
            };
            let html = index_html(&listing);
            write_response(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                html.as_bytes(),
            );
        }
        ("GET", "/health") => write_response(&mut stream, 200, "text/plain", b"ok"),
        ("GET", "/status") => {
            let names = shared
                .registry
                .lock()
                .map(|registry| {
                    registry
                        .names
                        .iter()
                        .map(|(name, addr)| format!("{name}={addr}"))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            let snap = shared.decoder.lock().map(|decoder| decoder.snapshot()).ok();
            let peak = shared
                .pcm
                .lock()
                .map(|pcm| {
                    pcm.iter()
                        .fold(0.0_f32, |acc, sample| acc.max(sample.abs()))
                })
                .unwrap_or(0.0);
            let report = shared.report.lock().ok().map(|guard| guard.clone());
            let body = format!("names={names}\nsnap={snap:?}\npeak={peak}\nreport={report:?}\n");
            write_response(&mut stream, 200, "text/plain", body.as_bytes());
        }
        (method, path) if method == "GET" && path.ends_with("/pcm") => {
            let slug = path.trim_start_matches('/').trim_end_matches("/pcm");
            if !session_known(shared, slug) {
                write_response(&mut stream, 404, "text/plain", b"unknown session");
                return;
            }
            stream_pcm(&mut stream, shared);
        }
        ("GET", path) => {
            let slug = normalize_slug(path.trim_start_matches('/'));
            if slug.is_empty() {
                write_response(&mut stream, 404, "text/plain", b"missing name");
                return;
            }
            write_response(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                player_html(&slug).as_bytes(),
            );
        }
        _ => write_response(&mut stream, 405, "text/plain", b"method"),
    }
}

fn session_known(shared: &Shared, slug: &str) -> bool {
    let name = normalize_slug(slug);
    shared
        .registry
        .lock()
        .map(|registry| registry.names.contains_key(&name))
        .unwrap_or(false)
}

fn stream_pcm(stream: &mut TcpStream, shared: &Shared) {
    let header = b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
    if stream.write_all(header).is_err() {
        return;
    }
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut encoded = vec![0_u8; 960 * 2];
    loop {
        let pcm = shared.pcm.lock().ok().map(|guard| guard.clone());
        let Some(pcm) = pcm else {
            break;
        };
        if pcm.len() * 2 > encoded.len() {
            encoded.resize(pcm.len() * 2, 0);
        }
        for (index, sample) in pcm.iter().enumerate() {
            let quant = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
            encoded[index * 2..index * 2 + 2].copy_from_slice(&quant.to_le_bytes());
        }
        if stream.write_all(&encoded[..pcm.len() * 2]).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn parse_addr(value: &str) -> Option<SocketAddr> {
    value.to_socket_addrs().ok()?.next()
}

fn index_html(listing: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>RELAY</title>
<style>body{{font:16px/1.4 system-ui;background:#111;color:#eee;margin:2rem}}a{{color:#8cf}}</style>
</head><body><h1>RELAY listen</h1><p>Open a session name from the plugin.</p><ul>{listing}</ul></body></html>"#
    )
}

fn player_html(name: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>RELAY · {name}</title>
<style>body{{font:16px/1.4 system-ui;background:#111;color:#eee;margin:2rem;max-width:36rem}}
button{{font:inherit;padding:.6rem 1rem;border:0;border-radius:8px;background:#3af;color:#111;cursor:pointer}}
#st{{margin-top:1rem;opacity:.8}}</style></head>
<body><h1>{name}</h1><p>Browser listen for a RELAY plugin session. Click once — browsers block autoplay.</p>
<button id="go">Listen</button><div id="st">idle</div>
<script>
const name = {name:?};
const st = (t) => document.getElementById('st').textContent = t;
document.getElementById('go').onclick = async () => {{
  const ctx = new AudioContext({{sampleRate: 48000}});
  await ctx.resume();
  const proc = ctx.createScriptProcessor(2048, 0, 2);
  let q = new Float32Array(0);
  proc.onaudioprocess = (ev) => {{
    const L = ev.outputBuffer.getChannelData(0);
    const R = ev.outputBuffer.getChannelData(1);
    for (let i = 0; i < L.length; i++) {{
      L[i] = q[i*2] || 0;
      R[i] = q[i*2+1] || 0;
    }}
    q = q.length > L.length*2 ? q.subarray(L.length*2) : new Float32Array(0);
  }};
  proc.connect(ctx.destination);
  st('connecting');
  let res = null;
  for (let n = 0; n < 30; n++) {{
    res = await fetch('/' + name + '/pcm');
    if (res.ok && res.body) break;
    st('waiting for session…');
    await new Promise((r) => setTimeout(r, 1000));
  }}
  if (!res || !res.ok || !res.body) {{ st('unavailable — start the plugin + Link'); return; }}
  st('playing');
  const reader = res.body.getReader();
  let pending = new Uint8Array(0);
  while (true) {{
    const {{value, done}} = await reader.read();
    if (done) break;
    const next = new Uint8Array(pending.length + value.length);
    next.set(pending); next.set(value, pending.length);
    pending = next;
    const even = pending.length - (pending.length % 2);
    const view = new DataView(pending.buffer, pending.byteOffset, even);
    const extra = new Float32Array(even / 2);
    for (let i = 0; i < extra.length; i++) extra[i] = view.getInt16(i*2, true) / 32767;
    const merged = new Float32Array(q.length + extra.length);
    merged.set(q); merged.set(extra, q.length);
    q = merged.length > 48000 * 4 ? merged.subarray(merged.length - 48000 * 2) : merged;
    pending = pending.subarray(even);
  }}
  st('ended');
}};
</script></body></html>"#
    )
}
