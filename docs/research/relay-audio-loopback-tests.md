# RELAY audio-loop test evidence

## Decision

Add three short integration tests in `crates/relay-audio/tests/loopback.rs` and keep production Rust unchanged. The tests drive the real fixed capture SRC, canonical Opus encoder/decoder, deterministic network, RX reorder/deadline core, scheduled-playout adaptive SRC, bounded playback ring, and renderer. They use no sleep, thread, wall clock, or unsafe code.

Coverage is pairwise rather than Cartesian: all 5/10/20 ms durations and all 44.1/48/96/192 kHz capture and playback rates occur, while only one nominal case is repeated byte-for-byte. A focused 20 ms case adds duplicate, delay/reorder, one FEC-targeted drop, a two-packet hole for explicit PLC, wire sequence/timestamp wrap, final RX lookahead drain, ring bounds, and metrics. A third test preserves valid fixed-duration RTP timestamp increments while expressing +400, zero, and -400 ppm solely in the scheduled local-device-frame mapping; through real RX outcomes it asserts the estimator and output/input correction signs.

## Potential corrections captured by the tests

- `FrameSource::InbandFecOrPlc` is intentionally the honest assertion for a real Opus FEC request. It proves that RX invoked `decode_fec` using the following valid packet, **not** that libopus exposed proof that LBRR data was present. The test separately asserts `fec_attempts`; it does not rename this to “FEC recovered.”
- Virtual `NetworkTime` controls delivery only. Playback receives an extended media position reconstructed from the scheduled extended sequence plus the epoch timestamp; network arrival time is never converted into media position.
- Remote drift never changes the negotiated RTP increment. The sign test keeps each 10 ms media step at exactly 480 RTP ticks and maps that valid timeline onto rounded scheduled local-device-frame positions. Positive remote drift produces negative output/input correction; negative drift produces positive correction; zero drift remains zero when fill gains are disabled for isolation.
- `RxWorker::tick` has one-decision lookahead. The final scheduled position is required from `drain()` rather than conditionally ignored or produced by inventing another missing deadline; emitted count and final sequence/timestamp are exact.
- Every valid ingress rejection other than the one deliberate duplicate fails the test; unrelated rejection counters must remain zero. Intended FEC-or-PLC/PLC outcomes are bound to their exact extended sequences.
- Playback publication is all-or-drop. Each published chunk is rendered immediately; tests assert the observed queue never exceeds configured scalar capacity and that dropped-ring samples remain zero.

## Exact evidence (existing primary sources)

1. [`crates/relay-audio/src/tx.rs`](../../crates/relay-audio/src/tx.rs): `TxWorker::process_capture` validates a fixed capture chunk, uses the configured converter, drains the 48 kHz accumulator into fixed-duration Opus packets, and makes batch backpressure explicit.
2. [`crates/relay-audio/src/rx.rs`](../../crates/relay-audio/src/rx.rs): `RxWorker::tick` stages one deadline and resolves the previous one; `resolve` selects normal decode, following-packet `decode_fec`, or explicit PLC; `drain` resolves only the final staged position. `RxMetrics::fec_attempts` explicitly says it is an operation count, not proof of LBRR presence.
3. [`crates/relay-audio/src/network.rs`](../../crates/relay-audio/src/network.rs): `DeterministicNetwork` orders fixed-capacity scheduled copies by virtual delivery time and stable insertion ordinal; `NetworkAction` is limited to deliver/drop/duplicate/delay and contains no clock source.
4. [`crates/relay-audio/src/playback.rs`](../../crates/relay-audio/src/playback.rs): `PlaybackWorker::process_frame` accepts `ExtendedTimestamp` and scheduled local device frame, constructs a scheduled-playout observation, adaptively converts, and publishes one complete chunk to the bounded ring; `PlaybackRenderer::render` performs bounded copy/zero-fill only.

## Validation

From the repository root after the independent playback review correction:

```text
cargo test --locked -p relay-audio --test loopback
PASS: 3/3 tests, including valid_rx_timeline_drives_both_scheduled_drift_correction_signs
```

The coherent package/workspace debug, release, formatting, and strict-Clippy gate is recorded in the playback review-fix disposition after the long-run gate is added.
