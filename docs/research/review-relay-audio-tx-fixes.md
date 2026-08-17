# relay-audio TX review fixes

## Finding mapping

- [x] **H1 — validation cannot hide prior output:** `TxWorker::process_capture` validates chunk length and finiteness before draining accepted output. Every later live failure carries a `TxProcessFailure` with the exact committed `TxProcessReport`.
- [x] **H2 — converter failures have explicit recoverability:** prevalidated input errors leave the live worker usable; backend and non-finite-output failures fault it until a successful reset.
- [x] **M1 — PCM/packet commit is transactional:** the accumulator now peeks a complete frame, encodes and commits the packet/timeline, then discards PCM. Live failures report packets already committed. The finite worker has explicit ready/completed/faulted one-shot state and `FiniteTxError` reports confirmed input and packet progress.
- [x] **M1 — reset is staged:** the worker first enters `Faulted`, performs the fallible codec reset, and only after success clears converter/pending state and re-enters `Active`. Reset failure is documented to leave the epoch faulted and retryable only through reset.
- [x] **M2 — finite capacity preflight is truthful:** an undersized batch reports zero consumed source frames and a distinct `required_batch_capacity`; no conversion occurs and the ready state is retained.
- [x] **M3 — disconnect events are strict:** a `Chunk` submitted while `Disconnecting` is rejected without draining or consuming it; only repeated `Disconnected` events may continue the bounded drain.
- [x] **M4 — configured delay and abandoned tail are distinct:** disconnect reports both `configured_converter_delay_frames` and `abandoned_converter_tail_frames`; an immediate non-unity disconnect abandons zero frames, while a converter that accepted input reports its retained tail.
- [x] **L1 — focused contract coverage:** added direct SRC accounting, `input_pending` ownership under repeated one-slot backpressure, validation-with-prior-output, disconnect sequencing, non-unity immediate/post-input disconnect, recoverable/faulting converter errors, transactional encode/reset failures, finite capacity/reject/empty/repeat/recoverable/error-progress cases, and fixed-storage reuse coverage.
- [x] **L1 — portable determinism evidence:** removed fixed Opus/SRC payload hashes. The matrix now asserts same-run packet equality, bounded invariant packet counts, exact sequence/timestamp steps, non-empty payloads, and decoded-frame/count tolerances that are stable across CI targets.
- [x] **RT storage constraint:** production changes add no allocation, growth, locks, waits, or I/O to processing/reset. Accumulator frame staging reuses the existing PCM box; reports and lifecycle states are fixed-size values; existing pointer/capacity identity coverage remains active.

## Public behavior changes

- `TxProcessOutcome::Error` now contains `TxProcessFailure { cause, progress }` so callers cannot lose packets or accounting committed before a fault.
- `FiniteTxWorker::process_finite` returns `Result<FiniteTxReport, FiniteTxError>`; it is retryable only after a zero-progress validation/capacity rejection, completes exactly once, and becomes terminally faulted after ambiguous backend/output or packetization failures.
- `FiniteTxReport::required_batch_capacity` separates required output capacity from source consumption.
- `LiveDisconnectReport` separates configured converter delay from the tail actually abandoned by the epoch.

## Validation evidence

Executed with the checked-in lockfile:

- `cargo check -p relay-audio --all-targets --all-features --locked` — **PASS**.
- `cargo check --workspace --all-targets --all-features --locked` — **PASS**.
- `cargo test -p relay-audio --locked` — **PASS** (unit, foundation, RX integration, TX integration, docs).
- `cargo test --release -p relay-audio --locked` — **PASS**.
- `cargo test --workspace --all-features --locked` — **PASS** (one intentional libopus artifact smoke ignored).
- `cargo test --release --workspace --all-features --locked` — **PASS** (two intentional release/artifact gates ignored unless explicitly selected).
- `cargo fmt --all -- --check` — **PASS**.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — **PASS**.

The shared tree changed concurrently in RX-owned files during this work; no RX source or test was edited for this TX task. The final literal workspace formatting and strict-Clippy gates above were rerun after those owners completed their changes.
