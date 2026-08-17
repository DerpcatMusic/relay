# `relay-resample` implementation evidence

## Scope and disposition

This record covers only the worker-side Phase-1 seed in `crates/relay-resample`. This task did not edit the root workspace manifest. At final validation the crate inherited the root workspace edition, Rust version, lint policy, exact Rubato 4.0.0 dependency, and shared lockfile.

The implementation provides:

- `FixedRatioConverter`, backed by Rubato 4 `Fft<f32>` with `FixedSync::Input`;
- `AdaptiveClockConverter`, backed by Rubato 4 `Async<f32>::new_sinc` with `FixedAsync::Input`;
- the small `WorkerResampler` interface, which exposes next/max input and output frame counts plus output delay and processes caller-owned interleaved buffers;
- mono/stereo validation and the Phase-1 rate matrix `44.1/48/96/192 kHz ↔ 48 kHz` (including the 48-to-48 case);
- finite-sample guards and bounded, time-smoothed adaptive clock correction.

Construction, Rubato filter creation, and caller buffer allocation belong on the decode/resample worker or its control-time setup. The device callback must not construct, process, reset, or destroy these converters.

## Primary sources consulted

Only these two source families were used.

1. **Rubato 4.0.0 primary rustdoc:** [crate guidance](https://docs.rs/rubato/4.0.0/rubato/), [`Fft`](https://docs.rs/rubato/4.0.0/rubato/struct.Fft.html), [`Async`](https://docs.rs/rubato/4.0.0/rubato/struct.Async.html), [`Resampler`](https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html), and [`Adjustable`](https://docs.rs/rubato/4.0.0/rubato/trait.Adjustable.html).
2. **Repository-selected realtime audio rules:** `/home/derpcat/.agents/skills/audio-engineering-principles/SKILL.md` and `/home/derpcat/.agents/skills/audio-dsp/SKILL.md`.

No secondary Rubato tutorials or search-result claims were used.

## API findings applied

Rubato 4 distinguishes two true resampler families:

- `Fft` is the synchronous, fixed-ratio family and is the documented natural choice when rates are locked.
- `Async` is the adjustable family for clocks that can drift. `Async::new_sinc` retains anti-alias filtering; its ratio can change within construction-time relative bounds.

Rubato 4's constructors used here are:

```text
Fft::new(input_rate, output_rate, chunk_size, channels, FixedSync)
Async::new_sinc(initial_ratio, max_relative_ratio, &sinc_parameters,
                chunk_size, channels, FixedAsync)
```

The `Resampler` contract requires callers to query `input_frames_next()` and `output_frames_next()` for each call and exposes `input_frames_max()`, `output_frames_max()`, and `output_delay()`. An asynchronous resampler cannot make both sides fixed because its ratio can change. Both converters therefore fix the input side, while callers allocate output storage once to `output_frames_max * channels` and use the returned `ProcessReport::output_frames` prefix.

`Resampler::process()` allocates an output value. `process_into_buffer()` writes to a caller-provided adapter and is Rubato's documented nonallocating processing path. This implementation exclusively calls `process_into_buffer()` using stack-created `InterleavedSlice` adapters over caller-provided slices. The Rubato `log` feature remains disabled.

`Adjustable::set_resample_ratio(new_ratio, true)` ramps a change over the next chunk. RELAY adds a slower one-pole controller-domain smoother before that Rubato ramp. Each finite requested correction is clamped symmetrically to `AdaptiveClockConfig::max_correction_ppm`; the Rubato construction bound is chosen so the entire symmetric clamp is representable. The API is deliberately named `set_clock_correction_ppm`: packet arrival error and short-term jitter-buffer occupancy are not valid inputs. A clock-recovery component must estimate slow remote-media-clock versus local-monotonic-clock rate error.

## Design and edge behavior

- **Thread model:** soft-realtime worker only; no converter call belongs in the hard-realtime device callback.
- **Allocation:** Rubato and reusable input/output slices are allocated before streaming. Processing does not resize or allocate.
- **Complexity:** finite-input and finite-output scans are linear in interleaved samples. Rubato `Fft` has FFT-style block complexity; `Async::new_sinc` performs bounded sinc interpolation work determined by construction settings. Memory is fixed after construction and proportional to Rubato state plus caller maximum buffers.
- **Numeric policy:** NaN/infinite input and correction requests are rejected before backend state advances. Written output is checked for finiteness.
- **Silence/channel policy:** channels are processed independently; tests prove exact silence and stereo isolation.
- **Latency/frame policy:** startup delay is reported, not hidden. In fixed-input FFT streaming, a bounded tail can remain internally, so long-run produced-frame accounting is checked against the exact rational rate within the advertised maximum output block.
- **Reset:** retains allocations and clears streaming/controller history.
- **End of stream:** this seed exposes streaming chunks and reset, but does not invent an implicit flush policy. The composing audio pipeline must define zero-padding/tail trimming before it claims clip-complete duration.

## Tests

`tests/resample_contract.rs` covers:

1. construction and silent processing for every supported rate direction in mono and stereo, for both converter families;
2. impulse delay, preservation, and finite output;
3. 1 kHz sine energy, finite output, and stereo channel isolation;
4. exact-rate frame-count tracking with a bounded retained streaming tail;
5. NaN input and infinite correction rejection;
6. adaptive positive/negative clamp, one-pole smoothing, Rubato ramping, and ratio bounds;
7. enforcement of maximum preallocated output capacity.

## Validation evidence

Run from the repository root with Rust 1.92.0:

```text
cargo fmt --all -- --check
    passed

cargo check --locked -p relay-resample --all-targets --all-features
    passed

cargo test --locked -p relay-resample --all-targets --all-features
    7 passed; 0 failed

cargo clippy --locked -p relay-resample --all-targets --all-features -- -D warnings
    passed with no warnings
```

The root workspace pins `rubato = "=4.0.0"`; `relay-resample` inherits that dependency and the shared `Cargo.lock` records the resolved graph.

## Potential corrections to plans

1. **Name both Rubato families.** Fixed device/media-rate conversion should select Rubato 4 `Fft`; only clock-drift recovery should select adjustable `Async`. A blanket statement that all conversion uses the asynchronous family is unnecessarily expensive and obscures the two timing problems.
2. **Do not promise two fixed sides for ASRC.** Any plan requiring both fixed input and fixed output per adaptive call conflicts with Rubato 4. The composition layer needs an accumulator on the variable side and must honor next/max frame queries.
3. **Separate clock drift from packet jitter in interfaces and tests.** Ratio correction must come from a slow clock-rate estimator. Network arrival jitter remains a jitter-buffer concern; using it directly to steer ASRC would turn packet timing noise into pitch/rate modulation.
4. **Specify end-of-stream accounting before clip-duration acceptance.** Rubato reports startup delay and supports partial input zero-padding, but a streaming seed has no generic implicit flush. The later audio composition plan must state padding, tail production, trimming, and reset rules.
