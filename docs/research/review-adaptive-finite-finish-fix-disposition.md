# Adaptive Finite Finish Fix Disposition

**Disposition: BLOCKED**  
**Functional C1 disposition:** cleared; the former valid-input Rubato panic did not reproduce in debug or release, and the expanded opposite-ratio/stereo/alternating matrix passed.  
**Blocking residual:** the new allocator dev-dependency violates the repository's deny-wildcards policy, so the mandatory cargo-deny CI gate fails.

## Scope

Independent read-only review of the adaptive finite-finish fix in `relay-resample`, the pinned `rubato = 4.0.0` implementation, and the new allocation-counter dev crate. Production and test source were not edited; only this review record was written.

The repository has no useful tracked baseline for these files: most of the workspace, including `relay-resample`, is untracked in the current checkout. The disposition therefore audits the present tree rather than claiming a commit-to-commit diff.

## Finding

### High — new allocator path dependency is an implicit wildcard and fails the blocking dependency-policy gate

`crates/relay-resample/Cargo.toml:16` declares:

```toml
relay-resample-test-allocator = { path = "tests/allocation-counter" }
```

Cargo metadata resolves that omitted version as `req = "*"`. The repository explicitly sets `wildcards = "deny"`, `just check` includes `cargo deny check`, and Linux CI runs the locked four-family cargo-deny command as a failing step. The exact CI command now exits 2:

```text
error[wildcard]: found 1 wildcard dependency for crate 'relay-resample'
  crates/relay-resample/Cargo.toml:16:33
advisories ok, bans FAILED, licenses ok, sources ok
```

This is introduced inside the reviewed fix scope, not an unrelated workspace failure. It prevents a PASS disposition even though the adaptive panic itself is fixed. Declare the path dependency's exact local version (the package is `0.0.0`) in addition to its path, consistent with the repository's existing internal-edge policy, then rerun the locked cargo-deny command.

## C1 exact panic reproduction

The exact former blocker at `crates/relay-resample/tests/adaptive_finish.rs:264` was rerun without catch/unwind in both profiles:

- stereo, `48_000 -> 48_000`, `C = 240`;
- admitted `+100_000 ppm` live transaction followed by `-100_000 ppm` terminal transaction;
- one-microsecond smoother time constant, so the reversal is effectively maximal;
- full valid final transaction.

It passed in debug and release. The larger regression at `adaptive_finish.rs:298` also passed in the full debug and release package suites. That matrix covers all seven supported pairs involving the 48 kHz media boundary, stereo, `C = 240/480/960`, both first-sign directions, 1/2/7-block alternating histories, opposite finish target, and valid prefixes `1`, `C - 1`, and `C`. No Rubato indexing panic occurred and every reported generated sample was finite.

## Mathematical private-phase bound audit

The new construction at `adaptive.rs:163-224` is a derived private-workspace bound rather than another empirical guard.

For public chunk `C`, sinc length `L`, retained phase `x`, start/target ratios `r_s`/`r_t`, arithmetic mean `A`, and `y = C - L - 1 - x`, pinned Rubato 4 computes the fixed-input output count

```text
M = floor(y A)
```

(`asynchro.rs:377-389`). Its processing loop instead advances phase by the linearly ramped reciprocal ratios (`asynchro.rs:500-504`, `asynchro_sinc.rs:468-470`). Thus

```text
D = M H + delta/2,
H = (1/r_s + 1/r_t)/2,
delta = 1/r_t - 1/r_s.
```

The source proof's two required steps check out:

1. **Retained-phase lower invariant.** `x >= -L - 1 - 1/r_min`. For `M = 0`, `floor(yA) = 0` gives `y < 1/A <= 1/r_min`. For `M > 0`, `M >= yA - 1`, `AH >= 1`, and `-H + delta/2 = -1/r_s`, giving the same lower bound after subtracting the consumed input chunk.
2. **Largest phase/read bound.** The invariant gives `y <= C + 1/r_min`; `M <= y r_max`; and every reciprocal phase step is at most `1/r_min`. Therefore the largest processed phase is bounded by

   ```text
   C - L - 1 + (C + 1/r_min) (r_max/r_min - 1).
   ```

   Cubic interpolation can address one integer tap beyond `floor(phase)`. Pinned Rubato allocates `P + 2L` samples when constructed with private maximum `P`; choosing

   ```text
   P = C + ceil((C + 1/r_min) (r_max/r_min - 1))
   ```

   covers both the mono direct and stereo combined-sinc paths. The combined path's explicit `L + 1` spill tap is visible at `asynchro_sinc.rs:392-403`; the direct cubic path uses four nearest phases at `:425-435`.

The implementation rounds the minimum down and every upper intermediate up with `next_down`/`next_up` before the final `ceil`. This also turns an exactly integral theoretical excess into a strictly larger representable value before `ceil`, preserving the strict Rubato index inequality. Across the supported rate pairs at the 10% limit, the computed extra private frames are:

| Public `C` | Private guard range across all supported pairs |
|---:|---:|
| 1 | 1–2 |
| 240 | 54–55 |
| 480 | 107–108 |
| 960 | 214–215 |

The magnitude and rate dependence are consistent with the recurrence; this is not the former guessed `+1` guard.

### Public transaction and output bounds

- The backend is constructed with private `P` and immediately resized back to public `C` (`adaptive.rs:213-224`). `requirements()` publishes the configured `C`, and the alternating matrix asserts both `input_frames_next == C` and `input_frames_max == C` after each history block.
- `normal_output_workspace_frames` at `adaptive.rs:226-230` reproduces pinned Rubato's fixed-input `calculate_max_output_size` expression exactly (`asynchro.rs:410-420`), but evaluates it at public `C`, not private `P`.
- Finish capacity is `(zero_pump_blocks + 1) * normal_output_workspace_frames` with checked integer arithmetic. The `ceil((L/2)/C) + 1` zero-pump bound advances more than half the sinc support even at the minimum ratio and leaves one full transaction for phase. One-frame chunks exercise the maximum pump-count shape for every supported rate pair and both correction extremes.

No arithmetic or workspace-bound C/H residual was found.

## Behavioral and realtime contract audit

- **Strong collected accounting:** `adaptive_finish.rs:408` accumulates prior raw streaming output `S`, terminal raw generated output `G`, leading trim `L`, and trailing trim `T`, and verifies the collected useful range is exactly `S + G - L - T` across 0/1/2/7/31 prior alternating phases. The report also satisfies `G = output_frames + T` and returns no pending suffix.
- **Smoothing:** the final transaction is resized before `advance_smoothed_ratio`, so its one-pole duration is `valid_input_frames / input_rate` (`adaptive.rs:357-378`, `:484-492`). The zero-pump loop at `:415-449` does not advance smoothing or call the ratio setter. The focused test at `adaptive_finish.rs:368` independently computes the expected update and final ratio.
- **Sticky terminal fault/reset:** a nonfinite backend result sets `Faulted`; subsequent finish and live calls return `EndOfStream`. `reset` clears backend history, target/smoothed corrections, lifecycle, and restores the nominal ratio (`adaptive.rs:558-570`). The focused regression at `adaptive_finish.rs:529` passed.
- **Gain/isolation and boundaries:** the stereo DC/channel-isolation test (`adaptive_finish.rs:479`) passed, as did first/last impulse survival at every supported pair.
- **No allocation/caller stability:** the dedicated allocator test passed in debug and release with zero counted alloc/realloc/alloc-zeroed operations across an opposite live/finish reversal. Caller pointers and capacities remained unchanged.

## Dependency license and safety scope

The allocator package is `publish = false`, version `0.0.0`, and declares `MPL-2.0`, matching workspace policy. Cargo-deny's licenses, advisories, and sources families all pass. `cargo tree --edges normal` excludes it; it appears only under `relay-resample` dev edges.

Cargo nevertheless treats the nested path package as a workspace member. It intentionally does not inherit the workspace `unsafe_code = "forbid"` lint because implementing `GlobalAlloc` requires unsafe code. Its unsafe scope is narrow: one `unsafe impl GlobalAlloc`, with `alloc`, `alloc_zeroed`, `realloc`, and `dealloc` forwarding the exact allocator arguments to `System`; only the first three increment an `AtomicUsize`. No production crate links it through normal dependencies. Strict Clippy passes for both `relay-resample` targets and the allocator package.

The allocator test binary contains only this one test, reducing interference from parallel sibling tests. The counter can still produce a conservative false positive if an unrelated runtime thread allocates during the measured interval; that would fail rather than conceal a converter allocation. This is a test-oracle limitation, not a safety defect found in the forwarding implementation.

## Validation commands

| Command | Result |
|---|---|
| Exact C1 debug test with `--exact --nocapture` | **PASS**, 1/1 |
| Exact C1 release test with `--exact --nocapture` | **PASS**, 1/1 |
| `cargo test -p relay-resample --all-targets --all-features --locked` | **PASS**, allocator 1, adaptive finish 11, contract 15 |
| `cargo test --release -p relay-resample --all-targets --all-features --locked` | **PASS**, same test set |
| `cargo clippy -p relay-resample --all-targets --all-features --locked -- -D warnings` | **PASS** |
| `cargo clippy -p relay-resample-test-allocator --all-targets --all-features --locked -- -D warnings` | **PASS** |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | **FAIL outside scope**: two pre-existing `relay-audio/tests/virtual_hours.rs` warnings |
| `cargo deny --locked check licenses advisories sources bans` | **FAIL in scope**: new allocator path edge is an implicit wildcard; other three families pass |

## Limitations

- Runtime execution was on this x86_64 environment. The mathematical buffer bound is architecture-independent, but the stereo combined-sinc regression was not rerun on CI's Windows/macOS or an aarch64 NEON machine.
- The committed all-rate reversal matrix covers the supported production durations 5/10/20 ms and a separate one-frame pump test, not every integer `C` between them.
- Workspace-wide strict Clippy is not green because of two known, unrelated `relay-audio` test warnings. Package-scoped strict Clippy for every reviewed target is green.
- Because the workspace is largely untracked, provenance and diff completeness cannot be established from Git.

## Disposition

**BLOCKED.** The former critical Rubato panic and its C/H correctness residuals are cleared: exact debug/release reproduction, expanded stereo/all-rate alternating histories, the pinned recurrence proof, output/accounting contracts, smoothing, sticky reset, gain/isolation, and zero-allocation checks all pass. However, the fix introduces a **High** dependency-policy regression: the allocator dev edge is an implicit wildcard and fails the mandatory cargo-deny CI gate. Correct that manifest edge and rerun the locked gate before changing this disposition to PASS.
