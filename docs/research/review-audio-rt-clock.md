# Independent Review: `relay-rt` and `relay-clock`

## Disposition

**Changes requested for `relay-clock`; no implementation defect found in `relay-rt`.**

There are no critical findings. The clock estimator has one **high-severity** design defect: when its local timestamps are packet-arrival times, a change of only 1 ms in path delay across the default two-second measurement window is interpreted as about 500 ppm of clock drift. The existing jitter test stops before the first measurement and therefore does not exercise this failure. The recovery controller also needs clearer jitter isolation and stronger nonlinear-control tests before it should drive a production ASRC.

`relay-rt`'s callback operations are bounded and allocation-free by inspection. Construction owns all allocation; callback operations copy `f32` slices and perform atomics only. Its acquire/release payload publication is delegated to pinned `rtrb` 0.3.4. Endpoint destruction remains deliberately outside the callback-safe contract and is documented as such.

## Scope and thread model

- `AudioProducer::write` / `AudioConsumer::read`: hard realtime callback candidates; no allocation, lock, I/O, logging, retry, or heap-owning payload destruction is permitted.
- `DriftEstimator` / `ClockRecovery`: documented worker-side O(1) state machines, not hard callback code (`crates/relay-clock/src/lib.rs:1-11`, `crates/relay-clock/src/recovery.rs:93-102`).
- Reviewed pinned dependency and capacity/ordering behavior because `relay-rt` delegates storage and publication to `rtrb = 0.3.4` (`Cargo.toml:21`, `Cargo.lock:210-215`).

## Severity-ranked findings

### High — raw packet-arrival endpoint delay is indistinguishable from clock drift

**Evidence**

`DriftEstimator::observe` computes a point-to-point rate from exactly one anchor and the first observation at least `observation_window_seconds` later (`crates/relay-clock/src/estimator.rs:148-176`). It then clamps that measurement and, for the first window, publishes the clamped value without EWMA attenuation (`crates/relay-clock/src/estimator.rs:180-191`). The documentation claims anchored windows prevent an individual early/late packet from becoming an immediate ASRC command (`crates/relay-clock/src/estimator.rs:89-99`), but a longer window only scales the error; it does not separate clock skew from changing network delay.

For a nominal 48 kHz clock and the default two-second window:

- remote delta: 96,000 samples;
- local delta with a +1 ms endpoint delay change: 2.001 s;
- measured drift: `(96_000 / 2.001 / 48_000 - 1) * 1e6 = -499.75 ppm`.

Thus 1 ms of path-delay change nearly reaches the configured ±500 ppm limit; 3 ms produces about -1,498 ppm and is clamped to -500 ppm. RFC 3550 defines network interarrival jitter from variation in relative transit time, confirming that this arrival-time component is a network-delay signal, not sender clock rate by itself (section 6.4.1 and Appendix A.8).

The only jitter test does not cross the configured window: it observes packets `1..=199` at 10 ms spacing from an anchor at 3 ms, so its largest local delta is 1.984 s, below the two-second threshold (`crates/relay-clock/src/estimator.rs:238-256`). Its assertion of zero estimates therefore proves only the warm-up guard, not jitter rejection.

**Impact**

A routine path-delay step can publish a full-scale false drift estimate. With the default 25 ppm/s slew this can steer the ASRC for seconds, perturb output-ring fill, and make network jitter leak into pitch/rate recovery despite the crate's stated separation.

**Potential correction**

1. Specify that `local_time_seconds` must not be raw packet-arrival time unless the estimator is made delay-robust.
2. Prefer a sender-clock mapping (for example, an RTP/RTCP sender-report mapping) when available. Otherwise fit remote extended position against many arrival samples over a substantially longer horizon using robust regression / delay-outlier rejection rather than two endpoints.
3. Preserve the separate jitter buffer and reset both estimator and controller on epoch changes.
4. Add deterministic tests that run across many complete windows with: random/phase-shifted arrival jitter, a 1–10 ms delay step, asymmetric delay ramps, true drift plus jitter, reordering/duplicates, and a bound ensuring the estimate does not saturate for a nominal remote clock.

### Medium — instantaneous ring-fill error is a direct frequency-modulation input

**Evidence**

The recovery documentation calls ring-fill error an “even slower PI trim” and says packet jitter must not directly frequency-modulate the ASRC (`crates/relay-clock/src/recovery.rs:93-102`). The implementation, however, clamps the current single fill sample and immediately applies `Kp * fill_error` to the ratio target (`crates/relay-clock/src/recovery.rs:160-172`, `184-198`). There is no low-pass filter, deadband, cadence contract, target-fill hysteresis, or sampling-phase requirement.

With defaults, a 480-frame occupancy deviation contributes 24 ppm immediately to the requested target; 960 frames contributes 48 ppm (`0.05 ppm/frame`, `crates/relay-clock/src/recovery.rs:22-31`). Slew limiting reduces the step but does not distinguish sustained clock error from block/packet-quantized occupancy motion. No test injects zero-mean occupancy jitter or worker scheduling jitter; the sole closed-loop test uses a noiseless scalar plant and fixed 100 ms cadence (`crates/relay-clock/src/recovery.rs:250-285`).

**Impact**

Depending on where fill is sampled, decode/resampler block cadence and packet bursts can modulate the correction command even when the clocks agree.

**Potential correction**

Define one stable fill sampling point and update cadence. Feed the PI loop a low-pass-filtered/averaged fill error (or add an internal time-based filter and a small deadband) whose bandwidth is explicitly below packet/block jitter. Add tests with nominal clocks and realistic quantized fill sawtooth, bursty worker scheduling, and randomized `dt`; bound peak and RMS correction as well as long-term fill.

### Medium — anti-windup ignores slew saturation and long update gaps integrate stale error

**Evidence**

The controller blocks integration only when the *requested amplitude* would exceed the output limit (`crates/relay-clock/src/recovery.rs:165-182`). It updates the integral before calculating whether the actuator is slew-limited (`crates/relay-clock/src/recovery.rs:184-203`). Consequently the integral continues accumulating while `correction_ppm` cannot reach the target due to slew limiting.

Also, any positive finite `elapsed_seconds` is accepted (`crates/relay-clock/src/recovery.rs:143-154`). The current fill sample is integrated over that entire interval, and a sufficiently long gap permits movement across the complete correction range (`crates/relay-clock/src/recovery.rs:189-198`). This treats a sample taken after a worker stall as though it represented the whole missing interval.

**Impact**

Large fill steps or delayed worker updates can accumulate avoidable integral state, causing overshoot and slow unwinding after the actuator catches up. The existing convergence test does not exercise saturation reversal, variable cadence, or delayed updates.

**Potential correction**

Calculate the applied actuator command before committing the integral and use conditional integration or back-calculation based on the difference between requested and applied correction, including slew saturation. Add a maximum trusted controller interval; reject, subdivide with known historical error, or reset on larger gaps. Test sustained saturation followed by error reversal, both correction limits, variable `dt`, and a long scheduling pause.

### Low — feed-forward ratio is a first-order approximation, but its test plant assumes the same approximation

**Evidence**

The estimator reports remote rate as `1 + d`, while recovery emits an output/input multiplier `1 - d` at zero fill (`crates/relay-clock/src/estimator.rs:174-176`; `crates/relay-clock/src/recovery.rs:82-88`, `184-206`). The exact reciprocal for an output/input ratio is `1 / (1 + d)`. At +500 ppm, `1 - d` leaves about -0.25 ppm residual. The convergence test asserts exact negation (`crates/relay-clock/src/recovery.rs:226-247`), and the closed-loop plant uses the same first-order `d + c` approximation (`crates/relay-clock/src/recovery.rs:268-271`), so neither test can expose this residual.

**Impact**

Small (about 0.25 ppm at the configured extreme), normally removable by the fill integrator, but the API currently presents `ratio_multiplier` as ready to apply rather than explicitly approximate.

**Potential correction**

Either compute the reciprocal feed-forward multiplier and layer PI trim in ratio/ppm space consistently, or document the first-order approximation and tolerated residual. Exercise an exact rate/ratio plant in tests.

### Low — `saturated` omits integral clamp/suppression

**Evidence**

`ClockRecoveryOutput::saturated` promises to report whether amplitude or slew saturation affected the update (`crates/relay-clock/src/recovery.rs:77-91`). The integral can be clamped or deliberately not committed (`crates/relay-clock/src/recovery.rs:165-182`), but the reported flag only compares input/output clamps and slew movement (`crates/relay-clock/src/recovery.rs:200-207`). An integral limit can therefore affect an update while `saturated` remains false.

**Potential correction**

Include `candidate_integral != unclamped_integral` and anti-windup suppression in the flag, or split telemetry into `input_clamped`, `integral_limited`, `amplitude_limited`, and `slew_limited`. Add one assertion for each condition.

## `relay-rt` realtime and atomic audit

No implementation defect was identified.

- Allocation occurs only in `audio_ring`: ring storage and shared counters are created at `crates/relay-rt/src/ring.rs:18-41`.
- `write` performs an abandonment observation, a bounded entire-slice copy, and relaxed diagnostic increment on failure (`crates/relay-rt/src/ring.rs:71-101`; `crates/relay-rt/src/counters.rs:47-56`).
- `read` performs a bounded partial-slice copy, an abandonment observation, and relaxed diagnostic increments on underrun (`crates/relay-rt/src/ring.rs:151-182`). No output-tail zeroing is hidden; the caller owns concealment (`crates/relay-rt/src/lib.rs:19-22`).
- Payload is `f32`; per-item consumption has no destructor. `Arc` cloning occurs only during construction (`crates/relay-rt/src/ring.rs:25-39`).
- The last endpoint can deallocate `rtrb` storage and the last counters owner can deallocate counters. The crate correctly excludes endpoint/metrics destruction from the callback contract and requires a device-stop acknowledgement first (`crates/relay-rt/src/lib.rs:24-29`; `crates/relay-rt/src/ring.rs:61-69`, `141-149`).
- Diagnostic counters use native 64-bit relaxed atomics and explicitly disclaim transactional coherence (`crates/relay-rt/src/counters.rs:4-11`, `20-25`, `40-56`). Relaxed is sufficient for independent telemetry counts; it is not used for payload publication.
- The pinned `rtrb` ring uses acquire loads and release stores for SPSC index publication and uses all configured capacity slots. `is_abandoned` is based on `Arc::strong_count` and is not a synchronization barrier; relay correctly documents it as diagnostic rather than reclamation synchronization (`crates/relay-rt/src/ring.rs:103-118`, `184-200`; `crates/relay-rt/src/lib.rs:24-29`).
- Scalar-sample capacity and all-or-drop semantics are documented (`crates/relay-rt/src/lib.rs:1-22`). Zero is rejected (`crates/relay-rt/src/ring.rs:18-25`). Capacity wrap and full-slice rejection have sequential coverage (`crates/relay-rt/tests/ring.rs:8-78`).

**Residual test gap:** the wrapper tests are all single-threaded. Add a long concurrent SPSC sequence test with an odd capacity, repeated physical wraps, full/empty pressure, and concurrent opposite-end drop. Also consider allocator-instrumented callback-path tests and compile-time endpoint trait assertions. These would validate integration assumptions; they do not replace the pinned dependency's own concurrency proof/tests.

## Validation performed

From `/mnt/Windows11/DEV_PROJECTS/Repos/relay`:

```text
cargo test --workspace --all-targets --all-features --locked
PASS (exit 0)
- relay-clock: 12 passed
- relay-rt integration tests: 8 passed
- all other workspace tests passed

cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
PASS (exit 0; no warnings)
```

The review did not edit implementation files.

## Primary sources (2)

1. H. Schulzrinne et al., **RFC 3550: RTP: A Transport Protocol for Real-Time Applications**, especially section 6.4.1 and Appendix A.8 (interarrival jitter / relative transit-time variation): <https://www.rfc-editor.org/rfc/rfc3550>
2. `rtrb` 0.3.4 pinned dependency documentation and source (wait-free SPSC contract, acquire/release indices, abandonment semantics): <https://docs.rs/rtrb/0.3.4/rtrb/> and <https://docs.rs/rtrb/0.3.4/src/rtrb/lib.rs.html>
