import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { forceOpusStereo } from "../src/opus-stereo.mjs";

const workerSrc = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../src/worker.ts"),
  "utf8",
);

const CHROME_ANSWER = [
  "v=0",
  "m=audio 9 UDP/TLS/RTP/SAVPF 111",
  "a=rtpmap:111 opus/48000/2",
  "a=fmtp:111 minptime=10;useinbandfec=1",
  "a=recvonly",
  "",
].join("\r\n");

test("Chrome answer without stereo=1 is munged so Opus decodes two channels", () => {
  const out = forceOpusStereo(CHROME_ANSWER);
  assert.match(out, /a=fmtp:111 .*stereo=1/);
  assert.match(out, /sprop-stereo=1/);
  assert.match(out, /minptime=10/);
});

test("stereo=0 in an answer is flipped to stereo=1", () => {
  const sdp = CHROME_ANSWER.replace(
    "a=fmtp:111 minptime=10;useinbandfec=1",
    "a=fmtp:111 minptime=10;useinbandfec=1;stereo=0;sprop-stereo=0",
  );
  const out = forceOpusStereo(sdp);
  assert.match(out, /stereo=1/);
  assert.match(out, /sprop-stereo=1/);
  assert.doesNotMatch(out, /stereo=0/);
});

test("opus rtpmap without fmtp gets a stereo fmtp line", () => {
  const sdp = [
    "m=audio 9 UDP/TLS/RTP/SAVPF 111",
    "a=rtpmap:111 opus/48000/2",
    "a=recvonly",
    "",
  ].join("\n");
  const out = forceOpusStereo(sdp);
  assert.match(out, /a=rtpmap:111 opus\/48000\/2\na=fmtp:111 .*stereo=1/);
});

test("listen page embeds the same stereo munge", () => {
  assert.match(workerSrc, /from "\.\/opus-stereo\.mjs"/);
  assert.match(workerSrc, /forceOpusStereo\.toString\(\)/);
});

test("non-opus fmtp is left alone", () => {
  const sdp = [
    "m=audio 9 UDP/TLS/RTP/SAVPF 111 126",
    "a=rtpmap:111 opus/48000/2",
    "a=fmtp:111 minptime=10;useinbandfec=1",
    "a=rtpmap:126 telephone-event/8000",
    "a=fmtp:126 0-15",
    "",
  ].join("\n");
  const out = forceOpusStereo(sdp);
  assert.match(out, /a=fmtp:126 0-15/);
  assert.match(out, /a=fmtp:111 .*stereo=1/);
});
