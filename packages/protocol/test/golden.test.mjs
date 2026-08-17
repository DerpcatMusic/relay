import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { fromBinary, toBinary } from "@bufbuild/protobuf";
import { EnvelopeSchema } from "../dist/generated/relay/v1/signaling_pb.js";

const fixtureUrl = new URL(
  "../../../tests/fixtures/protocol/hello-resume-v1.bin",
  import.meta.url,
);

test("TypeScript decodes and re-encodes the Hello/Resume golden fixture", async () => {
  const golden = new Uint8Array(await readFile(fixtureUrl));
  const envelope = fromBinary(EnvelopeSchema, golden);

  assert.equal(envelope.version?.major, 1);
  assert.equal(envelope.revision, 42n);
  assert.equal(envelope.payload.case, "hello");
  assert.equal(envelope.payload.value?.entry.case, "resume");
  assert.equal(envelope.payload.value?.entry.value?.lastSeenRevision, 41n);
  assert.deepEqual(toBinary(EnvelopeSchema, envelope), golden);
});
