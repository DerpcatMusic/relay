# Audio Lab Review Fixes

**Input:** `review-audio-lab.md`  
**Disposition:** implemented; independent re-review pending.

## High findings

- **H1 fixed:** removed the success-only `worker_errors` and literal
  `clean_shutdown` fields. Publication/control/render failures now remain fatal
  process errors. The document explicitly says this synchronous headless run
  has no native callback acknowledgement and makes no shutdown-metric claim.
  The successful final RX lookahead is instead recorded as the observed
  `drained_lookahead_frames` after `RxWorker::drain()` returns `Some`.
- **H2 fixed:** renamed rate fields and human labels to
  `configured_{capture,playback}_rate_hz` / `configured nominal rates`. Achieved
  frame discrepancy remains separately reported by `playback_error_frames`.

## Medium findings

- **M1 fixed:** configuration now requires at least 50 ms: this is the smallest 10 ms-aligned input duration that produces at least two complete packets even for the supported 192 kHz capture / 20 ms packet case after fixed-SRC startup. The complete matrix is additionally proven at 100 ms. Integration tests now parse every JSON result with
  `serde_json`, including the signed playback error. They exercise all 48
  rate/duration combinations (4 capture x 4 playback x 3 packet durations),
  exact input and clean ingress/emission/drain identities, the full 50 ms minimum matrix and the 10,000 ms maximum boundary, unsupported rates, missing arguments and
  out-of-range values. The maximum run exercises the declared finite capacity.
- **M2 fixed:** `network_duplicate_requests`,
  `network_duplicate_copies_scheduled`, and `rx_duplicate_rejections` are
  separate facts. The impaired fixture freezes exact values 1/1/1 plus two
  drops, 23 accepted packets, and 25 emitted frames.

## Validation

Executed from the repository root after formatting:

- `cargo test --locked -p relay-audio-lab --all-targets` — PASS, 6 tests,
  10.29 s.
- `cargo test --release --locked -p relay-audio-lab --all-targets` — PASS,
  6 tests, 0.35 s test execution.
- `cargo clippy --locked -p relay-audio-lab --all-targets -- -D warnings` —
  PASS.
- `cargo fmt --all -- --check` — PASS before the commands above; `cargo fmt
  --all` produced the validated formatting.

## Remaining external limitation

There is still no platform audio-device adapter, native callback acknowledgement,
or hardware deadline evidence. `--device` intentionally exits nonzero and the
document retains that release gate.
