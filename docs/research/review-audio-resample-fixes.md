# Audio Resampler Review Fixes

## Scope

This record closes resampler findings R1, R2, R3, and the resampler portion of T1 from `review-audio-codecs.md`. Changes are limited to `crates/relay-resample` and this evidence file. Rubato remains pinned at workspace version 4.0.0; no dependency or root-workspace file changed.

## Finding disposition

### R1 — finite streams could not recover a partial final chunk and filter tail: fixed

`WorkerResampler` is now explicitly documented as an infinite/live-stream contract. It intentionally accepts only complete chunks, retains history for the next call, and has no end-of-stream operation.

`FiniteFixedRatioConverter` is the separate finite-stream adapter. Construction allocates the backend and scratch storage; `process_interleaved` uses caller-owned input/output storage and performs no explicit heap allocation. It resets the backend, submits every source frame, submits the non-chunk-aligned final block through Rubato 4.0 `Indexing::partial_len(valid)`, and drains with `partial_len(0)` until the useful tail is present.

`FiniteFrameRequirements` reports the exact destination length and conservative workspace length. `FiniteProcessReport` reports exact valid input, raw generated output, useful output, leading trim, trailing trim, and the half-open valid frame range. Counts use checked integer ceiling arithmetic rather than floating-point rounding. The raw identity is asserted by tests:

`generated_output_frames = leading_trim_frames + output_frames + trailing_trim_frames`.

Non-chunk-aligned first- and last-frame impulses are tested for every supported direction: 44.1↔48 kHz, 48↔96 kHz, 48↔192 kHz, and 48→48 kHz. Validation tests cover incomplete interleaved frames, NaN, undersized workspace, exact useful length, and long finite streams.

### R2 — correction sign could be inverted at the clock/resampler boundary: fixed

The adaptive setter now requires `OutputInputRatioCorrectionPpm`, a named validated value. Its sole meaning is correction of the **output-frames/input-frames** ratio:

`ratio = nominal_output_per_input * (1 + correction_ppm / 1_000_000)`.

Positive correction increases output per input; negative correction decreases it. Raw remote drift and packet jitter are explicitly rejected as API concepts. `from_ratio_multiplier` accepts the wire-compatible semantic value published by `relay_clock::ClockRecoveryOutput::ratio_multiplier`; this prevents treating a raw positive remote-drift observation as a positive resampler command.

The cross-crate-compatible sign test models relay-clock's fast-remote result (`ratio_multiplier = 0.9998`), proves it becomes `-200 ppm`, feeds that named command to the adaptive converter, and proves the applied ratio moves below nominal. Non-finite and non-positive multiplier validation is also tested. Existing smoothing and symmetric configured clamps remain unchanged.

### R3 — fixed 48 kHz unity conversion added avoidable delay: fixed

`FixedRatioConverter` and `FiniteFixedRatioConverter` select an exact passthrough backend when input and output are both 48 kHz. The live path copies between caller buffers, performs no processing-time allocation, and reports exactly zero algorithmic delay. Tests assert bit-exact mono/stereo output, zero delay, exact frame counts, and stable caller buffer pointers/capacities. Adaptive 48→48 remains a sinc backend because it must retain the ability to steer the ratio; its delay remains reported and budgetable.

### T1 — RT/allocation and numeric quality lacked regression evidence: fixed for relay-resample

Both live and finite processing continue to use preconstructed state and caller-owned buffers. Non-unity processing calls Rubato's preallocated `process_into_buffer`; unity processing is a direct slice copy. Pointer/capacity stability is asserted over long finite processing. The crate forbids unsafe code, so an in-crate global allocator interceptor cannot be implemented; Rubato 4.0's processing path remains source-audited for preallocation, while tests protect the public caller-buffer contract. This is the resampler disposition only; codec allocation evidence is outside this task.

Deterministic tests now cover:

- DC gain and stereo isolation for every supported rate pair;
- centered impulse area against the output/input density ratio (0.5% tolerance);
- 1 kHz passband RMS gain (0.2% tolerance);
- above-destination-Nyquist alias rejection for all downsampling directions (-40 dB conservative bound);
- first/last boundary impulses and non-chunk-aligned finite completion;
- exact 30-second-plus-remainder integer output counts;
- finite output and NaN/input-layout/workspace validation;
- mono/stereo supported-rate processing and adaptive correction bounds.

The tolerances are intentionally wider than observed pinned-backend error while narrow enough to catch normalization, passband, alias-filter, or channel-routing regressions. Tests run in both debug and optimized profiles.

## Real-time and ownership contract

Construction belongs on a worker/control thread. Processing belongs on the decode/resample worker, not a hard-real-time device callback. All input and output storage is caller-owned and preallocated. The finite adapter requires its workspace to be sized before processing; it returns a view range rather than allocating or compacting output. Adaptive correction remains bounded and one-pole smoothed before reaching Rubato.

## Validation

Executed from the repository root with the pinned lockfile:

- `cargo fmt --all -- --check` — passed.
- `cargo test --locked -p relay-resample --all-targets` — passed, 15 integration tests.
- `cargo test --release --locked -p relay-resample --all-targets` — passed, 15 integration tests.
- `cargo clippy --locked -p relay-resample --all-targets -- -D warnings` — passed.
- `cargo check --locked --workspace --all-targets --all-features` — passed.

Clippy's enum-size finding was resolved by boxing the construction-time Rubato backend; the process path does not allocate. The interleaved validation uses `is_multiple_of` as required by the pinned toolchain.

## Potential master-plan corrections

1. Describe chunked `WorkerResampler` APIs as live/infinite contracts, never as implicit finite-file converters.
2. Require a separate partial/finalize/drain adapter with exact trim metadata for finite media.
3. Standardize ASRC commands as named output/input ratio corrections or ratio multipliers; never pass raw remote drift directly.
4. Require zero-delay bypass for fixed unity conversion while retaining explicit adaptive-delay budgeting where steering is needed.
5. Keep deterministic debug and release numeric tests with documented tolerances alongside preallocation/source-audit evidence.
