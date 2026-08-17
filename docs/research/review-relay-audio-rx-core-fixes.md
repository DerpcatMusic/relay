# relay-audio RX core review fixes

## Finding-to-change mapping

- **R1 — sequence-extension boundaries and exhaustion:** add deterministic capacity-1, ahead, half-range, before-epoch, `u64` overflow/exhaustion, reset, and wrap-boundary tests with complete metrics snapshots; harden reorder/head invariants.
- **R2 — operationally honest RX metrics:** split malformed, oversized, identity, duration/timestamp, and extension rejection counters; add `emitted_frames`; document nonexclusive FEC/PLC/error counters.
- **R3 — reorder/playout invariants:** debug-assert the reorder-returned wire head matches the extended head and define the `Playout::Empty` invariant.
- **R4 — deterministic recovery state-machine coverage:** add a private test-only scripted decoder seam proving FEC→normal exactly once, FEC error→PLC→normal, normal error→PLC→subsequent success, PLC error→zero→recovery, and Ready emits without a second decode while retaining real libopus tests.
- **R5 — hygiene/docs:** remove duplicate allowance and qualify inline address documentation.
- **Validation/evidence:** run locked relay-audio package and workspace debug/release tests, formatting checks, and strict Clippy; record commands, results, and disposition below.

## Evidence and results

- **R1 fixed:** the black-box suite now covers configured packet capacity, reorder
  capacity edge/ahead rejection, exact half range, before-epoch input, ordinary and
  wire-wrap ordering, `u64::MAX` final decision/exhaustion, and reset recovery. Each
  scenario asserts the complete metrics snapshot and returned packet ownership.
- **R2 fixed:** `RxMetrics` separates identity, duration/timestamp, malformed,
  oversized, and extension failures; adds `emitted_frames`; and documents that FEC
  attempts, PLC frames, and codec errors are nonexclusive operational counters.
- **R3 fixed:** the returned reorder sequence is debug-asserted against the extended
  head for packet and missing decisions. `Playout::Empty` after explicit rebase is
  documented/checked as an invariant fallback rather than ordinary loss evidence.
- **R4 fixed:** a private `cfg(test)` scripted decoder records calls and injects
  failures. Four unit tests prove FEC->normal exactly once with Ready reuse,
  FEC-error->PLC->normal, normal-error->PLC->next success, and PLC-error->zero->
  subsequent recovery. Real libopus black-box FEC-or-PLC tests remain.
- **R5 fixed:** the duplicate lint allowance was removed, the single reasoned
  expectation preserves fixed-inline packet storage without per-tick boxing, and PCM
  docs promise reused storage only while the worker is not moved.

Final package results:

```text
cargo fmt --all -- --check
PASS
cargo test -p relay-audio --all-targets --all-features --locked
PASS: 15 unit + 18 foundation + 11 RX + 14 TX
cargo test --release -p relay-audio --all-targets --all-features --locked
PASS: same 58 tests
cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings
PASS
```

Final integrated workspace results:

```text
cargo check --workspace --all-targets --all-features --locked
PASS
cargo test --workspace --all-targets --all-features --locked
PASS
cargo test --release --workspace --all-targets --all-features --locked
PASS
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
PASS
```

## Final disposition

All findings from `review-relay-audio-rx-core.md` are addressed. The bounded 48 kHz
RX core, honest FEC-or-PLC taxonomy, failure-containment paths, serial/timestamp
boundaries, metrics and tests pass for Phase 1 composition. Adaptive playback SRC,
clock recovery and callback publication remain outside this core and are the next
work package.
