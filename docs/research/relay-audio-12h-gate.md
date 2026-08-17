# relay-audio Phase-1 virtual 12-hour gate

## Scope

`crates/relay-audio/tests/virtual_hours.rs` is a deterministic, single-threaded
virtual-time endurance test. It covers all twelve `(device rate, packet
duration)` pairs formed by the Phase-1 supported set 44.1 / 48 / 96 / 192 kHz
and 5 / 10 / 20 ms. Every case advances exactly 12 virtual hours without
sleeping or reading wall, network, or arrival time.

The gate ends at an explicit **synthetic decode/sample-count boundary**.
Production packet ordering and clock-control types are exercised, but a
primary, one-frame in-band-FEC-or-PLC attempt, and PLC are each represented by
exactly one packet-duration of 48 kHz media frames. Encoded Opus bytes, decoded
sample values, the Opus decoder, and the resampling kernel are not part of this
endurance model.

## Review corrections applied

The previous six-case soak failed independent review
(`review-relay-audio-12h-gate.md`). This revision addresses every C/H/M finding:

| Finding | Correction |
|---|---|
| C1 missing terminal interval | The plant observes remote positions `0, F, 2F, …, packet_count·F` and runs exactly `packet_count` produce/consume intervals. Terminal remote and local positions are asserted independently. |
| C2 incomplete matrix | All four supported device rates and all three durations run. Each 12-hour case is staged across the approved drift set and the approved impairment profiles. |
| H1 invented fill bound | Target, capacity, and safe margin come from `AudioPipelineConfig` + `PlaybackConfig::for_pipeline`. Final fill error must stay inside one negotiated device frame; max fill must stay inside capacity and the configured safe margin. |
| H2 unasserted delay/reorder | Delay buckets 0..=10 ms, on-time/delayed/late, arrival inversions, and `AcceptedPacket::Reordered` are counted and required to be nonzero where the scenario demands them. |
| H3 incomplete accounting | Produced and consumed frames are checked `u64` totals. Fill is rebuilt from `target + produced − consumed` every interval. The fractional SRC remainder is asserted in `[0, 1)` after every step. |
| M1 warmup / stages | Warmup observations are counted. The first estimate must appear within three virtual seconds. Every stage has a visited flag and NaN sentinels before sampling. |
| M2 wrap / overflow | Sequence and timestamp wraps are computed from checked extended arithmetic and asserted exactly (132 / 66 / 33 sequence wraps; one timestamp wrap). |
| M3 unlocked validation | Commands below are locked and include package-wide Clippy. |

## Exact deterministic model

- No PRNG. Impairments are closed-form functions of the global source packet
  index so expected loss, burst, duplicate, and inversion totals are independent
  of the plant.
- Twelve hours are partitioned into seven stages that sum to 720 minutes
  (120 + 6×100) so every duration divides every stage:

  | Stage | Minutes | Drift (ppm) | Impairment |
  |---:|---:|---:|---|
  | 0 | 120 | 0 | 20 min clean warmup, then zero-mean jitter plus a 1–10 ms ramp |
  | 1 | 100 | −250 | clean |
  | 2 | 100 | −100 | exact 1% loss (`index % 100 == 99`) |
  | 3 | 100 | −20 | exact 5% loss (`index % 20 == 19`) |
  | 4 | 100 | +20 | 1–10 ms delay steps (`1 + index % 10`) |
  | 5 | 100 | +100 | 3-packet bursts every 200 packets |
  | 6 | 100 | +250 | duplicate every 256th packet; adjacent-pair arrival inversion |

- RTP progression: sequence starts at 65,530 and increments with `u16`
  wrapping; timestamp starts at `u32::MAX − 3 · media_frames` and advances by
  the fixed 48 kHz packet frame count. Extended positions use checked `u64`
  arithmetic first; the wire width is applied only after that.
- Reordering: production `ReorderBuffer<u32>` is rebased to 65,530. Adjacent
  reorder pairs are generated even-first, held, and flushed on the later odd
  tick into a not-yet-drained slot so inversion is visible without a late
  rejection.
- Clock: scheduled local positions are the rounded piecewise conversion of
  extended remote media frames to device frames using
  `media · device_rate · 1e6 / (48_000 · (1e6 + drift_ppm))`. Only
  `PlayoutClockObservation::from_scheduled_playout` enters `DriftEstimator`.
- Ring: target fill is `PlaybackConfig::for_pipeline` (converter output plus
  delay, strictly inside the 100,000-sample stereo ring). Each interval
  consumes the integer scheduled-local delta and produces the corrected
  nominal device frames through a retained fractional accumulator.

## Assertions

For every case the test asserts:

- remote terminal position is exactly `packet_count · media_frames`
  (2,073,600,000 frames) and local terminal position matches the piecewise
  schedule;
- `initial + produced − consumed = final` fill, with the SRC remainder in
  `[0, 1)` and no unnamed leftover;
- fill stays in `[0, capacity]`, never exceeds `target + safe_margin`, and
  every stage-end plus the final fill error is within one negotiated device
  frame;
- each of the seven stages is visited; end-of-stage drift is within 3 ppm of
  the configured value; correction has the reciprocal sign and is within 2 ppm
  of the exact reciprocal;
- estimator warmup occurs and the first estimate is available within three
  virtual seconds;
- correction stays inside `ClockRecoveryConfig::max_abs_correction_ppm`;
  drift-input and ring-fill clamps stay at zero;
- 0-ppm stage peak/RMS correction stay inside 1.05 / 1.02 ppm (the
  controller unit-test 0.05 / 0.02 bounds are for a deadbanded quantized-fill
  plant at exact 0 drift; this soak includes scheduled-position rounding at
  every supported rate);
- sequence wraps are exactly 132 / 66 / 33 for 5 / 10 / 20 ms; timestamp wrap
  is exactly once;
- delay buckets 1..=10 ms, inversions, reordered accepts, duplicates, 1% + 5%
  + burst loss, FEC-or-PLC, and PLC are all present and match the closed-form
  scenario totals where those totals are independent of concealment;
- 1–10 ms delay never arrives after the four-packet playout deadline.

## Validation

Run from the repository root:

```text
cargo test --locked -p relay-audio --test virtual_hours --all-features
cargo test --release --locked -p relay-audio --test virtual_hours --all-features
cargo clippy --locked -p relay-audio --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Observed on the implementation workstation (2026-08-17):

| Command | Result |
|---|---|
| locked debug `virtual_hours` | PASS, ~53 s |
| locked release `virtual_hours` | PASS, 3.42 s |
| locked package Clippy `-D warnings` | PASS |
| `cargo fmt --all` | applied; audio-lab files that previously failed format were included |

Runtime is machine-dependent; the test does not inspect elapsed wall time.
Release is the intended CI duration. Debug is slower because it walks twelve
full 12-hour plants.

## Limitations

This is a control-plane/sample-count endurance model, not an audio-quality,
codec-bitstream, resampler-kernel, multi-channel, transport, RT callback, or
hardware-device test. FEC-or-PLC records the production-style attempt
classification but cannot determine actual Opus FEC availability without
encoded payloads. Floating-point control results use explicit tolerances;
network and classification identities are integer-exact.

## Local primary sources consulted

1. `crates/relay-jitter/src/lib.rs` — bounded reorder, push classifications, deadline playout.
2. `crates/relay-clock/src/estimator.rs` — scheduled-playout observation and drift semantics.
3. `crates/relay-clock/src/recovery.rs` — reciprocal feed-forward, PI fill control, and cadence.
4. `crates/relay-audio/src/playback.rs` — `PlaybackConfig::for_pipeline` target and ring bounds.
5. `crates/relay-resample/src/lib.rs` — supported rate set.
6. `docs/research/review-relay-audio-12h-gate.md` — failed-review required corrections.
7. `docs/research/relay-audio-composition-design.md` — approved twelve-hour exit gate.
