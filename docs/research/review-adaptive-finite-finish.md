# Adversarial review: `AdaptiveClockConverter` finite finish

**Disposition: BLOCKED**  
**Reviewed:** `crates/relay-resample/src/adaptive.rs`, `src/lib.rs`, `tests/adaptive_finish.rs` against `docs/design/audio-finite-drain-option-a.md`, `docs/design/audio-finite-drain-synthesis.md`, and pinned `rubato = 4.0.0`.  
**Method:** read-only source review plus debug/release/package-Clippy and removed throwaway integration tests. No production or committed test source was edited.

## Findings

### Critical — the one-frame private Rubato guard does not cover admitted ratio/phase histories; safe finish input panics

**Location:** `crates/relay-resample/src/adaptive.rs:171-189`, exercised by `finish_validated` at `:319-350`.

Construction adds exactly one input frame (`chunk_frames + 1`) to Rubato's private maximum and claims this covers an extreme ratio/phase interpolation. It does not. A fully valid public sequence—one ordinary block at the admitted positive correction extreme, then a full valid finish after switching the target to the admitted negative extreme—panics inside pinned Rubato rather than returning a `ResampleError`:

```rust
#[test]
fn finish_panics_for_valid_admitted_ratio_history() {
    let n = 240;
    let mut r = AdaptiveClockConverter::new(
        48_000,
        48_000,
        2,
        n,
        AdaptiveClockConfig {
            max_correction_ppm: 100_000.0,
            smoothing_time_seconds: 0.000_001,
        },
    ).unwrap();

    let q = r.requirements();
    let input = vec![0.0; q.input_frames_next * 2];
    let mut live_output = vec![0.0; q.output_frames_max * 2];
    r.set_output_input_correction(OutputInputRatioCorrectionPpm::new(100_000.0).unwrap());
    r.process_interleaved(&input, &mut live_output).unwrap();

    r.set_output_input_correction(OutputInputRatioCorrectionPpm::new(-100_000.0).unwrap());
    let fq = r.finish_requirements().unwrap();
    let final_input = vec![0.0; n * 2];
    let mut finish_output = vec![0.0; fq.output_workspace_frames * 2];
    r.finish_interleaved(&final_input, n, &mut finish_output).unwrap();
}
```

Both debug and release fail with:

```text
rubato-4.0.0/src/asynchro_sinc.rs:403:58:
index out of bounds: the len is 753 but the index is 753
```

This is not confined to an adversarial nonstandard chunk. A catch-and-continue matrix found the same finish panic at 240, 480, and 960 frames and across multiple supported rate pairs, directions of ratio reversal, and valid-prefix lengths. A separate legal streaming sequence at `44_100 -> 48_000`, stereo, 480 frames, alternating `-100_000/+100_000 ppm` panics on the second ordinary `process_interleaved` call with `len 993, index 993`; thus the construction change also fails to preserve panic-free live behavior.

Direct pinned-Rubato probing of the minimal 480-frame reversal showed guards 0, 1, and 2 all panic and guard 3 happens to pass that one case. That is evidence against the current `+1`, **not** proof that `+3` is a global bound. The private allocation must be derived and tested over the full admitted ratio ramp, fractional phase, channel path, chunk-size, and valid-prefix state space, or the backend/configuration must prohibit the unsafe trajectory before processing. Safe caller input must never reach a backend indexing panic.

**Disposition:** release blocker. Caller output-workspace sufficiency and terminal accounting cannot be accepted while the backend's private workspace can panic before producing a report.

### Medium — the committed oracle omits required histories and therefore misses the blocker

**Location:** `crates/relay-resample/tests/adaptive_finish.rs:30-261`; design test plan `audio-finite-drain-option-a.md:423-432`.

The main extrema test uses mono only and holds one correction target for three prefix blocks and finish. It never reverses/alternates the correction before the terminal transaction. The remaining tests likewise do not cover the required combination of:

- alternating correction/ramp history and opposite final target;
- stereo finite finish/channel isolation;
- direct proof that the final valid transaction advances smoothing by its actual valid duration and that zero pumps advance it no further;
- DC/passband gain after adaptive trim;
- a heap-allocation counter (pointer/capacity identity only proves caller vectors did not move);
- sticky finish fault and reset after a backend/nonfinite-output failure;
- strong `S + G - L - T` collected-stream accounting across varied prior streaming phase.

Several assertions are internal identities rather than independent oracles: `generated == output + trailing`, generated within the reported workspace, and a very low `peak > 0.005`. They cannot expose the admitted phase/ramp crash above or establish bounded boundary gain/area.

**Disposition:** add the exact reproducer first, then a catch-free matrix covering all 48 kHz playback pairs, 5/10/20 ms chunks, stereo, both ratio-reversal directions, partial boundaries, and alternating histories. Keep tests against independent expected state/accounting, not only report self-consistency.

### Low — public module/trait documentation still says adaptive finite media has no finish path

**Location:** `crates/relay-resample/src/lib.rs:4-8` and `:56-60`.

The docs direct every finite source to `FiniteFixedRatioConverter` and describe the worker trait as having no usable finite path, while the crate now publicly exports `AdaptiveClockConverter::finish_interleaved`. The trait should remain finish-free, but the guidance needs to distinguish fixed finite conversion from the concrete adaptive terminal operation.

**Disposition:** correct the API documentation before publishing the surface.

## Cleared observations (subject to the critical fix)

- External terminal rejections are prevalidated before state mutation: lifecycle, exact full input length, `1..=chunk`, finite valid prefix only, checked sample counts, and required output capacity. The inspected rejection sequence left ratio, smoothing, lifecycle usability, and caller output unchanged.
- The valid suffix is not inspected or treated as media. Removed adversarial stereo tests also observed that Rubato writes only the reported generated prefix and leaves an extra caller suffix untouched in nonpanicking cases.
- Arithmetic used to publish requirements is checked. Given the backend report/capacity invariants, the later output slice and trim subtractions have no independent reachable underflow/overflow path; the backend panic is the actual unsafe boundary found.
- Finish advances the smoother once after resizing to the valid prefix. Removed tests matched the one-pole update using `valid_input_frames / input_rate`, and observed no further smoother change during zero pumping.
- Backend/nonfinite-output finish failure is sticky: a removed `f32::MAX` test produced `NonFiniteOutput`, a retry returned `EndOfStream`, and `reset` restored successful use.
- Pinned Rubato source audit: `Resizable::set_chunk_size` (`rubato-4.0.0/src/asynchro.rs:666`) only updates scalar sizes; `process_into_buffer` (`:470`) uses retained buffers; neither processing path allocates. A removed global-allocation-counter test (compiled with lint caps solely because the workspace forbids unsafe test allocators) counted zero allocations around one ordinary call and one successful finish.
- The final ratio is frozen through zero pumping: no smoothing/ratio setter is called in the pump loop. Output finiteness is checked over every reported generated sample.

## Validation log

| Command/test | Result |
|---|---|
| `cargo test -p relay-resample` | PASS — 20 integration tests (5 adaptive-finish + 15 contract), plus unit/doc targets |
| `cargo test --release -p relay-resample` | PASS — same committed suite |
| `cargo clippy -p relay-resample --all-targets --all-features -- -D warnings` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | FAIL outside reviewed crate: two `relay-audio/tests/virtual_hours.rs` warnings (`field_reassign_with_default`, `manual_is_multiple_of`) |
| Removed minimal finish ratio-reversal test, debug | **FAIL/PANIC**, Rubato `len 753, index 753` |
| Removed minimal finish ratio-reversal test, release | **FAIL/PANIC**, same |
| Removed standard-chunk/rate/phase catch matrix | **FAIL**, many 240/480/960 finish panics across supported pairs |
| Removed alternating live 44.1→48 kHz stereo test | **FAIL/PANIC** on second valid live call, Rubato `len 993, index 993` |
| Removed no-allocation counter, release | PASS, zero allocations for ordinary processing and a non-reversal finish |
| Removed nonfinite-output sticky-fault/reset test | PASS |

All throwaway test/example files were deleted after execution.
