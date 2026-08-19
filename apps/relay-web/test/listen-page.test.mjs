import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/worker.ts"),
  "utf8",
);

test("listen click never awaits empty audio.play", () => {
  assert.equal(src.includes("await speaker.play"), false);
  assert.match(src, /speaker\.play\(\)\.catch/);
});

test("unlock hides the gate before any await", () => {
  const unlock = src.slice(src.indexOf("const unlock"), src.indexOf("armAudio();") + 12);
  assert.match(unlock, /gate\.classList\.remove\('show'\)/);
  assert.match(unlock, /speaker\.play\(\)\.catch/);
  assert.doesNotMatch(unlock, /await /);
});

test("offer is answered without waiting for Listen", () => {
  assert.match(src, /pendingOffer = msg\.sdp;\s*acceptOffer\(msg\.sdp\)/);
  assert.doesNotMatch(src, /if \(ctx\) acceptOffer/);
});

test("page advertises listen revision 9", () => {
  assert.match(src, /name="relay-listen" content="9"/);
});

test("meters are flat GYR rails without analog LED notches", () => {
  assert.equal(src.includes('class="leds"'), false);
  assert.equal(src.includes('class="tics"'), false);
  assert.equal(src.includes("repeating-linear-gradient"), false);
  assert.match(src, /\.rail\{[^}]*width:8px/);
});

test("empty firefox end-of-candidates is not sent to the host", () => {
  assert.match(src, /!ev\.candidate\.candidate/);
});

test("listen page meters from plugin peak stats", () => {
  assert.match(src, /msg\.peak/);
  assert.match(src, /setMeterPeak\(msg\.peak\)/);
});

test("disconnected is not treated as a fatal connection failure", () => {
  assert.equal(src.includes("Connection failed"), false);
  assert.match(src, /connectionState === 'disconnected'/);
  assert.match(src, /requestOffer\('ice failed'\)/);
  assert.match(src, /requestOffer\('ice stuck'\)/);
  assert.doesNotMatch(src, /iceConnectionState === 'disconnected'\) bounceSocket/);
});

test("missing offer asks the host again without closing signaling", () => {
  assert.match(src, /function requestOffer/);
  assert.match(src, /function armOfferWatch/);
  assert.match(src, /t: 'want'/);
  assert.match(src, /sendWant\('no offer'\)/);
  assert.match(src, /4000/);
  assert.match(src, /armOfferWatch\(\)/);
});

test("listen unmutes the audio element after the user gesture", () => {
  assert.match(src, /speaker\.muted = !armed/);
  assert.match(src, /speaker\.muted = false/);
});

test("on-page logs never print secrets", () => {
  assert.match(src, /function log\(/);
  assert.match(src, /function iceKind\(/);
  assert.match(src, /function sdpSummary\(/);
  assert.doesNotMatch(src, /log\(msg\.sdp\)/);
  assert.doesNotMatch(src, /log\(msg\.cand\)/);
  assert.doesNotMatch(src, /ice-pwd/);
  assert.match(src, /sdpSummary\(msg\.sdp\)/);
  assert.match(src, /iceKind\(msg\.cand\)/);
});

test("listen desk is a stereo channel strip", () => {
  assert.match(src, /aria-label="Left"/);
  assert.match(src, /aria-label="Right"/);
  assert.match(src, /id="coverL"/);
  assert.match(src, /id="coverR"/);
  assert.match(src, /createChannelSplitter/);
});

test("session title types a new room and redirects", () => {
  assert.match(src, /location\.assign\('\/' \+ slug\)/);
  assert.match(src, /id="title"/);
  assert.match(src, /class="title"/);
});

test("diagnostics live in an expandable tape, not a polite live region", () => {
  assert.match(src, /<details class="tape">/);
  assert.match(src, /id="tapeLine"/);
  assert.match(src, /role="status" aria-live="polite"/);
  assert.match(src, /id="log" aria-hidden="true"/);
});

test("listen gate is a dialog over the strip", () => {
  assert.match(src, /role="dialog"/);
  assert.match(src, /aria-modal="true"/);
});

test("volume is a hardware fader between L and R, not a native range chrome", () => {
  assert.match(src, /id="throw"/);
  assert.match(src, /id="cap"/);
  assert.match(src, /id="mute"/);
  assert.match(src, /coverL[\s\S]*id="vol"[\s\S]*coverR/);
});

test("session name field explains how to jump rooms", () => {
  assert.match(src, /id="titleHint"/);
  assert.match(src, /Type another name and press Enter to jump/);
});

test("Chrome Opus answers are munged to stereo=1 before setLocalDescription", () => {
  assert.match(src, /forceOpusStereo\.toString\(\)/);
  assert.match(src, /answer\.sdp = forceOpusStereo\(answer\.sdp\)/);
  assert.match(src, /setRemoteDescription\(\{ type: 'offer', sdp: forceOpusStereo\(sdp\) \}\)/);
});

test("Chromium and mobile skip the Web Audio tap that steals the element", () => {
  assert.match(src, /function tapIsUnsafe/);
  assert.match(src, /if \(tapIsUnsafe\(\)\) return/);
});
