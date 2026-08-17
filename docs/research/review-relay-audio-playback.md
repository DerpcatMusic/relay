# relay-audio playback review

## Scope

Independent critical/high-correctness review of the new scheduled playback path, limited to:

- `crates/relay-audio/src/playback.rs`;
- its public export/dependency integration in `crates/relay-audio/src/lib.rs`, `crates/relay-audio/Cargo.toml`, workspace `Cargo.toml`, and `Cargo.lock`;
- the directly relevant RX timestamp, clock-recovery, adaptive-resampler, and SPSC-ring contracts; and
- the claimed implementation/test evidence in `docs/research/relay-audio-playback-implementation.md` and `crates/relay-audio/tests/loopback.rs`.

No Rust source was edited. The four primary local contract groups consulted were: (1) `relay-audio` implementation/RX/integration, (2) `relay-clock`, (3) `relay-resample`, and (4) `relay-rt`. No external source was needed.

Thread-model decision: `PlaybackWorker` is worker/control-side; `PlaybackRenderer::render` is the hard-realtime callback seam; endpoint and metrics destruction remains an off-callback lifecycle responsibility.

## Findings

### HIGH — the only drift-direction playback test uses an RX-impossible media timeline, and the composed loopback does not exercise playback

**Locations:** `crates/relay-audio/src/playback.rs:727-765`; `crates/relay-audio/src/rx.rs:628-641`; `crates/relay-audio/tests/loopback.rs:12-28`; `docs/research/relay-audio-playback-implementation.md:64-69`.

The sign test presents one 480-frame PCM transaction per call but advances `ExtendedTimestamp` by 481 (`packet * 481`) while advancing the 48 kHz scheduled device position by 480. The production RX contract admits only the exact negotiated timestamp implied by extended sequence and packet duration; a 10 ms stream therefore advances by 480 RTP ticks per emitted frame and rejects a 481-tick packet step. The test proves the controller's arithmetic for a synthetic one-sample media gap, but it does not prove the sign or timestamp-domain wiring for an actual `RxWorker` output.

There is no composed fallback for that missing evidence: `FullLoopSkeleton` contains neither `PlaybackWorker` nor `PlaybackRenderer`, and its test only takes the skeleton's `size_of`. Consequently, the most important cross-module property—valid fixed-duration RTP progression bound to scheduled local device frames, yielding the correct adaptive output/input sign without packet-arrival timing—has not been executed through the public composition seam. The four-rate/three-duration unit matrix verifies fixed workspace and finite rendering, but it drains each produced block immediately and does not close this domain/integration gap.

**Potential correction:** retain exact valid RTP/media increments for the negotiated duration and represent remote clock offset in the scheduled local-device-frame mapping (using an exact/rounded long-window mapping so integer-frame quantization has zero mean). Drive real `RxWorker` outcomes through timestamp extension, `PlaybackWorker`, and `PlaybackRenderer`; cover positive, zero, and negative drift at 44.1/48/96/192 kHz and 5/10/20 ms. Assert the output/input correction sign, bounded fill around the configured target, no use of arrival/network time, no allocation growth, all-or-drop behavior, reset epoch separation, starvation zero-fill, misalignment non-consumption, and disconnect/drain lifecycle. A deterministic long-duration/virtual-hour case should then guard controller cadence, ratio convergence, and counter bounds.

### Validation gate — rustfmt currently fails outside the playback module

**Location:** `crates/relay-audio/tests/loopback.rs:7-10`.

`cargo fmt --all -- --check` exits 1 because the imports are not in rustfmt order. This is not a playback algorithm defect, but the requested repository gate is red and must be corrected before acceptance.

### Explicit no-finding for the inspected critical/high source paths

Apart from the high validation/evidence gap above, I found **no critical or high source-code correctness defect** in the inspected playback implementation:

- the public worker input names and constructs only a scheduled-playout observation; arrival/socket/wall time is absent;
- clock domains are checked as remote 48 kHz versus the configured local playback rate;
- fill is sampled at the consistent post-publication phase, `current - target` has the controller's expected sign, and the typed `ratio_multiplier` conversion preserves positive-remote-drift to negative output/input correction;
- cadence uses checked subtraction on scheduled device frames and reports a post-publication control failure without hiding committed/dropped progress;
- malformed sizes/nonfinite PCM are rejected before estimator/SRC mutation, while stateful backend/discontinuity faults require reset;
- output storage is allocated from authoritative `output_frames_max` with checked construction arithmetic and reused across calls/reset;
- ring writes are complete-or-drop, fill uses scalar capacity/free slots divided by validated channel alignment, and reset is allowed only after the old ring epoch is empty;
- callback rendering performs bounded full zero-fill followed by a partial SPSC copy, does not consume misaligned buffers, and has no direct allocation, lock, logging, I/O, network, or DSP operation; endpoint/metrics drop is explicitly forbidden on the callback; and
- worker publication/drop/update/discontinuity/reset counters plus ring drop/underrun counters expose the relevant bounded outcomes. No critical/high overflow was found under the validated pipeline bounds.

## Evidence

- `playback_pair` validates an interior target and exact estimator domains, reconstructs the authoritative controller/converter, checks `output_frames_max * channels`, allocates the workspace once, and constructs the fixed ring (`playback.rs:263-316`). `lib.rs:30-35` publicly exports the full playback API, and `relay-rt` is a direct dependency at `crates/relay-audio/Cargo.toml:9-15`; workspace membership and locked `rubato = 4.0.0` / `rtrb = 0.3.4` integration are present.
- `process_samples` validates length/finiteness before observation, uses `PlayoutClockObservation::from_scheduled_playout`, converts into the fixed workspace, records an explicit all-or-drop publication, samples post-publication fill, and returns any later controller fault together with progress (`playback.rs:363-460`).
- `ClockRecovery` defines positive fill as `current - target`, applies reciprocal drift feed-forward, subtracts proportional/integral overfill trim, validates before mutation, and rejects an overly long interval (`relay-clock/src/recovery.rs:138-151`, `195-296`). `playback.rs:463-508` matches that contract and uses the typed resampler ratio boundary.
- `AdaptiveClockConverter` uses fixed-input `FixedAsync::Input`, validates I/O before state change, smooths/clamps the target ratio, and resets retained allocation (`relay-resample/src/adaptive.rs:101-215`); the worker output is sized from its authoritative maximum.
- `AudioProducer::write` is complete-or-drop and `AudioConsumer::read` is bounded partial-copy with primitive counters (`relay-rt/src/ring.rs:71-119`, `151-201`). `PlaybackRenderer::render` initializes the entire caller output before reading and preserves the ring on misalignment (`playback.rs:531-568`). Native lock-free 64-bit callback counters are a compile-time requirement (`relay-rt/src/counters.rs:1-5`).
- Existing module tests cover construction, all rate/duration combinations, fixed workspace identity, finite output, starvation/misalignment/disconnect zero-fill, full-ring drops, quantization, progress-bearing control fault, discontinuity/drain/reset, and recoverable nonfinite input (`playback.rs:609-870`). The high finding identifies the remaining production-domain and composed-lifecycle hole.

## Required fixes

1. **Before treating playback integration as validated:** replace or supplement the synthetic `481/480` sign case with a production-reachable scheduled-playout test using valid fixed-duration RTP increments and drift expressed on the local scheduled-device-frame timeline; execute it through the real RX/playback public seam.
2. **Before claiming the full deterministic loop:** replace the `size_of` loopback skeleton with an actual playback worker/ring/renderer composition and add bounded fill/ratio plus reset/disconnect lifecycle assertions. Include the supported rate/duration matrix and a long deterministic drift/quantization soak.
3. **Before merging:** run rustfmt on the loopback import block (without altering playback semantics) and rerun every gate below.

Decision: **changes requested for validation/evidence; no critical/high algorithmic source fix requested from `playback.rs` on this audit.**

## Validation

Commands were run from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` against the inspected working tree:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | **FAIL (exit 1)** — only reported import ordering in `crates/relay-audio/tests/loopback.rs:7-10`; reproduced on a second run. |
| `cargo test -p relay-audio --locked` | **PASS (exit 0)** — 24 unit + 18 foundation + 1 loopback skeleton + 11 RX + 14 TX tests passed; doc-tests: 0. |
| `cargo test -p relay-audio --locked --release` | **PASS (exit 0)** — same 68 tests passed in release; doc-tests: 0. |
| `cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings` | **PASS (exit 0)**. |

The tests demonstrate the unit-level playback properties listed above, but the nominally passing loopback test is only a compile-time skeleton and therefore does not discharge the high finding.
