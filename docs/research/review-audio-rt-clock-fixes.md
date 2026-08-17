# Audio RT/Clock Review Fixes

## Disposition

All acceptance findings in [`review-audio-rt-clock.md`](review-audio-rt-clock.md) are addressed. The estimator now accepts only scheduled media progression on a local audio-device frame timeline; the controller isolates quantized fill motion, bounds update age, applies exact reciprocal feed-forward, handles both amplitude and slew anti-windup, and reports each limiting cause. The `relay-rt` implementation remains unchanged; its missing concurrent wrapper coverage is added as an integration test.

## Findings-to-fixes mapping

| Review finding | Fix and disposition | Deterministic evidence |
|---|---|---|
| **High: raw packet-arrival endpoint delay is indistinguishable from clock drift** | **Fixed by contract and API shape.** `DriftEstimator::observe` and its free-form seconds argument were removed. `PlayoutClockObservation` has private fields and only `from_scheduled_playout(remote_media_sample_position, local_device_frame_position)`. `observe_scheduled_playout` therefore consumes extended scheduled media progression against a monotonic audio-device frame counter. Crate/type/method documentation explicitly rejects raw packet arrival, socket receive, network transit, and wall-clock timestamps. Configuration names the local device rate used to convert frame progression to elapsed time. | `multi_window_network_jitter_and_delay_steps_are_not_observations` crosses at least 50 complete windows for each of -250, 0, and +250 ppm while separately generating alternating arrival jitter, 10 ms/1 ms delay steps, and an asymmetric delay ramp. Those network values cannot enter the device-frame observation and every estimate converges within 0.3 ppm. `estimator_converges_for_required_drift_range` covers the full required range. |
| **Medium: instantaneous ring-fill error directly frequency-modulates output** | **Fixed.** Fill is clamped, passed through a backward-Euler time-based first-order low-pass, and then through a continuous symmetric deadband before proportional or integral control. The API contract requires one stable sampling phase per worker update and explicitly forbids alternating pre/post packet or block phases. Defaults are a 1 s time constant and 24-frame deadband. | `time_filter_and_deadband_reject_quantized_fill_jitter_at_variable_cadence` drives a zero-mean five-level, 96-frame-span quantized sawtooth at six varying update intervals and bounds both peak (<0.05 ppm) and RMS (<0.02 ppm) correction. |
| **Medium: anti-windup ignores slew saturation and long gaps integrate stale error** | **Fixed.** Candidate integration is compared with the command actually reachable after amplitude and slew limits. Conditional integration suppresses any integral movement that drives farther into either residual; movement back toward the reachable range remains enabled. `max_update_interval_seconds` is a validated hard cadence bound (default 250 ms); larger intervals return `UpdateIntervalTooLong` before filter, integral, or command mutation. | `anti_windup_covers_slew_and_both_amplitude_reversals` proves initial amplitude+slew suppression, no windup, and reversal away from both negative and positive saturation. `long_gap_is_rejected_without_any_state_mutation` compares all controller state after a rejected 251 ms gap. The exact plant test exercises six variable `dt` values. |
| **Low: feed-forward ratio is only a first-order approximation and its plant repeats that approximation** | **Fixed.** Feed-forward is calculated in ratio space as `1 / (1 + drift_ppm / 1_000_000)`. `correction_ppm` is the exact ratio-space representation and `ratio_multiplier` is exactly `1 + correction_ppm / 1_000_000`. | `feed_forward_converges_to_exact_reciprocal_ratio` uses exact equality across -250 through +250 ppm. `exact_rate_ratio_plant_converges_with_variable_dt` models input consumption as the reciprocal of the output/input multiplier, uses variable cadence, bounds fill, and converges to the exact required plant ratio. |
| **Low: aggregate `saturated` omits integral limiting/suppression** | **Fixed with split telemetry.** Output separately reports `drift_input_clamped`, `ring_fill_input_clamped`, `integral_limited`, `anti_windup_active`, `amplitude_limited`, and `slew_limited`; `saturated` is their aggregate. Attempted limiting remains reported even when anti-windup changes the final request. | `split_saturation_telemetry_reports_every_limiter` triggers and asserts every individual cause and the aggregate. |
| **`relay-rt` residual gap: wrapper concurrency with odd capacity, wraps, pressure, and endpoint drop** | **Fixed by test only; implementation remains unchanged.** A long two-thread SPSC test uses capacity 31, mixed write/read chunk lengths, 200,000 ordered samples, deterministic full pressure, concurrent empty pressure, repeated physical wraps, and producer endpoint destruction concurrent with the consumer's final drain. | `concurrent_odd_capacity_wraps_under_full_and_empty_pressure` asserts exact FIFO sequence, complete drain after disconnect, dropped-sample telemetry, and underrun telemetry. Existing callback operations remain bounded copies plus atomics only: no allocation, lock, wait, logging, I/O, or retry was added. |

## Realtime and complexity disposition

- No `relay-rt` callback implementation was changed. Only `crates/relay-rt/tests/ring.rs` gained coverage.
- `DriftEstimator::observe_scheduled_playout` and `ClockRecovery::update` retain fixed-size scalar state and O(1) deterministic work.
- Neither clock update allocates, locks, performs I/O, retries, or depends on an async runtime.
- Long-gap rejection validates all inputs and cadence before mutating any filter, integral, or command state.

## Validation

Executed from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` with the locked dependency graph:

```text
cargo fmt --all
PASS

cargo test -p relay-clock --locked
PASS: 15 passed; 0 failed (plus doc-tests: 0 failed)

cargo test -p relay-rt --test ring --locked
PASS: 9 passed; 0 failed

cargo clippy -p relay-clock -p relay-rt --all-targets --all-features --locked -- -D warnings
PASS: no warnings
```

```text
cargo test --workspace --all-targets --all-features --locked
PASS: all workspace tests passed (including relay-clock 15/15 and relay-rt 9/9)

cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
BLOCKED outside permitted edit scope: relay-resample has two pre-existing warnings promoted to errors:
- crates/relay-resample/src/fixed.rs:10: `clippy::large_enum_variant`
- crates/relay-resample/src/fixed.rs:236: `clippy::manual_is_multiple_of`
The relay-clock/relay-rt narrow clippy gate above is green. No out-of-scope relay-resample or manifest edits were made.
```
