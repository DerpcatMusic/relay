# Testkit seed — Research and implementation evidence

**Date:** 2026-08-16  
**Task owner:** Prime Agent  
**Status:** Complete

## Scope

Create only the dependency-free Phase-0 `relay-testkit` seed: a manually advanced fake monotonic clock, a deterministic interleaved audio source generator, and an interleaved audio sink collector. Network simulation, transport behavior, async runtimes, codecs, production-domain coupling, and root-workspace integration are explicit non-goals.

## Acceptance criteria

- [x] `FakeClock` has explicit time, deterministic manual advancement, checked overflow, and no wall-clock dependency.
- [x] `AudioSource` generates samples in deterministic frame-major/channel-minor order and reads only complete frames.
- [x] `AudioSink` collects complete interleaved frames in write order and supports checked sample lookup.
- [x] Audio invariants are documented and enforced at every public construction/read/write boundary.
- [x] Empty audio is valid, zero channels and partial frames are rejected, and invalid operations do not mutate state.
- [x] Arbitrary `f32` bit patterns are preserved so downstream numeric-error tests remain possible.
- [x] The crate has no dependencies, including no production dependency on `relay-domain`.
- [x] Unsafe Rust is forbidden and unit tests, formatting, Clippy, and documentation checks pass.
- [x] Network simulation is not present.

## Sources consulted

Research stopped after these three official Rust/Cargo primary sources.

| Source | Why it is authoritative | Accessed |
|---|---|---|
| https://doc.rust-lang.org/std/time/struct.Duration.html#method.checked_add | Official standard-library contract for checked monotonic-duration addition and overflow reporting | 2026-08-16 |
| https://doc.rust-lang.org/std/primitive.usize.html#method.checked_mul | Official standard-library contract for checking frame-count × channel-count arithmetic | 2026-08-16 |
| https://doc.rust-lang.org/cargo/reference/workspaces.html | Official Cargo rules for workspace membership, explicit workspace roots, and packages under a workspace directory | 2026-08-16 |

## Findings

- `Duration::checked_add` reports unrepresentable results with `None`, so `FakeClock::advance` can reject overflow without changing its current time.
- `usize::checked_mul` reports dimension overflow without wrapping, so source generation validates `frames * channels` before allocation or invoking the generator.
- Cargo determines workspace membership from manifest workspace declarations and directory ancestry; a package may belong to only one workspace. Because the repository root does not yet list `relay-testkit` and this task forbids editing the root manifest, a local empty `[workspace]` table makes the seed independently validatable by manifest path.
- The Phase-0 plan requires only deterministic clock and audio source/sink vocabulary and explicitly leaves network simulation to later audio/transport work.
- The useful audio boundary is a complete frame, not an individual sample. The crate therefore keeps channels fixed and nonzero, accepts only sample lengths divisible by channels, counts positions in frames, and never returns or appends partial frames.
- Samples remain opaque `f32` values. The testkit preserves NaN, infinities, and signed zero rather than silently imposing a production numeric policy; downstream components can use these fixtures to test their own validation.

## Potential corrections to the master plan

1. **Make workspace-integration timing explicit for F0.9.** A crate located beneath the repository workspace but omitted from `workspace.members` cannot be validated normally unless it is excluded by the root or declares its own workspace. **Impact:** this focused task needs a temporary local `[workspace]` marker because the root manifest is out of scope. **Disposition:** applied the local marker now; when an integration owner adds `crates/relay-testkit` to root `workspace.members`, remove the crate-local `[workspace]` table so there is one workspace root and shared workspace settings can be inherited.
2. **No testkit vocabulary correction required.** The plan's intentionally small source/sink/clock scope is sufficient for Phase 0. Exact traits shared with production code should wait until a real consumer demonstrates the seam. **Disposition:** implemented concrete dependency-free fixtures without coupling them to `relay-domain`.

## Decisions applied

- `FakeClock` stores `std::time::Duration`, starts at zero by default, optionally starts at a selected logical time, and advances only on explicit calls.
- `AudioSource::generate(channels, frames, sample_at)` calls the generator exactly once per sample in ascending frame then channel order. It also supports validated construction from an existing interleaved buffer.
- Source reads accept only frame-aligned output slices. At end-of-source, only the returned complete-frame prefix is written and the unused output suffix is unchanged; `reset` deterministically rewinds the frame cursor.
- `AudioSink` fixes its channel count at construction, validates each write before reserving/appending, exposes collected samples, provides non-panicking `(frame, channel)` lookup, and can clear samples without changing format.
- Dimension arithmetic and clock arithmetic are checked. Fallible vector reservation turns impossible capacity requests into an `AudioError` instead of a capacity panic.
- The crate uses edition 2024 and Rust 1.92 directly because it is temporarily its own workspace. `#![forbid(unsafe_code)]` and manifest lint policy make the unsafe prohibition visible both in source and build configuration.

## Validation evidence

The first formatting check identified only rustfmt layout differences; `cargo fmt` corrected them. The following manifest-scoped validation then passed on the repository toolchain:

```text
$ rustc --version
rustc 1.92.0 (ded5c06cf 2025-12-08)

$ cargo --version
cargo 1.92.0 (344c4567c 2025-10-21)

$ cargo fmt --manifest-path crates/relay-testkit/Cargo.toml -- --check
passed (no output)

$ cargo check --manifest-path crates/relay-testkit/Cargo.toml --all-targets
Finished `dev` profile; relay-testkit checked successfully

$ cargo test --manifest-path crates/relay-testkit/Cargo.toml
11 passed; 0 failed; unit and doc-test targets passed

$ cargo clippy --manifest-path crates/relay-testkit/Cargo.toml --all-targets -- -D warnings
Finished successfully with no warnings

$ cargo doc --manifest-path crates/relay-testkit/Cargo.toml --no-deps
Finished successfully; relay-testkit documentation generated

$ cargo metadata --manifest-path crates/relay-testkit/Cargo.toml --no-deps --format-version 1
package relay-testkit; edition 2024; rust-version 1.92; dependencies []
```

The crate-local `[workspace]` table is the manifest-only workaround used for these commands. The root `Cargo.toml` was not edited.

## Deferred follow-ups

- Add network loss, delay, jitter, reorder, duplication, or transport simulation only in the later audio/transport scope.
- Introduce a shared production trait only when a Phase-1 consumer proves the required interface; keep this seed independent meanwhile.
- Have the workspace integration owner add `relay-testkit` to root membership and remove the temporary crate-local `[workspace]` marker in the same change.
- Run repository-level and three-OS workspace gates after integration; this focused validation proves only the manifest-scoped crate on the current Linux environment.
