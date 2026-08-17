import { DurableObject } from "cloudflare:workers";

export interface Env {
  ROOM: DurableObjectNamespace<SessionRoom>;
}

type Claim = {
  name: string;
  port: number;
  lan: string[];
  lanHttp: number;
  mode: string;
  codec: string;
  rate: number;
  deviceRate: number;
  block: number;
  bitrate: number;
  bits: number;
  compression: number;
  pass: string;
  at: number;
};

const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET,POST,OPTIONS",
  "access-control-allow-headers": "content-type",
};

export class SessionRoom extends DurableObject {
  seq = 0;
  lastAt = 0;
  lastBytes = 0;
  locked = false;
  silent = false;
  claim: Claim | null = null;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.ctx.blockConcurrencyWhile(async () => {
      this.claim = (await this.ctx.storage.get<Claim>("claim")) ?? null;
      this.locked = Boolean(this.claim?.pass);
    });
  }

  async fetch(request: Request): Promise<Response> {
    try {
      return await this.handle(request);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return new Response(`room: ${message}`, { status: 500, headers: CORS });
    }
  }

  private async handle(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") {
      return new Response(null, { headers: CORS });
    }
    if (request.headers.get("Upgrade") === "websocket") {
      const pair = new WebSocketPair();
      const server = pair[1];
      const tag = url.pathname.endsWith("/in") ? "in" : "out";
      this.ctx.acceptWebSocket(server, [tag]);
      if (tag === "out") {
        server.serializeAttachment({ ok: !this.locked || (await this.isOpen()) });
      }
      this.broadcastText(await this.roomEvent());
      return new Response(null, { status: 101, webSocket: pair[0] });
    }
    if (request.method === "POST" && url.pathname.endsWith("/claim")) {
      const body = (await request.json()) as Partial<Claim>;
      const claim: Claim = {
        name: String(body.name ?? "").toLowerCase().replace(/[^a-z0-9-]/g, "").slice(0, 48),
        port: Number(body.port) || 17492,
        lan: Array.isArray(body.lan) ? body.lan.map(String).slice(0, 8) : [],
        lanHttp: Number(body.lanHttp) || 8787,
        mode: String(body.mode ?? "opus"),
        codec: String(body.codec ?? body.mode ?? "opus"),
        rate: Number(body.rate) || 48000,
        deviceRate: Number(body.deviceRate) || Number(body.rate) || 48000,
        block: Number(body.block) || 0,
        bitrate: Number(body.bitrate) || 192,
        bits: Number(body.bits) || 16,
        compression: Number(body.compression) || 0,
        pass: String(body.pass ?? "").toLowerCase().replace(/[^a-f0-9]/g, "").slice(0, 64),
        at: Date.now(),
      };
      this.locked = Boolean(claim.pass);
      this.claim = claim;
      await this.ctx.storage.put("claim", claim);
      this.broadcastText(await this.roomEvent());
      return Response.json({ ok: true, locked: this.locked }, { headers: CORS });
    }
    return gone();
  }

  async webSocketMessage(ws: WebSocket, message: ArrayBuffer | string): Promise<void> {
    const tags = this.ctx.getTags(ws);
    if (typeof message === "string") {
      if (tags.includes("in") && (await this.handleCtrl(message))) {
        return;
      }
      if (tags.includes("out") && (await this.checkPassword(message))) {
        ws.serializeAttachment({ ok: true });
        this.broadcastText(await this.roomEvent());
      }
      return;
    }
    if (!tags.includes("in")) {
      return;
    }
    this.silent = false;
    this.fanout(message);
  }

  private isLive(): boolean {
    return this.lastAt > 0 && Date.now() - this.lastAt < 800;
  }

  private broadcastText(text: string): void {
    for (const peer of this.ctx.getWebSockets()) {
      try {
        peer.send(text);
      } catch {
        /* peer gone */
      }
    }
  }

  private async handleCtrl(raw: string): Promise<boolean> {
    let msg: { t?: string; codec?: string; bitrate?: number; bits?: number; compression?: number; rate?: number; deviceRate?: number; block?: number };
    try {
      msg = JSON.parse(raw) as typeof msg;
    } catch {
      return false;
    }
    if (!msg.t) {
      return false;
    }
    if (msg.t === "dtx") {
      if (this.silent) {
        return true;
      }
      this.silent = true;
    } else if (msg.t === "go") {
      if (!this.silent) {
        return true;
      }
      this.silent = false;
    } else if (msg.t === "cfg") {
      await this.mergeClaim(msg);
    } else {
      return false;
    }
    this.broadcastText(await this.roomEvent());
    return true;
  }

  private async mergeClaim(msg: {
    codec?: string;
    bitrate?: number;
    bits?: number;
    compression?: number;
    rate?: number;
    deviceRate?: number;
    block?: number;
  }): Promise<void> {
    const claim = this.claim;
    if (!claim) {
      return;
    }
    if (msg.codec) {
      claim.codec = String(msg.codec);
      claim.mode = claim.codec;
    }
    if (msg.bitrate !== undefined) {
      claim.bitrate = Number(msg.bitrate) || claim.bitrate;
    }
    if (msg.bits !== undefined) {
      claim.bits = Number(msg.bits) || claim.bits;
    }
    if (msg.compression !== undefined) {
      claim.compression = Number(msg.compression) || 0;
    }
    if (msg.rate !== undefined) {
      claim.rate = Number(msg.rate) || claim.rate;
    }
    if (msg.deviceRate !== undefined) {
      claim.deviceRate = Number(msg.deviceRate) || claim.deviceRate;
    }
    if (msg.block !== undefined) {
      claim.block = Number(msg.block) || 0;
    }
    this.claim = claim;
    await this.ctx.storage.put("claim", claim);
  }

  async webSocketClose(ws: WebSocket, code: number, reason: string): Promise<void> {
    try {
      ws.close(code || 1000, reason);
    } catch {
      /* already closed */
    }
    this.broadcastText(await this.roomEvent());
  }

  private async roomEvent(): Promise<string> {
    const counts = this.listenerCounts();
    return JSON.stringify({
      t: "room",
      claim: await this.publicClaim(),
      listeners: counts.listeners,
      waiting: counts.waiting,
      silent: this.silent,
      live: this.isLive(),
    });
  }

  private async isOpen(): Promise<boolean> {
    return !this.claim?.pass;
  }

  private async checkPassword(password: string): Promise<boolean> {
    if (!this.claim?.pass) {
      return true;
    }
    return (await sha256hex(password.trim())) === this.claim.pass;
  }

  private async publicClaim(): Promise<(Omit<Claim, "pass"> & { locked: boolean }) | null> {
    const claim = this.claim;
    if (!claim) {
      return null;
    }
    const { pass, ...rest } = claim;
    return { ...rest, locked: Boolean(pass) };
  }

  private listenerCounts(): { listeners: number; waiting: number } {
    let listeners = 0;
    let waiting = 0;
    for (const peer of this.ctx.getWebSockets("out")) {
      if (this.outOk(peer)) {
        listeners += 1;
      } else {
        waiting += 1;
      }
    }
    return { listeners, waiting };
  }

  private fanout(message: ArrayBuffer): void {
    const framed = wrapFrame(message, this.seq + 1);
    this.seq = framed.seq;
    this.lastBytes = framed.pcmBytes;
    this.lastAt = Date.now();
    for (const peer of this.ctx.getWebSockets("out")) {
      if (!this.outOk(peer)) {
        continue;
      }
      try {
        peer.send(framed.bytes);
      } catch {
        /* listener gone */
      }
    }
  }

  private outOk(peer: WebSocket): boolean {
    const att = peer.deserializeAttachment() as { ok?: boolean } | null;
    if (att?.ok) {
      return true;
    }
    if (!this.locked && att?.ok !== false) {
      if (!att) {
        try {
          peer.serializeAttachment({ ok: true });
        } catch {
          /* hibernation attachment optional */
        }
      }
      return true;
    }
    return false;
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") {
      return new Response(null, { headers: CORS });
    }
    if (url.pathname === "/" || url.pathname === "/health") {
      return html(indexHtml());
    }
    if (url.pathname === "/api/claim" && request.method === "POST") {
      const body = (await request.json()) as { name?: string; port?: number; lan?: string[]; mode?: string };
      const slug = slugify(body.name ?? "");
      if (!slug) {
        return new Response("bad name", { status: 400, headers: CORS });
      }
      try {
        return await env.ROOM.getByName(slug).fetch(
          new Request("https://room/claim", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ ...body, name: slug }),
          }),
        );
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        return new Response(`claim: ${message}`, { status: 500, headers: CORS });
      }
    }
    const parts = url.pathname.split("/").filter(Boolean);
    const slug = slugify(parts[0] ?? "");
    if (!slug) {
      return new Response("missing name", { status: 404 });
    }
    if (parts[1] === "in" || parts[1] === "out") {
      if (request.headers.get("Upgrade") !== "websocket") {
        return new Response("expected websocket", { status: 426, headers: CORS });
      }
      return env.ROOM.getByName(slug).fetch(request);
    }
    if (parts[1] === "info" || parts[1] === "pcm" || parts[1] === "ctrl" || parts[1] === "unlock") {
      return gone();
    }
    return html(playerHtml(slug));
  },
};

function slugify(raw: string): string {
  return raw.toLowerCase().replace(/[^a-z0-9-]/g, "").slice(0, 48);
}

function html(body: string): Response {
  return new Response(body, {
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function gone(): Response {
  return new Response("gone — connect over websocket", {
    status: 410,
    headers: {
      ...CORS,
      "cache-control": "public, max-age=86400",
    },
  });
}

async function sha256hex(value: string): Promise<string> {
  const bytes = new TextEncoder().encode(value);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function isFramed(bytes: Uint8Array): boolean {
  return bytes.byteLength >= 5
    && bytes[0] === 0x52
    && bytes[1] === 0x4c
    && bytes[2] === 0x59
    && (bytes[3] === 0x31 || bytes[3] === 0x42 || bytes[3] === 0x4f);
}

function wrapFrame(message: ArrayBuffer, nextSeq: number): { bytes: ArrayBuffer; seq: number; pcmBytes: number } {
  const src = new Uint8Array(message);
  if (isFramed(src)) {
    const seq = new DataView(src.buffer, src.byteOffset, src.byteLength).getUint32(4, true);
    return { bytes: src.slice().buffer, seq, pcmBytes: src.byteLength - 8 };
  }
  const seq = nextSeq >>> 0;
  const out = new Uint8Array(8 + src.byteLength);
  out[0] = 0x52;
  out[1] = 0x4c;
  out[2] = 0x59;
  out[3] = 0x31;
  new DataView(out.buffer).setUint32(4, seq, true);
  out.set(src, 8);
  return { bytes: out.buffer, seq, pcmBytes: src.byteLength };
}

function indexHtml(): string {
  return listenPage("session", true);
}

function playerHtml(name: string): string {
  return listenPage(name, false);
}

const WORKLET_SRC = `
class RelayPlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.n = 96000;
    this.l = new Float32Array(this.n);
    this.r = new Float32Array(this.n);
    this.wr = 0; this.rd = 0; this.filled = 0;
    this.min = Math.round(0.02 * sampleRate);
    this.target = Math.round(0.04 * sampleRate);
    this.max = Math.round(0.32 * sampleRate);
    this.primed = false;
    this.lastL = 0; this.lastR = 0;
    this.drops = 0;
    this.port.onmessage = (ev) => {
      if (ev.data && ev.data.clear) {
        this.wr = 0; this.rd = 0; this.filled = 0; this.primed = false;
        return;
      }
      if (ev.data && ev.data.target) {
        this.target = Math.max(this.min, Math.min(this.max, ev.data.target | 0));
        return;
      }
      const L = ev.data.l;
      const R = ev.data.r;
      for (let i = 0; i < L.length; i++) {
        this.l[this.wr] = L[i];
        this.r[this.wr] = R[i];
        this.wr = (this.wr + 1) % this.n;
        if (this.filled < this.n) this.filled++;
        else this.rd = this.wr;
      }
      if (this.filled > this.max) {
        const keep = this.target;
        const skip = this.filled - keep;
        this.rd = (this.rd + skip) % this.n;
        this.filled = keep;
      }
      if (this.filled >= this.target) this.primed = true;
    };
  }
  process(_inputs, outputs) {
    const oL = outputs[0][0];
    const oR = outputs[0][1] || oL;
    const frames = oL.length;
    if (!this.primed || this.filled < frames) {
      for (let i = 0; i < frames; i++) {
        this.lastL *= 0.97;
        this.lastR *= 0.97;
        oL[i] = this.lastL;
        oR[i] = this.lastR;
      }
      if (this.primed) {
        this.drops++;
        this.target = Math.min(this.max, this.target + frames);
      }
      return true;
    }
    for (let i = 0; i < frames; i++) {
      oL[i] = this.l[this.rd];
      oR[i] = this.r[this.rd];
      this.lastL = oL[i];
      this.lastR = oR[i];
      this.rd = (this.rd + 1) % this.n;
    }
    this.filled -= frames;
    if (this.filled > this.target * 3 && this.target > this.min) {
      this.target -= Math.round(0.001 * sampleRate);
    }
    return true;
  }
}
registerProcessor('relay-player', RelayPlayer);
`;

function listenPage(name: string, landing: boolean): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${landing ? "RELAY" : `RELAY · ${name}`}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Barlow:wght@500;600;700&display=swap" rel="stylesheet">
<style>
:root{--bg:#191919;--lane:#252525;--surface:#353535;--text:#fff;--muted:#b8b8b8;--accent:#00aaff;--ice:#25e7ff;--ok:#5be8b3;--warn:#ffc75c;--hot:#ff7088}
*{box-sizing:border-box}html,body{margin:0;min-height:100%;background:var(--bg);color:var(--text);font-family:Barlow,system-ui,sans-serif}
::selection{background:var(--accent);color:#041018}
:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
::-webkit-scrollbar{width:10px;height:10px}
::-webkit-scrollbar-thumb{background:var(--surface);border-radius:8px}
body{display:flex;justify-content:center;padding:28px 20px 48px}
.wrap{width:min(360px,100%)}
.nav{display:flex;align-items:center;gap:10px;height:36px;margin:0 0 28px}
.mark{width:28px;height:17px;display:block;flex:none}
.product{font-size:13px;font-weight:700;letter-spacing:.16em;color:var(--text)}
h1{font-size:22px;line-height:1.15;letter-spacing:-.02em;margin:0 0 6px;font-weight:600}
.who{margin:0 0 22px;color:var(--muted);font-size:13px;min-height:1.2em}
.desk{display:flex;align-items:stretch;gap:18px;height:148px}
.meter{flex:1;display:flex;align-items:center}
.strip{position:relative;width:100%;height:28px;border-radius:4px;overflow:hidden;background:linear-gradient(90deg,#3d8f6a 0%,#5be8b3 42%,#ffc75c 78%,#ff7088 100%)}
.cover{position:absolute;top:0;right:0;bottom:0;width:100%;background:var(--lane)}
.hold{position:absolute;top:0;bottom:0;left:0;width:2px;background:#fff}
.vol{display:flex;flex-direction:column;align-items:center;justify-content:space-between;gap:8px;width:28px;flex:none}
.vol input{appearance:slider-vertical;writing-mode:vertical-lr;direction:rtl;width:22px;flex:1;margin:0;accent-color:var(--accent);background:transparent}
.vol span{font-size:12px;color:var(--muted);font-variant-numeric:tabular-nums;text-align:center}
.gate{position:fixed;inset:0;display:none;place-items:center;background:#191919f2;z-index:20}
.gate.show{display:grid}
.gate button{font:inherit;font-weight:700;font-size:16px;border:0;border-radius:8px;padding:12px 22px;background:var(--accent);color:#041018;cursor:pointer}
.lock{display:none;margin:0 0 18px}
.lock.show{display:block}
.lock label{display:block;font-size:12px;color:var(--muted);margin:0 0 6px}
.lock input{width:100%;font:inherit;font-size:15px;border:0;border-radius:6px;padding:10px 12px;background:var(--lane);color:var(--text);caret-color:var(--accent);outline:2px solid transparent}
.lock input:focus{outline-color:var(--accent)}
.lock button{margin-top:10px;font:inherit;font-weight:700;border:0;border-radius:6px;padding:8px 14px;background:var(--accent);color:#041018;cursor:pointer}
.lock .err{color:var(--hot);font-size:13px;margin-top:8px;min-height:1.2em}
</style>
</head>
<body>
<main class="wrap">
  <header class="nav">
    <svg class="mark" viewBox="0 0 2408 1488" aria-hidden="true"><path fill="#fff" d="M99.021 1486.974c-53.202.013-96.36-43.07-96.439-96.273C2.179 1122.129 1.048 366.891.645 97.561.606 71.902 10.797 47.286 28.962 29.162 47.126 11.039 71.765.904 97.424 1c69.143.26 159.026.597 212.615.799 29.175.109 56.732 13.423 74.949 36.212 86.302 107.957 344.363 430.771 467.706 585.064 17.097 21.388 42.482 34.497 69.819 36.057 27.337 1.559 54.048-8.578 73.466-27.882 155.807-154.885 504.705-501.721 606.179-602.595 18.054-17.947 42.472-28.026 67.928-28.038 44.951-.022 118.986-.057 173.583-.084 41.56-.02 78.456 26.593 91.552 66.036 75.96 228.788 328.628 989.806 429.472 1293.541 9.759 29.393 4.804 61.685-13.319 86.8-18.123 25.115-47.207 39.995-78.178 39.998-256.183.023-811.251.072-1038.748.093-16.809.001-31.963-10.123-38.396-25.652-6.433-15.529-2.878-33.404 9.007-45.29 59.457-59.456 141.243-141.242 183.755-183.754 18.081-18.081 42.604-28.24 68.174-28.24 56.829-.002 163.744-.005 250.405-.008 30.989-.001 60.088-14.896 78.21-40.034 18.121-25.138 23.056-57.454 13.262-86.855-34.745-104.305-84.02-252.224-120.519-361.795-10.545-31.654-36.704-55.606-69.162-63.328-32.459-7.721-66.602 1.887-90.27 25.402-148.441 147.483-408.034 405.401-530.914 527.487-37.693 37.45-98.58 37.346-136.145-.232-66.869-66.892-171.554-171.613-260.274-260.363-27.571-27.58-69.041-35.836-105.073-20.918-36.031 14.919-59.528 50.074-59.532 89.072-.016 128.53-.034 281.266-.046 378.02-.006 53.237-43.158 96.393-96.394 96.406-69.26.016-162.244.039-231.485.055Z"/></svg>
    <span class="product">RELAY</span>
  </header>
  <h1 id="title">${landing ? "Listen" : name}</h1>
  <p class="who" id="who">${landing ? "Open a session from the plugin." : "Waiting"}</p>
  <form class="lock" id="lock" ${landing ? "hidden" : ""}>
    <label for="pw">Password</label>
    <input id="pw" type="password" autocomplete="current-password" />
    <button type="submit">Unlock</button>
    <p class="err" id="pwerr"></p>
  </form>
  <div class="desk" ${landing ? "hidden" : ""}>
    <div class="meter"><div class="strip" aria-label="Level"><div class="cover" id="cover"></div><div class="hold" id="hold"></div></div></div>
    <label class="vol">
      <input id="vol" type="range" min="0" max="2" step="0.01" value="1" orient="vertical" aria-label="Volume">
      <span id="voln">0 dB</span>
    </label>
  </div>
</main>
<div class="gate${landing ? "" : " show"}" id="gate"><div><button id="go" type="button">Listen</button></div></div>
${landing ? "" : `<script>
const name = ${JSON.stringify(name)};
function listenTargetSec(claim) {
  const device = Number(claim && claim.deviceRate) || Number(claim && claim.rate) || 48000;
  const block = Number(claim && claim.block) || 0;
  const dawMs = block > 0 ? (block * 1000 / device) : 20;
  return Math.min(0.16, Math.max(0.04, (dawMs * 2 + 20) / 1000));
}
function linToDb(v) {
  return v < 1e-6 ? -60 : Math.max(-60, Math.min(0, 20 * Math.log10(v)));
}
function dbToPos(db) {
  return Math.max(0, Math.min(1, (db + 48) / 48));
}
const cover = document.getElementById('cover');
const holdEl = document.getElementById('hold');
const who = document.getElementById('who');
const vol = document.getElementById('vol');
const voln = document.getElementById('voln');
const gate = document.getElementById('gate');
let ctx = null;
let gainNode = null;
let lastSeq = -1;
let rate = 48000;
const hold = { p: 0, a: 0 };
const pending = [];
function parseFrame(buf) {
  const bytes = new Uint8Array(buf);
  if (bytes.byteLength >= 8 && bytes[0] === 0x52 && bytes[1] === 0x4c && bytes[2] === 0x59 && bytes[3] === 0x31) {
    const seq = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(4, true);
    return [{ kind: 'pcm', seq, pcm: bytes.buffer.slice(bytes.byteOffset + 8, bytes.byteOffset + bytes.byteLength) }];
  }
  if (bytes.byteLength >= 5 && bytes[0] === 0x52 && bytes[1] === 0x4c && bytes[2] === 0x59 && bytes[3] === 0x42) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const parts = [];
    let off = 5;
    const n = bytes[4];
    for (let i = 0; i < n && off + 9 <= bytes.byteLength; i++) {
      const kind = bytes[off] === 2 ? 'opus' : 'pcm';
      const seq = view.getUint32(off + 1, true);
      const len = view.getUint32(off + 5, true);
      off += 9;
      if (off + len > bytes.byteLength) break;
      const payload = bytes.buffer.slice(bytes.byteOffset + off, bytes.byteOffset + off + len);
      parts.push({ kind, seq, pcm: payload });
      off += len;
    }
    return parts;
  }
  return [{ kind: 'pcm', seq: lastSeq + 1, pcm: buf }];
}
function decodeI16(buf) {
  const view = new DataView(buf);
  const n = view.byteLength >> 1;
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = view.getInt16(i * 2, true) / 32767;
  return out;
}
let opusDec = null;
let opusTs = 0;
let opusReady = false;
function resetOpus() {
  if (opusDec) try { opusDec.close(); } catch (e) {}
  opusDec = null;
  opusTs = 0;
  opusReady = false;
}
function armOpus() {
  if (opusDec || typeof AudioDecoder === 'undefined') return;
  try {
    opusDec = new AudioDecoder({
      output: (audio) => {
        const frames = audio.numberOfFrames;
        const l = new Float32Array(frames);
        const r = new Float32Array(frames);
        audio.copyTo(l, { planeIndex: 0, format: 'f32-planar' });
        audio.copyTo(r, { planeIndex: audio.numberOfChannels > 1 ? 1 : 0, format: 'f32-planar' });
        audio.close();
        const interleaved = new Float32Array(frames * 2);
        for (let i = 0; i < frames; i++) {
          interleaved[i * 2] = l[i];
          interleaved[i * 2 + 1] = r[i];
        }
        pushSamples(interleaved, 48000);
      },
      error: () => { resetOpus(); }
    });
    opusDec.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 2 });
    opusReady = true;
  } catch (e) {
    resetOpus();
  }
}
function decodeOpus(packet) {
  armOpus();
  if (!opusDec || !opusReady) return;
  try {
    opusDec.decode(new EncodedAudioChunk({
      type: 'key',
      timestamp: opusTs,
      duration: 20_000,
      data: packet
    }));
    opusTs += 20_000;
  } catch (e) {
    resetOpus();
  }
}
function setMeter(samples) {
  let peak = 0;
  for (let i = 0; i < samples.length; i++) peak = Math.max(peak, Math.abs(samples[i]));
  if (peak >= hold.p) { hold.p = peak; hold.a = 0; }
  else {
    hold.a += 0.04;
    if (hold.a > 0.9) hold.p *= 0.82;
  }
  cover.style.width = ((1 - dbToPos(linToDb(peak))) * 100) + '%';
  holdEl.style.left = (dbToPos(linToDb(hold.p)) * 100) + '%';
}
let node = null;
let targetSec = 0.04;
let socket = null;
let dtxMode = false;
let lastCfg = '';
let secret = '';
function resamplePlanar(interleaved, srcRate, dstRate) {
  const srcFrames = interleaved.length >> 1;
  if (!srcFrames) return { l: new Float32Array(0), r: new Float32Array(0) };
  const dstFrames = Math.max(1, Math.round(srcFrames * dstRate / srcRate));
  const l = new Float32Array(dstFrames);
  const r = new Float32Array(dstFrames);
  for (let i = 0; i < dstFrames; i++) {
    const srcPos = i * srcRate / dstRate;
    const j = Math.min(srcFrames - 1, srcPos | 0);
    const f = srcPos - j;
    const j2 = Math.min(srcFrames - 1, j + 1);
    l[i] = interleaved[j * 2] * (1 - f) + interleaved[j2 * 2] * f;
    r[i] = interleaved[j * 2 + 1] * (1 - f) + interleaved[j2 * 2 + 1] * f;
  }
  return { l, r };
}
function takeSeq(seq) {
  if (lastSeq >= 0 && seq <= lastSeq) {
    if (lastSeq - seq > 32) lastSeq = seq;
    else return false;
  } else {
    lastSeq = seq;
  }
  return true;
}
function fadeIn(samples) {
  const n = Math.min(samples.length, Math.round(0.03 * rate * 2));
  for (let i = 0; i < n; i++) {
    const t = i / Math.max(1, n - 1);
    samples[i] *= 0.5 - 0.5 * Math.cos(Math.PI * t);
  }
}
function pushSamples(samples, srcRate) {
  setMeter(samples);
  if (!ctx || !node) {
    pending.push(samples);
    if (pending.length > 24) pending.shift();
    return;
  }
  node.port.postMessage(resamplePlanar(samples, srcRate, ctx.sampleRate));
}
function enqueue(buf, srcRate, resume) {
  let first = !!resume;
  for (const frame of parseFrame(buf)) {
    if (!takeSeq(frame.seq)) continue;
    if (frame.kind === 'opus') {
      decodeOpus(frame.pcm, srcRate);
      first = false;
      continue;
    }
    const samples = decodeI16(frame.pcm);
    if (first) { fadeIn(samples); first = false; }
    pushSamples(samples, srcRate);
  }
}
function same24(a, b) {
  const pa = String(a).split('.');
  const pb = String(b).split('.');
  return pa.length === 4 && pb.length === 4 && pa[0] === pb[0] && pa[1] === pb[1] && pa[2] === pb[2];
}
function gatherHostIps() {
  return new Promise((resolve) => {
    const ips = [];
    const pc = new RTCPeerConnection({ iceServers: [] });
    try { pc.createDataChannel('lan'); } catch (e) {}
    const finish = () => { try { pc.close(); } catch (e) {} resolve(ips); };
    const t = setTimeout(finish, 450);
    pc.onicecandidate = (e) => {
      if (!e || !e.candidate) { clearTimeout(t); finish(); return; }
      const m = String(e.candidate.candidate || '').match(/(\d+\.\d+\.\d+\.\d+)/);
      if (m && m[1].indexOf('127.') !== 0) ips.push(m[1]);
    };
    pc.createOffer().then((o) => pc.setLocalDescription(o)).catch(finish);
  });
}
let lanTried = false;
async function maybeLan(claim) {
  if (lanTried || !claim || !claim.lan || !claim.lan.length) return;
  lanTried = true;
  const mine = await gatherHostIps();
  const port = Number(claim.lanHttp) || 8787;
  for (let i = 0; i < claim.lan.length; i++) {
    const ip = claim.lan[i];
    for (let j = 0; j < mine.length; j++) {
      if (same24(ip, mine[j])) {
        if (socket) try { socket.close(); } catch (e) {}
        location.replace('http://' + ip + ':' + port + '/' + name);
        return;
      }
    }
  }
}
function flush(srcRate) {
  if (!ctx || !node) return;
  while (pending.length) {
    node.port.postMessage(resamplePlanar(pending.shift(), srcRate, ctx.sampleRate));
  }
}
const WORKLET = ${JSON.stringify(WORKLET_SRC)};
function resetListen(claim) {
  lastSeq = -1;
  pending.length = 0;
  resetOpus();
  if (node) node.port.postMessage({ clear: 1 });
  if (claim) applyClaim(claim);
}
function applyClaim(claim) {
  if (!claim || !claim.name) return;
  rate = Number(claim.rate) || 48000;
  targetSec = listenTargetSec(claim);
  if (node && ctx) node.port.postMessage({ target: Math.round(targetSec * ctx.sampleRate) });
}
function onCtrl(raw) {
  let msg;
  try { msg = JSON.parse(raw); } catch (e) { return; }
  if (msg.t === 'room') {
    who.textContent = msg.silent ? 'Silent' : msg.live ? 'Live' : 'Waiting';
    if (msg.silent) dtxMode = true;
    if (msg.claim) {
      maybeLan(msg.claim);
      const key = [msg.claim.codec, msg.claim.bitrate, msg.claim.bits, msg.claim.compression, msg.claim.rate].join('|');
      if (lastCfg && lastCfg !== key) resetListen(msg.claim);
      else applyClaim(msg.claim);
      lastCfg = key;
    }
    const lock = document.getElementById('lock');
    if (msg.claim && msg.claim.locked && lock && !secret) {
      lock.classList.add('show');
      who.textContent = 'Password required';
    }
    return;
  }
  if (msg.t === 'dtx') {
    dtxMode = true;
    who.textContent = 'Silent';
    return;
  }
  if (msg.t === 'go') {
    dtxMode = false;
    who.textContent = 'Live';
    return;
  }
  if (msg.t === 'cfg') {
    const key = [msg.codec, msg.bitrate, msg.bits, msg.compression, msg.rate].join('|');
    applyClaim(Object.assign({}, { name: name }, msg));
    if (lastCfg && lastCfg !== key) resetListen(msg);
    lastCfg = key;
  }
}
async function armAudio() {
  if (ctx && ctx.state === 'running') return true;
  if (!ctx) {
    ctx = new AudioContext();
    gainNode = ctx.createGain();
    gainNode.connect(ctx.destination);
    const url = URL.createObjectURL(new Blob([WORKLET], { type: 'text/javascript' }));
    await ctx.audioWorklet.addModule(url);
    URL.revokeObjectURL(url);
    node = new AudioWorkletNode(ctx, 'relay-player', { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2] });
    node.port.postMessage({ target: Math.round(targetSec * ctx.sampleRate) });
    node.connect(gainNode);
    const applyVol = () => {
      const v = Number(vol.value);
      gainNode.gain.value = v;
      voln.textContent = v <= 0 ? '−∞ dB' : (20 * Math.log10(v)).toFixed(1) + ' dB';
    };
    vol.oninput = applyVol;
    applyVol();
  }
  if (ctx.state === 'suspended') await ctx.resume();
  if (ctx.state !== 'running') return false;
  gate.classList.remove('show');
  flush(rate);
  return true;
}
async function start() {
  gate.classList.add('show');
  who.textContent = 'Waiting';
  const lock = document.getElementById('lock');
  if (lock) {
    lock.onsubmit = (ev) => {
      ev.preventDefault();
      const pw = document.getElementById('pw').value;
      secret = pw;
      if (socket && socket.readyState === 1) socket.send(pw);
    };
  }
  const unlock = async (ev) => {
    ev.preventDefault();
    if (await armAudio()) {
      window.removeEventListener('pointerdown', unlock);
    }
  };
  window.addEventListener('pointerdown', unlock);
  document.getElementById('go').onclick = unlock;
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  let retryMs = 1000;
  const openSocket = () => {
    const ws = new WebSocket(proto + '://' + location.host + '/' + name + '/out');
    socket = ws;
    ws.binaryType = 'arraybuffer';
    ws.onopen = () => {
      retryMs = 1000;
      if (secret) ws.send(secret);
    };
    ws.onclose = () => {
      const wait = retryMs;
      retryMs = Math.min(retryMs * 2, 30000);
      setTimeout(openSocket, wait);
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === 'string') { onCtrl(ev.data); return; }
      const resume = dtxMode;
      dtxMode = false;
      enqueue(ev.data, rate, resume);
    };
  };
  openSocket();
}
start();
</script>`}
</body></html>`;
}
