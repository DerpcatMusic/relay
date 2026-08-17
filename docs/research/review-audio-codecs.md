# Independent Review: `relay-resample`, `relay-opus`, and `relay-opus-sys`

## Scope and disposition

This was a read-only audit of the three named crates, using the repository's pinned Rubato 4.0.0 implementation and the installed system libopus 1.6.1. The review covered unsafe/FFI ownership, lengths and variadic controls; post-construction allocation behavior; sample-rate, channel, and frame-duration contracts; Opus PLC/FEC sequencing; adaptive ratio direction and bounds; resampler delay/end-of-stream behavior; and numeric test coverage.

**Disposition: changes requested.** I found no memory-safety defect in the present FFI wrapper and no test or Clippy failure. However, the safe decoder does not enforce its advertised fixed frame duration, the resampler API cannot correctly finish a finite stream, and the adaptive correction sign contract is too ambiguous for safe integration. These are functional/integration defects rather than evidence of undefined behavior.

## Severity-ranked findings

### Medium — O1: the fixed-duration Opus decoder silently accepts shorter packet durations

**Evidence.** `DecoderConfig` stores one negotiated `FrameDuration` (`crates/relay-opus/src/lib.rs:166-200`), and the crate advertises a fixed-format boundary (`:1-6`). `decode_packet` passes the configured duration only as libopus's *maximum output capacity* (`:462-487`). `finish_decode` rejects only a result greater than the configured duration (`:501-511`), not a result different from it. Opus packets carry their own duration, so a 5 ms packet decoded by a decoder configured for 20 ms succeeds with 240 samples/channel rather than the configured 960. I reproduced exactly that behavior against the linked libopus 1.6.1 (`opus_encode_float(..., 240, ...)` followed by `opus_decode_float(..., frame_size=960, decode_fec=0)` returned 240).

The returned `DecodedSamples` prevents an out-of-bounds read if every caller honors it, but the result violates the crate's fixed-frame invariant and can turn a negotiated-format mismatch into a timing discontinuity or stale suffix in downstream fixed-period code.

**Potential correction.** For ordinary packet decode, require `samples_per_channel == config.frame_duration.samples_per_channel()` and return a dedicated `UnexpectedDecodedDuration { expected, actual }` error otherwise. If variable packet durations are intended, remove the fixed-duration claim/config invariant and make maximum duration explicit instead. Add cross-duration negative tests (5→20 and 10→20 at minimum).

### Medium — R1: finite streams cannot be finalized without losing input remainder and filter tail

**Evidence.** `WorkerResampler::process_interleaved` requires exactly `input_frames_next * channels` samples (`crates/relay-resample/src/lib.rs:45-60`, validation at `:142-165`). The public interface has no partial-input, finish, drain, or end-of-stream operation. Both Rubato backends retain algorithmic history; `output_delay` is exposed (`:19-34`, `:131-139`) but no operation zero-pads the final partial block, drains the retained tail, or reports how much padded output to trim. The fixed-ratio test explicitly observes a retained tail (`crates/relay-resample/tests/resample_contract.rs:119-141`) without exercising recovery of it.

Consequently, a finite input whose length is not an exact sequence of required chunks cannot be completely converted through this API, and even exact chunks end with delayed samples still resident in the filter.

**Potential correction.** Add an explicit finalization protocol: accept a final partial frame count (zero-pad internally without reallocating), drain enough zero-input chunks to emit the filter tail, and return valid output counts plus exact leading/trailing trim metadata. Alternatively, declare the interface infinite-stream-only and provide a separate offline/finite-stream adapter using Rubato's partial-processing contract. Test impulses at the first and last source frame and arbitrary non-chunk-aligned lengths for every supported rate direction.

### Medium — R2: the adaptive ppm sign contract can invert clock recovery at integration

**Evidence.** Rubato's ratio is output frames per input frame; this crate applies
`nominal_ratio * (1 + correction_ppm / 1_000_000)` (`crates/relay-resample/src/adaptive.rs:129-142`). That arithmetic is internally consistent with `relay-clock`'s published output/input multiplier. But the type documentation says to feed the method a “slow clock-rate estimate derived from remote media-clock progression versus a local monotonic clock” (`:38-42`). A positive *remote drift estimate* means the remote clock is fast and must be negated by clock recovery, while a positive *resampler correction* means produce more output frames per input. The tests only establish that positive input raises the ratio (`crates/relay-resample/tests/resample_contract.rs:163-196`); they do not establish the integration sign with `relay-clock`.

**Potential correction.** Specify on `set_clock_correction_ppm` that positive values increase the output/input ratio and that the method accepts `ClockRecovery`'s correction, not a raw remote-drift estimate. Prefer a named value type shared with `relay-clock` (for example `OutputPerInputCorrectionPpm`) over a bare `f64`. Add a cross-crate closed-loop test: positive remote drift must produce negative recovery correction, a ratio below nominal, and bounded ring-fill error.

### Low — O2: the FEC test exercises fallback PLC, not actual FEC recovery or the required two-step sequence

**Evidence.** The test enables FEC, encodes only one silence packet, and immediately calls `decode_fec` on that first packet (`crates/relay-opus/src/lib.rs:631-655`). A first packet cannot carry recovery data for an earlier encoded packet, and silence does not distinguish recovered audio from PLC. Libopus documents that `decode_fec=1` falls back to loss decoding when no FEC is present. The public method correctly says it recovers the previous lost frame from the following packet (`:436-445`), but it does not state the caller must then normally decode that same following packet to obtain the current frame.

**Potential correction.** Encode at least two non-silent, distinguishable voice-mode frames with FEC and a nonzero loss hint; drop the first; decode FEC from the second; then decode the second normally. Assert both calls return the configured duration, remain finite, and produce distinct expected energy/correlation. Document the sequence and the one-packet latency explicitly. Keep a separate test proving no-FEC fallback produces PLC.

### Low — T1: real-time allocation and numeric-quality claims are source-audited but not regression-tested

**Evidence.** The wrappers use caller-owned slices and the Rubato `process_into_buffer` path (`crates/relay-resample/src/lib.rs:168-193`; `crates/relay-opus-sys/src/lib.rs:114-164,213-250`). Rubato's processing implementations use preallocated internal buffers, and libopus state is allocated at construction. I found no post-construction Rust allocation, lock, logging, file I/O, or explicit syscall in the reviewed processing paths. Current tests, however, do not install a counting/failing allocator around steady-state calls. Numeric assertions are mostly finiteness/nonzero/channel-isolation checks (`crates/relay-resample/tests/resample_contract.rs:64-141`; `crates/relay-opus/src/lib.rs:542-579`) and do not measure resampler gain, passband error, alias rejection, phase continuity, or long-run adaptive count accuracy across non-unity rate pairs.

**Potential correction.** Add an allocation-counting integration test that permits construction allocations, warms each object, then asserts zero allocations for encode/decode/PLC/FEC/controls/reset and both resampler process/reset paths. Add deterministic DC, impulse-area, swept-sine/passband, stopband/alias, long-duration sample-count, stereo-isolation, and NaN/Inf tests with explicit tolerances. Run those tests in optimized builds as well as debug builds.

### Low — R3: supported unity-ratio construction adds avoidable latency

**Evidence.** `48_000 → 48_000` is explicitly supported (`crates/relay-resample/tests/resample_contract.rs:20-28`) but `FixedRatioConverter` still constructs Rubato's FFT backend (`crates/relay-resample/src/fixed.rs:18-35`). With the pinned Rubato defaults and a 480-frame chunk, the unity-rate FFT block has 240 output frames of delay (5 ms). The adaptive default 256-tap sinc has approximately 128 output frames of delay at unity (2.67 ms). The delay is correctly exposed through `FrameRequirements`, so this is not hidden memory/state corruption, but no test or API policy prevents paying it where no conversion is needed.

**Potential correction.** Use an allocation-free passthrough implementation for fixed 48 kHz→48 kHz, or reject/bypass unity construction at the call site. If adaptive clock steering is required, retain the adaptive backend but document and budget its startup delay. Add exact latency assertions so dependency upgrades cannot silently change the budget.

## FFI and invariant audit notes (no defect found)

- **Ownership/lifetimes:** `NonNull` owns each create result, destruction occurs once in `Drop`, all stateful calls require `&mut self`, and `PhantomData<Rc<()>>` plus the explicit `Send` implementation keeps each wrapper non-`Sync` (`crates/relay-opus-sys/src/lib.rs:77-85,167-176,178-185,253-262`). No borrowed pointer escapes a call.
- **Length conversions:** encode checks exact `frame_size * channels`, decode checks at least that capacity, multiplication is checked, slice lengths are converted to `i32`, and negative libopus results remain errors (`:114-145,213-250,271-280`). The safe layer caps packets at 4000 bytes (`crates/relay-opus/src/lib.rs:16-19,350-376,462-487`).
- **Variadic controls:** request values 4012 and 4014 match libopus 1.6.1; both calls pass exactly one `c_int`, matching C default promotion and the control macros (`crates/relay-opus-sys/src/lib.rs:28-29,147-164`). The 0/1 FEC and 0–100 loss values are valid.
- **PLC/FEC call shape:** PLC uses `(data=NULL, len=0, decode_fec=0)` and FEC uses the following nonempty packet with `decode_fec=1`; configured 5/10/20 ms sizes are valid 2.5 ms multiples (`:213-250`; safe facade at `crates/relay-opus/src/lib.rs:431-488`).
- **Adaptive bounds:** the configured maximum is finite and at most 10%; the reciprocal Rubato construction bound contains both the negative and positive clamps (`crates/relay-resample/src/adaptive.rs:8-10,54-85`). Per-call validation precedes smoothing/backend advancement (`:151-160`).
- **Post-construction behavior:** reviewed processing paths reuse caller/internal storage. This satisfies worker-thread predictability and would also satisfy the no-allocation portion of an audio callback contract, but these crates appropriately document worker/control-thread construction and do not claim that Rubato FFT/sinc work belongs on a hard-real-time device callback.

## Validation performed

Environment: `pkg-config --modversion opus` reported **1.6.1**.

```text
cargo test -p relay-opus-sys -p relay-opus -p relay-resample
  relay-opus:       8 passed
  relay-opus-sys:   0 tests
  relay-resample:   7 passed
  doc tests:        0 failures

cargo clippy -p relay-opus-sys -p relay-opus -p relay-resample --all-targets -- -D warnings
  passed, no diagnostics

cargo test --release -p relay-opus-sys -p relay-opus -p relay-resample
  relay-opus:       8 passed
  relay-opus-sys:   0 tests
  relay-resample:   7 passed
  doc tests:        0 failures
```

A read-only ctypes probe against the same installed libopus confirmed finding O1: a stereo 48 kHz 5 ms packet decoded with 20 ms output capacity returned 240 samples/channel.

## Recommended CI workflow

Keep the current narrow gates as a required Linux job with the system development package for libopus installed:

```sh
pkg-config --modversion opus
cargo test --locked -p relay-opus-sys -p relay-opus -p relay-resample
cargo test --locked --release -p relay-opus-sys -p relay-opus -p relay-resample
cargo clippy --locked -p relay-opus-sys -p relay-opus -p relay-resample --all-targets -- -D warnings
```

Add a second required regression job once the corrections above exist. It should run: (1) exact cross-duration rejection; (2) genuine two-packet FEC recovery followed by normal decode of the following packet; (3) zero-allocation steady-state checks; (4) non-chunk-aligned EOS/drain and delay trimming; and (5) cross-crate clock-recovery sign/convergence plus resampler frequency/count tolerances. Keep the libopus version visible in CI logs so a system-library upgrade is attributable. No repository workflow file was changed in this read-only review.

## Primary sources

1. libopus 1.6 API declarations and contract documentation (`opus.h`, `opus_defines.h`): <https://opus-codec.org/docs/opus_api-1.6/opus_8h_source.html> and <https://opus-codec.org/docs/opus_api-1.6/opus__defines_8h_source.html>.
2. Rubato 4.0.0 `Resampler`/`Adjustable` API and implementation source: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html> and <https://docs.rs/rubato/4.0.0/src/rubato/asynchro.rs.html>.
