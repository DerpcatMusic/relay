# Audio Lab Fix Disposition

## Scope

Final read-only acceptance audit of the `apps/audio-lab` fixes against
`docs/research/review-audio-lab.md`, covering H1, H2, M1, and M2. No source or
existing documentation was edited; this disposition is the only audit artifact.

## Current status

**COMPLETE — PASS.** All requested semantic checks and validation commands
passed.

## Finding disposition

### H1 — success-only diagnostics

**Resolved.** `worker_errors` and `clean_shutdown` are absent from both the
runtime diagnostics and the lab documentation. Playback publication/control
faults remain fatal errors with a nonzero exit rather than success constants.
The reported final lookahead drain is gated by an actual successful `rx.drain()`;
it is not an unconditional shutdown claim.

### H2 — effective-rate labelling

**Resolved.** Runtime fields are `configured_capture_rate_hz` and
`configured_playback_rate_hz`; human output says `configured nominal rates`.
The test rejects the old `effective rates` wording, and the documentation also
uses configured nominal rates.

### M1 — JSON validity and boundary/matrix proof

**Resolved.** Tests parse stdout with `serde_json::from_slice`, including the
signed `playback_error_frames` field. The 100 ms test executes all 48 cases
(4 capture rates × 4 playback rates × 3 packet durations) and asserts exact
input, encoded, rendered, playback-error, emitted, accepted, final-drain, drop,
underrun, and publication identities. Its frozen path expectations are:

- input frames: capture rate / 10;
- packet counts at 5/10/20 ms: 20/10/5 except 192 kHz capture, where the fixed
  capture SRC yields 19/9/4;
- playback error at 44.1/48/96/192 kHz: -2/-2/-4/-8 frames;
- rendered frames: encoded packets × packet duration × playback rate / 1000,
  plus the exact playback error.

The separate 50 ms test runs the same complete 48-case cross-product and
requires at least two encoded packets in every case, including 192 kHz capture
SRC cases. The exact 10,000 ms maximum is exercised and produces 480,000 input
frames, 500 encoded/emitted frames, 480,000 rendered frames, one drained
lookahead frame, and zero ring drops at the default 48 kHz / 20 ms clean case.
Out-of-range 10,010 ms is rejected.

### M2 — duplicate semantics

**Resolved.** Diagnostics distinguish `network_duplicate_requests`,
`network_duplicate_copies_scheduled`, and `rx_duplicate_rejections`. The seeded
500 ms impaired assertion observes exactly 1/1/1 respectively, alongside the
separate accepted-packet and drop counts.

## Validation evidence

All required commands passed:

- `cargo fmt --all -- --check`
- `cargo test --locked -p relay-audio-lab --all-targets` — 6/6 integration
  tests passed (debug)
- `cargo test --release --locked -p relay-audio-lab --all-targets` — 6/6
  integration tests passed
- `cargo clippy --locked -p relay-audio-lab --all-targets -- -D warnings`
- `cargo run --locked -p relay-audio-lab -- --json --duration-ms 10000
  --profile impaired --seed 7`

The impaired sample exited 0 and parsed as valid JSON. It reported 480,000
input/rendered frames, 500 encoded/emitted frames, 11 scheduler drops, one
duplicate request, one scheduled duplicate copy, one RX duplicate rejection,
11 FEC-or-PLC attempts, zero explicit PLC, zero ring drops/underruns, and zero
playback-frame error.

## Final disposition

**PASS.** H1, H2, M1, and M2 are resolved, and the locked debug/release,
strict-Clippy, formatting, and impaired-sample gates all pass.
