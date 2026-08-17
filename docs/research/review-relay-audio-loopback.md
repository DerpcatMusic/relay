# Review: relay-audio loopback tests

## Scope

Independent review limited to:

- `crates/relay-audio/tests/loopback.rs`
- `playback::tests::scheduled_remote_drift_produces_the_correct_output_input_sign` in `crates/relay-audio/src/playback.rs` (there is no `crates/relay-audio/tests/playback.rs`)
- `docs/research/relay-audio-loopback-tests.md`

No Rust was edited. The audit covered claimed stage execution, RTP increments and scheduled-only drift, FEC/PLC honesty, deterministic reorder/loss, wrap, final drain, supported rate/duration coverage, non-vacuous assertions, callback/ring lifecycle, and runtime.

## Critical findings

None.

## High findings

### H1 — The claimed final RX lookahead drain is called but not verified

**Evidence:** `loopback.rs:243-259` performs exactly `encoded_packets` calls to the one-decision-lookahead `tick()`, conditionally consuming returned frames. `loopback.rs:260-262` also consumes `drain()` only under `if let Some(...)`. There is no assertion that `drain()` returned the final frame, that `sources.len() == encoded_packets`, that `rx_metrics.emitted_frames == encoded_packets`, or that the last emitted sequence/timestamp is the final encoded position. `loopback.rs:264` only proves that already-published playback samples were consumed from the ring. Nevertheless, `relay-audio-loopback-tests.md:7,14` states that the focused test covers/emits the final RX lookahead position with `drain()`.

**Impact:** A regression making `drain()` return `None`, or silently losing the final staged position, can leave all three integration tests passing. The nominal repeat will reproduce the same truncation; the fault test reaches its FEC/PLC assertions before the final position; and the 64-packet drift cases can warm the estimator without the last frame.

**Required correction:** Assert the expected emitted count and final sequence/timestamp, and require the final `drain()` outcome rather than accepting `None`.

### H2 — RX ingress accepts every rejection class, so valid-packet regressions can be concealed

**Evidence:** `loopback.rs:248-254` accepts `IngressStatus::Rejected(_)` without restricting it to the deliberately duplicated packet. The later fault assertions at `loopback.rs:339-352` require only that at least one FEC, PLC, duplicate, and reordered event occurred; they do not require identity/timestamp/duration/malformed/late/ahead rejection counters to remain zero. The drift test at `loopback.rs:363-394` does not assert zero PLC/rejections either.

**Impact:** Some correctly formed, scheduled packets may be rejected for an unintended reason while the loss-shaped test still produces the required FEC/PLC categories, or while a long drift run still produces a sign estimate. That weakens the document's claim at `relay-audio-loopback-tests.md:5-7` that the real RX path and valid timeline are fully exercised.

**Required correction:** Match the exact expected duplicate rejection and require all other ingress packets to be accepted in-order/reordered as appropriate; assert all unrelated RX rejection metrics are zero.

## Medium findings

### M1 — The fault test does not bind FEC and PLC to the intended missing sequence positions

**Evidence:** The deterministic topology drops indices 4, 7, and 8 (`loopback.rs:90-109`). The standalone gap at 4 and the second position of the two-packet hole at 8 can both be followed by a real packet, while position 7 is the explicit-PLC case. But `LoopResult` retains only a source list, not `(sequence, source)` (`loopback.rs:26-37,216-224`), and the test checks only `contains(...)`, `fec_attempts >= 1`, and `plc_frames >= 1` (`loopback.rs:339-350`).

**Impact:** The standalone FEC-targeted position could regress to PLC while a later FEC attempt at position 8 keeps the test green. Thus FEC/PLC naming is honest—`InbandFecOrPlc` does not claim LBRR recovery, consistent with `relay-audio-loopback-tests.md:11`—but the specifically described impairment roles are not proven.

**Required correction:** Retain emitted sequence with each source and assert the intended source per gap (plus exact expected operation counts), while continuing to call the FEC result `InbandFecOrPlc` rather than “recovered.”

### M2 — The documented strict-Clippy disposition is not reproducible as a package all-target gate

**Evidence:** `relay-audio-loopback-tests.md:33` refers to a coherent package/workspace strict-Clippy gate elsewhere. The focused loopback target passes `cargo clippy --locked -p relay-audio --test loopback -- -D warnings`, and the library target passes `cargo clippy --locked -p relay-audio --lib -- -D warnings`. However, `cargo clippy --locked -p relay-audio --all-targets -- -D warnings` fails on dead fields `device_rate_hz`, `packet_ms`, and `drift_ppm` in `crates/relay-audio/tests/virtual_hours.rs:14-16`. Cargo cannot lint the corrected `#[cfg(test)]` playback unit in isolation without selecting test targets, so the current strict package test gate is blocked outside this review's Rust scope.

**Impact:** The reviewed targets compile and test, but the documentation should not imply a currently clean coherent strict gate unless the exact passing command and disposition are available.

**Required correction:** Record the exact scoped commands honestly, or repair the separate all-target failure in its own scope before claiming the package/workspace gate.

## Confirmed evidence

- **All claimed functional stages are invoked:** capture PCM enters `TxWorker` and real packets are collected (`loopback.rs:150-179`), actions are scheduled on virtual `NetworkTime` (`181-197,241-255`), RX outcomes feed scheduled playback (`216-239`), and each published chunk is rendered immediately.
- **RTP increments remain valid:** `scheduled_position` derives the media delta solely from extended sequence and negotiated duration and asserts the wrapped wire timestamp (`loopback.rs:118-140`). The corrected playback unit keeps media positions at `packet * 480` while changing only scheduled local frames to `packet * 479` (`playback.rs:754-764`). The integration drift mapping likewise changes only scheduled local-device position (`loopback.rs:129-140,363-394`).
- **Drift signs are scheduled-only:** network actions do not depend on `scheduled_drift_ppm`; positive, zero, and negative mappings are applied only when feeding playback, with fill gains disabled (`loopback.rs:58-64,90-115,217-230,363-394`).
- **FEC/PLC terminology is honest:** the test asserts `FrameSource::InbandFecOrPlc` and an operation count, not proof of LBRR presence (`loopback.rs:339-348`; document line 11).
- **Reorder/loss is deterministic:** fixed action indices create duplicate, reordered delivery, and three drops (`loopback.rs:90-115`); exact simulated-drop and duplicate-request metrics plus observed reorder/duplicate counters are checked (`347-352`).
- **Wrap is reached:** the chosen sequence starts four positions before 16-bit wrap and the timestamp starts 700 ticks before 32-bit wrap (`loopback.rs:16-17`); the fault test must progress through the later PLC gap and also checks the configured end crosses both wraps (`339-359`).
- **Supported coverage is accurate:** cases at `loopback.rs:311-316` collectively include every supported capture and playback rate (44.1/48/96/192 kHz) and every duration (5/10/20 ms), pairwise rather than Cartesian.
- **Audio assertions are nontrivial:** finite rendering, complete render sizes, stereo energy, channel difference, zero ring drops, and nonzero observed occupancy are checked (`loopback.rs:228-238,280-304`).
- **Ring/renderer lifecycle is bounded in this single-threaded harness:** every published chunk is immediately rendered; renderer availability is checked and ends at zero (`loopback.rs:225-264`). This validates the producer/consumer API path, not an OS audio callback thread, which the evidence document does not claim.
- **Runtime is short after build:** warm debug loopback completes in 0.88 s and release in 0.03 s; the corrected playback unit completes in 0.06 s debug and under 0.01 s release.

## Validation

Run from the repository root:

| Command | Result |
|---|---|
| `cargo test --locked -p relay-audio --test loopback` | PASS, 3/3; test runtime 0.88 s (0.91 s command) |
| `cargo test --locked -p relay-audio scheduled_remote_drift_produces_the_correct_output_input_sign --lib` | PASS, 1/1; test runtime 0.06 s (0.09 s command) |
| `cargo test --locked --release -p relay-audio --test loopback` | PASS, 3/3; test runtime 0.03 s (0.25 s command including incremental compile) |
| `cargo test --locked --release -p relay-audio scheduled_remote_drift_produces_the_correct_output_input_sign --lib` | PASS, 1/1; under 0.01 s test runtime (0.03 s command) |
| `cargo clippy --locked -p relay-audio --test loopback -- -D warnings` | PASS |
| `cargo clippy --locked -p relay-audio --lib -- -D warnings` | PASS (does not include `#[cfg(test)]` unit code) |
| `cargo clippy --locked -p relay-audio --all-targets -- -D warnings` | **FAIL**: unrelated `tests/virtual_hours.rs:14-16` dead fields |
