#!/usr/bin/env node
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require(
  "/home/derpcat/.npm/_npx/e41f203b7505f1fb/node_modules/playwright/index.js",
);
const url = process.argv[2] || "https://relay.matari-audio.com/room-ce368463";

const browser = await chromium.launch({
  headless: true,
  args: ["--autoplay-policy=no-user-gesture-required"],
});
const page = await browser.newPage();
const ws = [];
const statsMsgs = [];
page.on("websocket", (socket) => {
  socket.on("framereceived", (frame) => {
    const payload = String(frame.payload);
    if (payload.includes('"t":"stat"')) statsMsgs.push(payload.slice(0, 400));
    if (payload.includes('"sdp"') || payload.includes("m=audio")) {
      ws.push(payload.slice(0, 400));
    }
  });
});
await page.goto(url, { waitUntil: "domcontentloaded", timeout: 20000 });
await page.waitForFunction(() => window.relay?.ice === "connected" && window.relay?.pc, {
  timeout: 10000,
}).catch(() => {});
await page.waitForTimeout(3000);
await page.locator("#go").click({ force: true }).catch(() => {});
await page.waitForTimeout(3000);

const dump = await page.evaluate(async () => {
  const speaker = document.getElementById("spkr");
  const stream = speaker?.srcObject;
  const tracks = stream
    ? stream.getAudioTracks().map((t) => ({
        enabled: t.enabled,
        muted: t.muted,
        readyState: t.readyState,
        label: t.label,
      }))
    : [];
  const pc = window.relay?.pc;
  const rows = [];
  if (pc) {
    const stats = await pc.getStats();
    stats.forEach((r) => {
      if (
        r.type === "inbound-rtp" ||
        r.type === "remote-inbound-rtp" ||
        r.type === "media-source" ||
        r.type === "track" ||
        r.type === "codec" ||
        r.type === "transport" ||
        r.type === "candidate-pair"
      ) {
        rows.push(r);
      }
    });
  }
  const inbound = rows.filter((r) => r.type === "inbound-rtp");
  const pair = rows.filter((r) => r.type === "candidate-pair" && r.nominated);
  return {
    relay: {
      armed: window.relay?.armed,
      hooked: window.relay?.hooked,
      conn: window.relay?.conn,
      ice: window.relay?.ice,
      ctx: window.relay?.ctx,
      cover: window.relay?.cover,
    },
    who: document.getElementById("who")?.textContent,
    audio: {
      paused: speaker?.paused,
      muted: speaker?.muted,
      readyState: speaker?.readyState,
      volume: speaker?.volume,
    },
    tracks,
    inbound: inbound.map((r) => ({
      kind: r.kind,
      ssrc: r.ssrc,
      packetsReceived: r.packetsReceived,
      packetsLost: r.packetsLost,
      bytesReceived: r.bytesReceived,
      audioLevel: r.audioLevel,
      framesDecoded: r.framesDecoded,
      jitter: r.jitter,
      timestamp: r.timestamp,
      decoderImplementation: r.decoderImplementation,
      mimeType: r.mimeType,
      powerEfficientDecoder: r.powerEfficientDecoder,
    })),
    pair: pair.map((r) => ({
      state: r.state,
      nominated: r.nominated,
      bytesReceived: r.bytesReceived,
      bytesSent: r.bytesSent,
      currentRoundTripTime: r.currentRoundTripTime,
      availableOutgoingBitrate: r.availableOutgoingBitrate,
      localCandidateId: r.localCandidateId,
      remoteCandidateId: r.remoteCandidateId,
    })),
    codecs: rows.filter((r) => r.type === "codec").map((r) => ({
      mimeType: r.mimeType,
      clockRate: r.clockRate,
      channels: r.channels,
      payloadType: r.payloadType,
      sdpFmtpLine: r.sdpFmtpLine,
    })),
    transceivers: pc
      ? pc.getTransceivers().map((t) => ({
          mid: t.mid,
          dir: t.direction,
          current: t.currentDirection,
          stopped: t.stopped,
        }))
      : [],
  };
});

await browser.close();
console.log(JSON.stringify({ dump, offerSnippet: ws[0] ?? null, statsMsgs: statsMsgs.slice(-6) }, null, 2));
