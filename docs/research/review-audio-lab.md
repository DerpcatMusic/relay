# Audio Lab Independent Review

## Scope
Review limited to `apps/audio-lab`, `docs/audio-lab.md`, and workspace integration (`Cargo.toml`/`Cargo.lock`). No source files were edited.

## Result
No critical findings. The executable does drive the public capture conversion/Opus TX, deterministic network, RX, scheduled-playout adaptive conversion, playback ring, and renderer path. Playback clock input is reconstructed from extended sequence/timestamp and scheduled device frames (`main.rs:263-281`); network arrival time is not supplied to clock recovery. The clean/impaired actions are deterministic, RX lookahead is explicitly drained, the playback ring is empty-checked, FEC is conservatively labeled `FEC-or-PLC`, and `--device` clearly fails as unavailable.

## High findings

### H1 — `worker_errors` and `clean_shutdown` are success constants, not observed metrics
Evidence: every playback publication/control fault increments `worker_errors` and immediately returns `Err` (`apps/audio-lab/src/main.rs:283-286`), so no successful diagnostic can expose a nonzero value. `clean_shutdown` is assigned literal `true` (`main.rs:361`) without an observed shutdown/acknowledgement operation. The test only looks for the literal true value (`tests/headless_smoke.rs:45,110,122`). Thus the documentation claim that the summary reports playback error values and “clean bounded shutdown” (`docs/audio-lab.md`, summary paragraph) overstates what the JSON proves. In this single-thread headless program, resources do drop before `run` returns and before printing, but the fields themselves cannot detect a regression in shutdown or report a handled worker error.

### H2 — “effective rates” are merely requested/configured nominal rates
Evidence: diagnostics copy CLI configuration directly (`main.rs:357-358`) and human output labels these values “effective rates” (`diagnostics.rs:28,42-43`). There is no measurement or achieved-rate calculation. `docs/audio-lab.md` partly qualifies these as “configured effective rates,” but the emitted label remains misleading. The actual conversion discrepancy is separately visible: a clean 44.1 kHz→192 kHz, 100 ms, 5 ms run reported 19,192 rendered frames and `playback_error_frames:-8`, while still printing the configured 44,100/192,000 as effective rates.

## Medium findings

### M1 — JSON validity and matrix/count claims are weakly tested
Evidence: the “JSON” test checks only leading/trailing braces and extracts unsigned fields with string splitting (`tests/headless_smoke.rs:10-18,43-55`); it never parses JSON. This helper cannot parse a negative field such as `playback_error_frames`. The supported-rate test covers only 4 of 16 capture/playback pairs and assigns the three packet durations across those four cases (`tests/headless_smoke.rs:85-113`), rather than exercising the full advertised cross-product. Clean mode checks `encoded_packets == emitted_frames`, but no test asserts exact expected packet/input/render totals for each duration/rate, or separately proves the final `rx.drain()` frame identity/count. Boundary durations (20 and 10,000 ms), unsupported rates, missing values, and capacity/error/overflow behavior are also not covered. The current tests are not wholly vacuous—they launch the real binary, repeat output, demand packets/checksum, and check zero drops/ring faults—but they leave these claims under-proven.

### M2 — `network_duplicates` counts requests, not demonstrated duplicate delivery
Evidence: the field is populated from `network.metrics().duplicate_requests` (`main.rs:347-350`), and the impaired test asserts it equals one (`tests/headless_smoke.rs:77-79`). RX accepts any duplicate rejection in impaired mode (`main.rs:316-320`) and no delivered/duplicate-rejected count is recorded. The documentation’s phrase “deterministic-network truth” is defensible only if interpreted as configured scheduler action truth; consumers could reasonably read `network_duplicates` as actually delivered/observed duplicates.

## Other verified observations
- Bounded configuration is enforced: rates are allow-listed, packet duration is 5/10/20 ms, duration is a multiple of 10 in 20..=10,000, and pipeline/network/batch capacities derive from that bounded duration (`main.rs:46-53,83-92,120-143,188-212`).
- The harness allocates synthetic and render vectors in the control loop (`main.rs:147-155,217,291`); it does not claim this loop is an audio callback. The narrower claim that the renderer receives caller-owned storage and does not print is accurate. There is no native callback to validate allocation/deadline behavior, and the device limitation is explicitly documented.
- The 10-second virtual impaired run completed successfully and reported 500 encoded/emitted frames, 11 simulated drops, 11 FEC-or-PLC attempts, zero explicit PLC frames, zero ring faults, and deterministic checksum. This is honest about attempts: it does not claim proven FEC recovery.
- Workspace membership and lockfile integration are present (`Cargo.toml` member `apps/audio-lab`; `Cargo.lock` package `relay-audio-lab`).

## Commands run
All passed:

- `cargo fmt --all -- --check`
- `cargo test --locked -p relay-audio-lab --all-targets` (debug; 5 integration tests)
- `cargo test --release --locked -p relay-audio-lab --all-targets` (5 integration tests)
- `cargo clippy --locked -p relay-audio-lab --all-targets -- -D warnings`
- `cargo run --locked -p relay-audio-lab -- --json --duration-ms 10000 --profile impaired --seed 7` (6.38 s wall time)

## Status
Complete.
