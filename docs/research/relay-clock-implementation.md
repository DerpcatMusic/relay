# relay-clock — Research and implementation evidence

**Date:** 2026-08-16  
**Task owner:** Phase-1 relay-clock implementation agent  
**Status:** Complete

## Scope

Implement only `crates/relay-clock`: deterministic worker-thread estimation of a remote sample clock relative to caller-provided monotonic local time, plus a bounded feed-forward/PI recovery controller that trims an output/input asynchronous-resampler ratio using the drift estimate and slow ring-fill error.

Non-goals: RTP timestamp unwrapping, packet ordering/jitter buffering, resampling itself, platform clocks, async runtimes, device callbacks, transport integration, and root-workspace admission.

## Acceptance criteria

- [x] Estimate -250, -100, -20, 0, +20, +100, and +250 ppm deterministically.
- [x] Do not emit packet-interval ASRC commands; use multi-second anchored windows and EWMA smoothing.
- [x] Combine the slow estimate with bounded ring-fill PI trim.
- [x] Clamp measurements, ring error, integral state, final correction, and correction slew.
- [x] Reject non-finite configuration/input and non-positive local intervals before state mutation.
- [x] Define explicit and detected discontinuity/reset behavior.
- [x] Test convergence, boundedness, discontinuity, NaN, and zero intervals.
- [x] Keep the crate dependency-free and isolated from async/platform APIs.

## Sources consulted

Research stopped after these three primary/upstream sources.

| Source | Why it is authoritative | Accessed |
|---|---|---|
| [RFC 3550 §5.1 and §6.4.1](https://www.rfc-editor.org/rfc/rfc3550.html) | IETF RTP specification: defines the sampling-instant timestamp clock independently from wall-clock time and defines packet interarrival jitter from transit-time variation. | 2026-08-16 |
| [RFC 5905 §11](https://www.rfc-editor.org/rfc/rfc5905.html#section-11) | IETF NTPv4 specification: describes PLL/FLL clock discipline as a filtered feedback loop and distinguishes network-jitter-dominated phase updates from oscillator wander/frequency correction. | 2026-08-16 |
| [Rubato `Resampler` / adjustable ratio API](https://docs.rs/rubato/latest/rubato/trait.Resampler.html) | Upstream resampler documentation: asynchronous resamplers expose a runtime-adjustable resampling ratio, the eventual consumer of this crate's bounded command. | 2026-08-16 |

## Findings

1. RTP timestamp progression describes media sampling time; arrival-time variation is separately measured as interarrival jitter. Therefore adjacent packet transit deltas are not suitable direct ASRC commands. `DriftEstimator` anchors at least two seconds of remote progression against local monotonic time and only then emits a measurement; an EWMA further reduces endpoint noise.
2. Clock recovery is a feedback-control problem, not a packet scheduler. The implementation uses slow frequency feed-forward from the drift estimator and PI trim from output-ring phase/fill error, with slew limiting and anti-windup.
3. Adjustable ASRC APIs require an unambiguous ratio convention. This crate defines `ratio_multiplier` for an **output frames / input frames** nominal ratio. Positive remote drift or positive `current - target` ring error yields a negative correction so the receiver consumes remote input faster relative to local output.
4. RTP's raw 32-bit timestamp is not accepted directly. The transport boundary must unwrap it to an extended `u64` sample position and reset on an SSRC/epoch change. This keeps wrap policy out of the controller and makes regression detection deterministic.
5. Worker-side `update`/`observe` operations are O(1), allocation-free, lock-free, and I/O-free after construction. The crate intentionally contains no clock reads; caller-provided time makes tests and replay deterministic.

## Potential corrections to the master plan

1. **Make ratio direction explicit.** The master plan names `resample_ratio_adjustment_ppm` but does not state whether it adjusts output/input or input/output. This implementation uses output/input; integration must preserve that convention or invert it exactly once. **Disposition:** applied in API documentation and tests; recommend carrying the convention into the resampler integration contract.
2. **Do not feed raw RTP timestamps directly into `relay-clock`.** The master-plan input list says “RTP timestamp,” while ADR 0004 separately requires wrap handling. Clock estimation needs an extended sample position plus an explicit reset at SSRC/restart/seek discontinuities. **Disposition:** applied as a documented caller responsibility with automatic regression/stall detection.
3. **Treat packet arrival jitter and clock drift as separate signals.** The diagram could be read as allowing every packet-arrival delta to drive the PLL. **Disposition:** implemented only as long-window estimation; packet jitter remains a jitter-buffer concern, consistent with the plan's prose.

## Decisions applied

- `DriftEstimator` defaults to 48 kHz, a two-second minimum measurement window, 0.2 EWMA weight, and ±500 ppm measurement saturation.
- Positive estimated ppm means the remote clock progresses faster than nominal relative to local monotonic time.
- `ClockRecovery` computes `-drift - Kp*fill_error - integral`, with `fill_error = current - target`; defaults bound output to ±500 ppm and slew to 25 ppm/s.
- The integrator is bounded and conditionally disabled when it would push farther into output saturation.
- Non-finite values and zero/negative elapsed intervals return `ClockError` without mutation. Finite extremes saturate.
- Remote sample regression/stall begins a new estimator epoch and clears its estimate. Known transport discontinuities call `DriftEstimator::reset()` and `ClockRecovery::reset()` together; recovery resumes from nominal with slew limiting.
- The crate has an empty local `[workspace]` only so it can be validated without editing the root workspace, as required by this focused task.

## Validation evidence

Rust toolchain: repository-pinned Rust 1.92.0. Exact commands were run from the repository root.

```text
$ cargo test --manifest-path crates/relay-clock/Cargo.toml
running 12 tests
...
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Doc-tests relay_clock: 0 passed; 0 failed

$ cargo clippy --manifest-path crates/relay-clock/Cargo.toml --all-targets -- -D warnings
Finished `dev` profile ...

$ cargo fmt --manifest-path crates/relay-clock/Cargo.toml -- --check
(no output; exit status 0)
```

Coverage includes all seven required drift values, long-window behavior under alternating packet jitter, estimator saturation, remote regression and explicit reset, NaN and zero local intervals, controller convergence/sign convention, 10-hour virtual closed-loop boundedness with a 50 ppm estimator residual, slew/amplitude bounds, controller discontinuity reset, and invalid configuration/input.

## Deferred follow-ups

- Admit all Phase-1 crates to the root workspace together and remove their temporary nested `[workspace]` markers.
- Integrate the ratio convention with `relay-resample`/Rubato and verify whether that adapter expects output/input or its inverse.
- Add end-to-end RTP timestamp unwrapping and explicit SSRC/seek/restart reset signaling at the transport boundary.
- Run the master plan's full 12- and 24-virtual-hour pipeline soak once jitter, decoded-sample progression, resampling, and bounded rings are composed.
