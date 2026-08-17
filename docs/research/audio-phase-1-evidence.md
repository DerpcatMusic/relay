# RELAY Phase 1 audio acceptance evidence

**Status:** Local Phase-1 automated gates pass (lab, 60 s media, finite drain, 12-hour matrix, callback-safety audit, locked workspace). Hosted three-OS CI, physical-device smoke, and an independent 12-hour re-review remain.

## Scope

This record closes the Phase-1 engine/lab gate only when every row below has
direct code, test, and command evidence. It does not claim native transport,
browser, plugin, TURN, Stream, account, billing, or release readiness.

## Implemented modules

- `relay-rt`: bounded SPSC audio transport and primitive atomic metrics.
- `relay-clock`: scheduled-playout estimator and bounded recovery controller.
- `relay-jitter`: wrap-safe reorder/deadline state machine.
- `relay-resample`: fixed capture and adaptive playback SRC contracts.
- `relay-opus-sys` / `relay-opus`: canonical V1 libopus boundary.
- `relay-audio`: transactional TX, bounded fake network, one-frame-lookahead
  RX/FEC-or-PLC, scheduled playback worker, and callback renderer.

## Gate inventory

| Gate | Evidence | Status |
|---|---|---|
| Primitive reviews/fixes | linked implementation and review records | PASS |
| TX/RX composition | `relay-audio` unit/integration suites | PASS |
| Reviewed real-codec integration loop | `tests/loopback.rs`, loopback review, and fix record | PASS |
| 60-second real-media live stream at 5/10/20 ms | test/evidence/review/fix disposition | PASS |
| Finite capture finish and adaptive playback drain | Option A design/synthesis; strict finite TX; adaptive finish final PASS; real TX→RX-drain→playback finish fix disposition PASS | PASS |
| 12 virtual hours | Corrected matrix in `tests/virtual_hours.rs` and `relay-audio-12h-gate.md`; locked debug/release/Clippy pass 2026-08-17 | PASS locally; independent re-review still useful |
| Headless audio lab | Public full path plus independent review/fix disposition | PASS |
| Callback safety | `PlaybackRenderer::render` + `AudioConsumer::read` audited 2026-08-17; checklist below | PASS (headless; no device-deadline measurement) |
| Debug/release workspace | locked fmt/check/debug-test/release-test/clippy/deny passed 2026-08-17 | PASS locally |
| Platform device smoke | manual supported-host execution | EXTERNAL GATE |

## Realtime callback checklist

- [x] Caller output is fully initialized on every return path.
      `PlaybackRenderer::render` (`playback.rs:1028-1056`) `fill(0.0)`s the
      whole slice before any read; misaligned returns also leave a fully
      zeroed buffer.
- [x] Work is bounded by caller-provided scalar sample count.
      `AudioConsumer::read` (`relay-rt/src/ring.rs:158-182`) copies at most
      `output.len()` samples via `pop_partial_slice`.
- [x] No allocation, lock, wait, I/O, logging, networking, codec, SRC, DSP,
      owner destruction, or panic path occurs in render.
      The callback path is zero-fill + lock-free SPSC pop + relaxed atomic
      underrun increments + constant-time `RenderState` mapping. Worker
      encode/decode/SRC/recovery stay on `PlaybackWorker`.
- [x] Starvation/disconnect zero-fill is tested.
      `renderer_zero_fills_starvation_misalignment_and_resumes_without_consuming_odd_call`
      and `disconnected_is_terminal_only_after_the_post_read_queue_is_empty`;
      ring tests cover partial-read underrun counts.
- [x] Endpoint destruction occurs only after host callback acknowledgement.
      `RenderState::Disconnected` is emitted only when the producer is
      abandoned **and** the post-read queue is empty. Workers must be dropped
      off-callback; `reset_when_empty` refuses a nonempty ring.
- [x] Primitive counters are observational and final coherent summaries are
      assembled off callback.
      `AudioRingMetrics::snapshot` is relaxed atomic loads; coherent finish
      reports are built on the worker after stop.

Device-callback deadline microseconds remain an external/platform gate.

## Final automated commands

```sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-targets --all-features
cargo test --release --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo deny check licenses advisories sources bans
```

## Results

Executed from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` on 2026-08-17.

| Exact command | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --locked --workspace --all-targets --all-features` | PASS |
| `cargo test --locked --workspace --all-targets --all-features` | PASS (includes 12-hour soak ~62 s debug; `media_60s` ~203 s debug) |
| `cargo test --release --locked --workspace --all-targets --all-features` | PASS (12-hour soak 3.92 s; `media_60s` 2.51 s) |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo deny --locked check licenses advisories sources bans` | PASS — advisories/bans/licenses/sources ok; only unused BSD/ISC allowance warnings |

## Remaining external evidence

- Hosted Linux/Windows/macOS workflows have not run because the repository has
  no configured remote or initial Git baseline.
- Physical-device smoke and callback deadline measurements require supported
  audio hardware/host APIs and remain explicitly outside headless CI.
- Portable libopus packaging and production allocation/CPU instrumentation are
  release gates, not silently treated as Phase-1 proof.
