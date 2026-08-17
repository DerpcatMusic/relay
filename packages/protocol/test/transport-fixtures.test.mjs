import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { fromBinary, toBinary } from "@bufbuild/protobuf";
import {
  EnvelopeSchema,
  PeerUpdateKind,
} from "../dist/generated/relay/v1/signaling_pb.js";

const fixtureRoot = new URL("../../../tests/fixtures/transport/", import.meta.url);

const fixtures = [
  ["browser-offer-v1.bin", "offer"],
  ["native-answer-v1.bin", "answer"],
  ["native-offer-v1.bin", "offer"],
  ["browser-answer-v1.bin", "answer"],
  ["browser-trickle-candidate-v1.bin", "iceCandidate"],
  ["native-trickle-candidate-v1.bin", "iceCandidate"],
  ["browser-end-of-candidates-v1.bin", "iceCandidate"],
  ["native-end-of-candidates-v1.bin", "iceCandidate"],
  ["peer-left-v1.bin", "peerUpdate"],
  ["resume-request-v1.bin", "hello"],
  ["resume-accepted-v1.bin", "welcome"],
  ["browser-ice-restart-offer-v1.bin", "offer"],
  ["native-ice-restart-answer-v1.bin", "answer"],
  ["native-ice-restart-offer-v1.bin", "offer"],
  ["browser-ice-restart-answer-v1.bin", "answer"],
];

async function decodeFixture(name) {
  const bytes = new Uint8Array(await readFile(new URL(`v1/${name}`, fixtureRoot)));
  return { bytes, envelope: fromBinary(EnvelopeSchema, bytes) };
}

test("TypeScript decodes and re-encodes all transport V1 fixtures", async () => {
  for (const [name, payloadCase] of fixtures) {
    const { bytes, envelope } = await decodeFixture(name);

    assert.equal(envelope.version?.major, 1, name);
    assert.equal(envelope.sessionId, "transport-fixture-session-v1", name);
    assert.equal(envelope.payload.case, payloadCase, name);
    assert.deepEqual(toBinary(EnvelopeSchema, envelope), bytes, name);
  }
});

test("candidate, resume, peer-left, and ICE-restart semantics remain frozen", async () => {
  for (const name of [
    "browser-trickle-candidate-v1.bin",
    "native-trickle-candidate-v1.bin",
  ]) {
    const { envelope } = await decodeFixture(name);
    assert.equal(envelope.payload.case, "iceCandidate", name);
    assert.notEqual(envelope.payload.value.candidate, "", name);
    assert.equal(envelope.payload.value.endOfCandidates, false, name);
    assert.equal(envelope.payload.value.sdpMid, "data", name);
  }

  for (const name of [
    "browser-end-of-candidates-v1.bin",
    "native-end-of-candidates-v1.bin",
  ]) {
    const { envelope } = await decodeFixture(name);
    assert.equal(envelope.payload.case, "iceCandidate", name);
    assert.equal(envelope.payload.value.candidate, "", name);
    assert.equal(envelope.payload.value.endOfCandidates, true, name);
  }

  const { envelope: peerLeft } = await decodeFixture("peer-left-v1.bin");
  assert.equal(peerLeft.payload.case, "peerUpdate");
  assert.equal(peerLeft.payload.value.kind, PeerUpdateKind.LEFT);

  const { envelope: resumeRequest } = await decodeFixture("resume-request-v1.bin");
  assert.equal(resumeRequest.payload.case, "hello");
  assert.equal(resumeRequest.payload.value.entry.case, "resume");

  const { envelope: resumeAccepted } = await decodeFixture("resume-accepted-v1.bin");
  assert.equal(resumeAccepted.payload.case, "welcome");
  assert.equal(resumeAccepted.payload.value.recovery.case, "resumeAccepted");

  const restartPairs = [
    ["browser-offer-v1.bin", "browser-ice-restart-offer-v1.bin", "browser"],
    ["native-offer-v1.bin", "native-ice-restart-offer-v1.bin", "native"],
  ];
  for (const [baselineName, restartName, peer] of restartPairs) {
    const { envelope: baseline } = await decodeFixture(baselineName);
    const { envelope: restart } = await decodeFixture(restartName);
    assert.equal(baseline.payload.case, "offer", baselineName);
    assert.equal(restart.payload.case, "offer", restartName);
    assert.match(baseline.payload.value.sdp, new RegExp(`a=ice-ufrag:${peer}-base-v1`));
    assert.match(restart.payload.value.sdp, new RegExp(`a=ice-ufrag:${peer}-restart-v1`));
  }
});


test("transport fixture SHA-256 inventory matches the frozen corpus", async () => {
  const inventory = await readFile(new URL("SHA256SUMS", fixtureRoot), "utf8");
  const entries = inventory.trim().split("\n");

  assert.equal(entries.length, fixtures.length);
  for (const entry of entries) {
    const [expected, relativePath] = entry.split(/  /u);
    const bytes = await readFile(new URL(relativePath, fixtureRoot));
    assert.equal(createHash("sha256").update(bytes).digest("hex"), expected, relativePath);
  }
});

test("environment and scorecard templates parse and freeze the T0 rubric", async () => {
  const environment = JSON.parse(
    await readFile(new URL("environment-manifest-v1.template.json", fixtureRoot), "utf8"),
  );
  const scorecard = JSON.parse(
    await readFile(new URL("scorecard-v1.template.json", fixtureRoot), "utf8"),
  );

  assert.equal(environment.schemaVersion, "relay.transport.environment-manifest.v1");
  assert.equal(environment.network.publicStunTurnServices, false);
  assert.equal(environment.retryPolicy.reportFirstAttemptSeparately, true);
  assert.deepEqual(
    environment.targets.map(({ os, architecture }) => `${os}/${architecture}`),
    ["windows/x86_64", "macos/arm64", "macos/x86_64", "linux/x86_64"],
  );

  assert.equal(scorecard.schemaVersion, "relay.transport.scorecard.v1");
  assert.deepEqual(
    scorecard.hardGates.map(({ id, status }) => [id, status]),
    [
      ["adapter_fit", "not_run"],
      ["browser_interop", "not_run"],
      ["relay_security", "not_run"],
      ["recovery_lifecycle", "not_run"],
      ["licensing", "not_run"],
      ["packaging", "not_run"],
      ["maintenance", "not_run"],
    ],
  );
  assert.equal(
    scorecard.weightedDimensions.reduce((sum, dimension) => sum + dimension.weight, 0),
    100,
  );
  assert.ok(scorecard.weightedDimensions.every(({ rating }) => rating === null));
  assert.equal(scorecard.decision.eligibleForWeightedComparison, false);
});
