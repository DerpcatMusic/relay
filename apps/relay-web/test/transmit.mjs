#!/usr/bin/env node
// Event-path smoke: one claim, one /in, one /out. No /info /pcm /ctrl polls.

const origin = process.env.RELAY_ORIGIN ?? "https://relay.matari-audio.com";
const slug = `tx-${Math.floor(Date.now() / 1000)}`;

function sineFrame(seq, frames = 1920, rate = 48000) {
  const pcm = new Int16Array(frames * 2);
  for (let i = 0; i < frames; i++) {
    const t = (seq * frames + i) / rate;
    const s = Math.round(Math.sin(2 * Math.PI * 440 * t) * 16000);
    pcm[i * 2] = s;
    pcm[i * 2 + 1] = s;
  }
  const out = new Uint8Array(8 + pcm.byteLength);
  out[0] = 0x52;
  out[1] = 0x4c;
  out[2] = 0x59;
  out[3] = 0x31;
  new DataView(out.buffer).setUint32(4, seq, true);
  out.set(new Uint8Array(pcm.buffer), 8);
  return out;
}

function parseFrame(buf) {
  const bytes = new Uint8Array(buf);
  if (bytes.byteLength >= 8 && bytes[0] === 0x52 && bytes[1] === 0x4c && bytes[2] === 0x59 && bytes[3] === 0x31) {
    return {
      seq: new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(4, true),
      pcmBytes: bytes.byteLength - 8,
    };
  }
  return { seq: -1, pcmBytes: bytes.byteLength };
}

function peakFrom(buf) {
  const view = new DataView(buf);
  const start = view.byteLength >= 8 && view.getUint32(0, true) === 0x31594c52 ? 8 : 0;
  let peak = 0;
  for (let i = start; i + 1 < view.byteLength; i += 2) {
    peak = Math.max(peak, Math.abs(view.getInt16(i, true)));
  }
  return peak / 32767;
}

function openSocket(name, tag) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`${origin.replace("https", "wss")}/${name}/${tag}`);
    ws.binaryType = "arraybuffer";
    const frames = [];
    const rooms = [];
    const timer = setTimeout(() => reject(new Error(`${tag} open timeout`)), 8000);
    ws.addEventListener("open", () => {
      clearTimeout(timer);
      resolve({
        ws,
        frames,
        rooms,
        wait(n, ms = 4000) {
          return new Promise((done, fail) => {
            const start = Date.now();
            const tick = () => {
              if (frames.length >= n) return done(frames.slice(0, n));
              if (Date.now() - start > ms) return fail(new Error(`got ${frames.length}/${n}`));
              setTimeout(tick, 20);
            };
            tick();
          });
        },
        waitListeners(n, ms = 4000) {
          return new Promise((done, fail) => {
            const start = Date.now();
            const tick = () => {
              const last = rooms.at(-1);
              if (last && Number(last.listeners) === n) return done(last);
              if (Date.now() - start > ms) return fail(new Error(`listeners ${last?.listeners ?? "none"} != ${n}`));
              setTimeout(tick, 20);
            };
            tick();
          });
        },
      });
    });
    ws.addEventListener("error", () => reject(new Error(`${tag} error`)));
    ws.addEventListener("message", (ev) => {
      if (typeof ev.data === "string") {
        try {
          const msg = JSON.parse(ev.data);
          if (msg.t === "room") rooms.push(msg);
        } catch {
          /* ignore */
        }
        return;
      }
      const parsed = parseFrame(ev.data);
      frames.push({
        at: Date.now(),
        seq: parsed.seq,
        bytes: ev.data.byteLength,
        pcmBytes: parsed.pcmBytes,
        peak: peakFrom(ev.data),
      });
    });
  });
}

async function main() {
  const report = { slug, origin, ok: true, notes: [] };
  const claim = await fetch(`${origin}/api/claim`, {
    method: "POST",
    headers: { "content-type": "application/json", "user-agent": "Mozilla/5.0 RELAY/0.1" },
    body: JSON.stringify({ name: slug, port: 17492, codec: "opus", mode: "opus", rate: 48000, bitrate: 192 }),
  });
  report.claim = claim.status;
  if (claim.status !== 200) throw new Error(`claim ${claim.status}`);

  const inn = await openSocket(slug, "in");
  const a = await openSocket(slug, "out");
  const b = await openSocket(slug, "out");
  const room = await a.waitListeners(2);
  report.listenersAfterTwo = room.listeners;
  if (room.listeners < 2) {
    report.ok = false;
    report.notes.push(`expected 2 listeners, got ${room.listeners}`);
  }

  for (let i = 1; i <= 12; i++) {
    inn.ws.send(sineFrame(i));
    await new Promise((r) => setTimeout(r, 40));
  }
  const gotA = await a.wait(12, 5000);
  const gotB = await b.wait(12, 5000);
  report.steady = {
    a: gotA.map((f) => ({ seq: f.seq, pcm: f.pcmBytes, peak: Number(f.peak.toFixed(3)) })),
    gapsA: gotA.slice(1).map((f, i) => f.at - gotA[i].at),
  };
  if (gotA.length !== 12 || gotB.length !== 12) {
    report.ok = false;
    report.notes.push(`steady fanout a=${gotA.length} b=${gotB.length}`);
  }
  if (gotA.some((f) => f.peak < 0.2)) {
    report.ok = false;
    report.notes.push("steady frames looked silent");
  }

  a.ws.close();
  await b.waitListeners(1);
  report.listenersAfterClose = b.rooms.at(-1)?.listeners;
  b.ws.close();
  inn.ws.close();

  console.log(JSON.stringify(report, null, 2));
  if (!report.ok) process.exit(1);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
