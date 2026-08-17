# Review: relay-audio 12-virtual-hour gate

## Scope

Independent, read-only audit of:

- `crates/relay-audio/tests/virtual_hours.rs`
- `docs/research/relay-audio-12h-gate.md`

The comparison baseline is the approved twelve-hour exit gate in
`docs/research/relay-audio-composition-design.md:122-133` and the Phase-1 exit criterion in
`docs/plans/2026-08-15-relay-master-plan.md:3705-3715`. No Rust source was edited.

## Verdict

**FAIL — the current test is a useful deterministic synthetic control/count soak, but it does not
satisfy the approved 12-virtual-hour exit gate.** The debug and release target pass quickly, and the
production jitter/clock-control type usage is sound, but required duration, scenario, matrix, and
latency/accounting coverage is absent or not asserted.

## Critical findings

### C1. The clock/recovery/ring plant advances one packet less than 12 hours

`packet_count` represents exactly 12 hours of packets (`virtual_hours.rs:224-228`), and the synthetic
decode total counts all of them (`:406-420`). The control plant is different: its observations run
from `deadline_index == 0` through `packet_count - 1` (`:262`, `:321-354`), while consumption,
production, and recovery updates are skipped for index zero (`:370-396`). Consequently there are
only `packet_count - 1` produced/consumed intervals. The last observed scheduled boundary is the
start of the final packet, at 12 hours minus 5, 10, or 20 ms, rather than a terminal boundary at 12
hours.

The extra `PLAYOUT_DELAY_PACKETS` loop ticks only drain the delayed network; they do not add the
missing terminal media/device interval. Thus the document's unqualified statement that every case
"advances 12 virtual hours" (`relay-audio-12h-gate.md:5-8`) is true for generated/classified media
frames, but false for the estimator/recovery/ring plant that the exit gate is meant to soak.

**Required correction:** model explicit interval boundaries through remote position
`packet_count * media_frames`, process exactly `packet_count` production/consumption intervals,
and independently assert exact remote and local terminal positions/durations for every case.

### C2. The approved deterministic scenario and supported configuration matrices are not covered

The approved gate requires drift `[-250,-100,-20,0,20,100,250]` ppm, zero-mean jitter, 1-10 ms
delay steps/ramps, 0/1/5% loss, deterministic loss bursts, duplicates, and reorder. The test instead
has one three-stage `+250/0/-250` trace (`virtual_hours.rs:23`, `:178-195`), one independent
13/1024 loss profile, and a uniform 0-3-*packet* delay (`:278-303`). Depending on packet duration,
that delay spans 0-15, 0-30, or 0-60 ms; it is neither the required 1-10 ms step/ramp matrix nor a
zero-mean jitter scenario. There are no clean 0%, exact 1%, 5%, or deterministic burst cases.

The device matrix is also hard-coded to 44.1 and 48 kHz (`:24-31`). The authoritative Phase-1
supported set is 44.1, 48, 96, and 192 kHz (`crates/relay-resample/src/lib.rs:26-27`), so 6 of the 12
supported `(device rate, packet duration)` pairs are missing. The evidence document presents the
six implemented cases as the scope (`relay-audio-12h-gate.md:5-8`) but does not disclose that this
is only half of the supported matrix.

**Required correction:** add a deterministic scenario table covering every approved drift,
jitter/delay, loss/burst, duplicate, and reorder profile across all four supported device rates and
all three negotiated durations. Keep fixed per-scenario expected metrics and document the complete
matrix.

## High findings

### H1. The central latency-drift acceptance criterion is not established

The plant invents a one-device-second target and permits fill anywhere from zero to two seconds
(`virtual_hours.rs:250-252`, `:380-384`). This is not derived from the configured playback-ring
capacity or safe margin, and it is not the approved bound. Only the final fill error is retained and
asserted (`:424-431`, `:461-465`); no high-water, per-stage fill, monotonic-trend, target-delay,
peak/RMS nominal correction, or saturation telemetry is checked. A long fill/latency trend that
later reverses can pass.

This does not prove "12 virtual hours without latency drift," nor the acceptance requirements that
correction/target delay remain configured, final fill stay within one negotiated frame, maximum
fill remain inside the configured safe margin, and nominal zero-mean jitter meet peak/RMS limits.
The evidence claims only the synthetic `[0, 2 * device_rate]` bound
(`relay-audio-12h-gate.md:39-49`) and omits the approved configured-margin/trend requirements.

**Required correction:** derive target/capacity/safe margin from the authoritative pipeline
configuration; retain and assert min/max and per-stage/trend metrics, controller bounds and all
saturation flags, and peak/RMS correction in the nominal-jitter cases.

### H2. Delay and reorder occur for the fixed seeds but are completely unasserted

Loss and duplication have counters, exact fixture values, and positivity assertions
(`virtual_hours.rs:53-64`, `:72-139`, `:438`, `:468-471`). Delay is calculated but never counted
(`:282-303`), and arrival-order inversion/reordering is never counted. The accepted/delivered and
FEC/PLC totals are unchanged if every accepted packet arrives before its deadline; changing all
delays to zero can therefore leave every current expected metric intact. Manual reproduction of
the fixed PRNG prefix confirmed positive delayed deliveries and arrival inversions in all six
current cases, but the test itself cannot detect their removal.

**Required correction:** record network truth for every delay bucket, late/on-time delivery, and
actual arrival-order inversion/reorder event; give each deterministic scenario exact expected
values and explicit nonzero assertions where the scenario requires the impairment.

### H3. Produced/consumed/SRC-delay accounting and plant invariants are incomplete

The fractional output accumulator is present and the stable-phase order is sensible
(`virtual_hours.rs:370-395`): consume the scheduled delta, add corrected nominal output, retain the
fraction, update fill, then update recovery for the next interval. However, the test never retains
or independently asserts total produced frames, total consumed frames, fractional remainder
bounds, SRC delay, lookahead pending frames, or the accounting identity required by the approved
gate. The final fill assertion is derived from the same incremental plant and cannot show which
explicit delay/pending term explains a discrepancy. The C1 missing interval demonstrates the gap:
synthetic media totals still pass while adaptive output accounting is short by one packet.

**Required correction:** use checked integer/rational accumulators where possible; assert the
fractional remainder invariant after every step; expose checked produced/consumed totals and prove
`initial + produced - consumed = final` with every SRC delay, lookahead, concealment, or drop term
named separately.

## Medium findings

### M1. Estimator warmup and stage-transition convergence are not tested

`WarmingUp` is ignored (`virtual_hours.rs:359-368`), with no count or deadline for the first
estimate. Only end-of-stage values are sampled (`:399-403`, `:440-459`); no transition settling,
overshoot, convergence time, or stage-local fill is asserted. The result arrays start at zero
(`:255-256`), making the expected zero-drift stage especially weak because an unwritten slot has
the expected value. Discontinuities do fail, which is good, but that alone does not verify warmup
or transitions.

**Required correction:** initialize stage results with non-valid sentinels/visited flags; assert the
exact warmup/estimate schedule (or an explicit bounded schedule), stage visitation, transition
settling windows, convergence by a declared deadline, and fill/correction behavior at each stage.

### M2. Sequence wrap is not exact, and some arithmetic hides rather than proves overflow safety

Timestamp wrap is asserted exactly once (`virtual_hours.rs:422`, `:467`), and checked extended
remote positions plus checked metric increments are positive safeguards. Sequence wrap is only
`> 1` (`:421`, `:466`) even though the exact current expectations are 132, 66, and 33 wraps for
5, 10, and 20 ms. `fixed_timestamp` uses `wrapping_mul` followed by a truncating cast (`:174-176`),
and aggregate products/sums in final assertions are unchecked (`:413-420`). Intentional wire-width
wrap should not mask an unintended extended-width overflow.

**Required correction:** independently assert exact sequence and timestamp wrap counts per case;
perform checked extended multiplication/addition first and only then intentionally reduce to the
wire width; use checked arithmetic in aggregate/accounting assertions.

### M3. The evidence's validation recipe is not the strict, locked gate and workspace fmt is red

`relay-audio-12h-gate.md:64-70` omits `--locked` from Cargo invocations and documents only a
single-test Clippy command, not strict package/all-target coverage. On the audited tree the focused
Rust file is formatted, but the documented `cargo fmt --all -- --check` fails on unrelated
`apps/audio-lab` files. Therefore the document cannot presently present the workspace formatting
command as a green validation result.

**Required correction:** document and run locked check/debug/release commands and strict Clippy
(`--all-targets --all-features --locked -- -D warnings` as applicable); distinguish focused-file
format success from the currently failing workspace-format gate instead of implying full green
validation.

## Verified properties

- All six *implemented* cases generate/classify exactly 12 hours of 48 kHz synthetic media:
  2,073,600,000 frames, with checked per-counter/sample accumulation and fixed expected metrics.
- Production `ReorderBuffer`, `DriftEstimator`, `ClockRecovery`, and
  `OutputInputRatioCorrectionPpm` are used (`virtual_hours.rs:7-12`, `:234-248`, `:309-340`,
  `:355-395`). Raw arrival/network time never enters `PlayoutClockObservation`.
- The scheduled mapping uses the reciprocal remote-rate factor, and positive/negative drift is
  asserted to yield negative/positive output/input correction (`:178-195`, `:440-459`).
- The one-frame lookahead taxonomy is correct at this synthetic boundary: isolated/finally
  recoverable missing frames are FEC-or-PLC attempts, consecutive/terminal missing frames are PLC,
  the following packet remains primary, the final pending decision is drained, and exact fixture
  counts are asserted (`:197-221`, `:342-344`, `:406-420`, `:438`).
- Loss, duplicates, FEC-or-PLC, and PLC are all nonzero and asserted. Network-slot overflow and
  unexpected reorder rejection fail the test.
- The source and evidence truthfully disclose the synthetic decode/sample-count boundary and the
  exclusion of encoded Opus bytes, decoded sample values, the Opus decoder, resampler kernel, and
  realtime callback (`virtual_hours.rs:1-5`, `:154-162`; `relay-audio-12h-gate.md:10-14`, `:75-83`).
  `InbandFecOrPlc` is a classification/attempt only, not proof that Opus FEC data existed.
- The fixed PRNG seed and exact impairment/classification metrics make the currently measured
  quantities deterministic. Floating control results use declared tolerances.

## Validation run

| Command | Result |
|---|---|
| `cargo check -p relay-audio --test virtual_hours` | PASS |
| `cargo test -p relay-audio --test virtual_hours` | PASS; test body 8.94 s, shell 8.974 s |
| `cargo test --release -p relay-audio --test virtual_hours` | PASS; test body 1.46 s, shell 1.493 s |
| `cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings` | PASS |
| `rustfmt --edition 2024 --check crates/relay-audio/tests/virtual_hours.rs` | PASS |
| `cargo fmt --all -- --check` | **FAIL outside reviewed files**: formatting diffs in `apps/audio-lab/src/diagnostics.rs` and `apps/audio-lab/src/main.rs` |

The measured debug runtime remains below the stated 30-second budget and is consistent with the
approximately 9.2-second workstation figure in `relay-audio-12h-gate.md:72-73`.
