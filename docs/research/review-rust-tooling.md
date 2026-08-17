# Phase-0 Rust Workspace and Tooling Review

**Review date:** 2026-08-16  
**Scope:** Independent, read-only review of the Phase-0 Rust workspace, `relay-domain`, and repository-owned Rust tooling against the master plan and official Cargo/Rust semantics.  
**Implementation changes:** None. This evidence file is the only file written by this review.

## Scope and criteria

- Workspace/package structure and Cargo configuration match the Phase-0 plan.
- Feature, resolver, lint, toolchain, lockfile, and command settings interact as intended under official Cargo/Rust behavior.
- `relay-domain` remains dependency-free, synchronous, platform-neutral, and consistent with documented V1 invariants.
- Formatting, checking, ordinary tests, nextest, and Clippy are run where available; missing tools and alternate-toolchain results are distinguished.
- Findings are supported by repository evidence, command output, and primary upstream sources.

## Sources

| Source | Relevance | Accessed |
|---|---|---:|
| [Cargo workspaces reference](https://doc.rust-lang.org/cargo/reference/workspaces.html) | Virtual-workspace resolver selection, shared `Cargo.lock`, package/lint inheritance, workspace command behavior | 2026-08-16 |
| [Cargo manifest reference: lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section) | Rust and Clippy lint namespaces and package-local application | 2026-08-16 |
| [Cargo configuration reference: aliases](https://doc.rust-lang.org/cargo/reference/config.html#alias) | `.cargo/config.toml` discovery and alias syntax | 2026-08-16 |
| [`cargo check` manifest options](https://doc.rust-lang.org/cargo/commands/cargo-check.html#manifest-options) | Official `--locked` semantics: fail if the lockfile is missing or resolution would change it | 2026-08-16 |
| [rustup profiles](https://rust-lang.github.io/rustup/concepts/profiles.html) | `minimal` contains `rustc`, `rust-std`, and Cargo; `clippy` is part of `default`, not `minimal` | 2026-08-16 |
| [rustup overrides/toolchain file](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file) | Exact channel, profile, components, and directory override behavior | 2026-08-16 |
| [cargo-nextest configuration](https://nexte.st/docs/configuration/) | Automatic repository config location is `.config/nextest.toml` at the Cargo workspace root; `--config-file` overrides it | 2026-08-16 |
| [`docs/plans/2026-08-15-relay-master-plan.md`](../plans/2026-08-15-relay-master-plan.md) | Domain shape, canonical V1 media (`48,000 Hz`, stereo, Opus, DTX off), and expected CI commands | local specification |
| [`docs/plans/2026-08-15-relay-phase-0-foundation-plan.md`](../plans/2026-08-15-relay-phase-0-foundation-plan.md) | Phase-0 acceptance and reproducibility requirements | local specification |

All upstream pages returned HTTP 200 during this review. The prior task records in `docs/research/rust-domain-foundation.md` and `docs/research/repository-tooling-foundation.md` were treated as implementation history, not substitutes for direct inspection.

## Findings

### What is coherent

- The root is a valid virtual workspace with one member, explicit resolver 3, edition 2024, and Rust 1.92 inheritance (`Cargo.toml:1-13`). Official Cargo guidance requires a virtual workspace to select its resolver explicitly because it has no root package edition from which to infer one.
- `relay-domain` inherits the workspace edition, MSRV, and lints (`crates/relay-domain/Cargo.toml:1-8`). `cargo metadata --locked --no-deps` reports one workspace package, edition 2024, `rust_version: 1.92`, and no dependencies.
- Unsafe Rust is forbidden both through inherited workspace lint configuration and the crate attribute (`Cargo.toml:8-13`, `crates/relay-domain/src/lib.rs:1`).
- The session, route, fallback, connection, frame-duration, FEC, audio-profile, error, and quality vocabulary matches the Phase-0/master-plan names. No async, platform, codec, transport, or third-party dependency has entered the crate.
- `FrameDuration::microseconds` is total over its three variants and is a `const fn` (`crates/relay-domain/src/lib.rs:41-58`). Error values preserve the rejected sample rate/channel count and implement `Display` and `Error` without dependencies.
- Formatting, metadata, check, ordinary tests, and root-invoked nextest all pass on the pinned toolchain. A supplemental Clippy run passes on installed Rust 1.97.1, but that does not repair the pinned-toolchain failure described below.

## Severity-ranked issues

No critical or high-severity issue was found in the limited Phase-0 scope.

### Medium — V1 validation does not enforce the specified DTX-off constraint

**Evidence:** The master plan's canonical V1 media list explicitly says `DTX off` (`master-plan.md:800-807`). `AudioProfile::new` documents that it checks “the V1 media constraints,” but `validate` checks only 48 kHz, two channels, and nonzero bitrate (`crates/relay-domain/src/lib.rs:81-116`). The supplied `dtx` value is accepted unchanged, so `AudioProfile::new(..., true)` succeeds. Existing tests only use/assert `false` and contain no rejection test (`crates/relay-domain/tests/domain.rs:6-16, 27-37`).

**Impact:** A profile represented as validated for V1 can contradict a locked master-plan media invariant. Later codec/protocol code may assume DTX is off while receiving `true` from the domain API.

**Potential correction:** Either reject `dtx == true` with a specific error in V1 validation and test it, or correct the master plan/API documentation to distinguish a canonical default from an allowed negotiated value. Do not leave the contract ambiguous.

### Medium — `AudioProfile` validation is advisory rather than an invariant

**Evidence:** Every field is public (`crates/relay-domain/src/lib.rs:68-77`). Callers can construct an arbitrary literal without `new`, and can mutate a successfully validated profile into unsupported sample rates, channel counts, or zero bitrate. The test itself demonstrates post-construction invalidation by mutating `channels` and `bitrate_bps` before manually calling `validate` (`crates/relay-domain/tests/domain.rs:54-65`). `QualityProfile::Custom` accepts any `AudioProfile`, not only one that has just passed validation (`lib.rs:143-150`).

**Impact:** If downstream code treats `AudioProfile`/`QualityProfile::Custom` as valid domain values, invalid states remain representable and validation can be forgotten at any boundary. The current API is safe only under a convention that every consumer revalidates.

**Potential correction:** The master plan should decide explicitly between (a) a validated value object with private fields/getters and fallible construction/mutation, and (b) a wire-like open data carrier that must be validated at each consumer. The current naming and tests imply (a), while the public shape implements (b).

### Medium — the pinned, repository-declared toolchain cannot run the required Clippy command

**Evidence:** `rust-toolchain.toml:1-4` chooses profile `minimal` and only adds `rustfmt`. Official rustup semantics say `minimal` excludes Clippy. `rustup component list --toolchain 1.92.0 --installed` confirms only Cargo, rustc, rust-std, and rustfmt. Both the master plan (`master-plan.md:3217-3227`) and repository entry points (`.cargo/config.toml:4`, `justfile:19-20`) prescribe Clippy. On the pinned toolchain, both direct Clippy and `cargo relay-lint` fail with: `cargo-clippy is not installed for the toolchain '1.92.0...'`.

**Impact:** A fresh rustup setup honoring the checked-in toolchain file cannot execute the repository's declared lint gate without an undocumented manual component install.

**Potential correction:** Add `clippy` to the toolchain file's explicit components (retaining `minimal`) and re-run the pinned command. The successful supplemental Rust 1.97.1 Clippy run only shows the current source is clean under that newer installed component.

### Medium — root `nextest.toml` is not automatically discovered and its explicit path is invocation-directory-sensitive

**Evidence:** Official nextest discovery looks for `.config/nextest.toml` at the Cargo workspace root. The repository instead stores `nextest.toml` at root. Consequently, the master-plan command `cargo nextest run` does not load this file. Direct proof: `cargo nextest run --profile ci ...` from the workspace root fails with `profile 'ci' not found`; the same command with `--config-file nextest.toml` passes. Repository aliases compensate with a relative override (`.cargo/config.toml:3`), but Cargo discovers the alias when invoked in a member directory while nextest resolves the relative filename from that invocation directory: from `crates/relay-domain`, `cargo relay-test` fails with `failed to parse nextest config at nextest.toml: No such file or directory`.

**Impact:** The documented standard CI command silently uses nextest defaults, and a repository-provided alias fails from a normal workspace member directory. Retry/fail-fast/status/JUnit policy is therefore not uniformly applied.

**Potential correction:** Prefer the upstream discovery location `.config/nextest.toml`, remove path-sensitive overrides, and update the master plan's tree. If the root location is retained, every entry point must first anchor execution at the workspace root and the master-plan CI command must include the override.

### Low — reproducible Cargo entry points do not enforce the checked-in lockfile

**Evidence:** `Cargo.lock` is committed, and the Phase-0 objective is reproducibility, but `.cargo/config.toml` and `justfile` omit `--locked` from check/test/lint commands. Official Cargo semantics state that `--locked` fails rather than creating or changing lockfile resolution. The review's equivalent `--locked` metadata/check/test commands passed.

**Impact:** Once dependencies are introduced, routine or CI commands may update `Cargo.lock` rather than detect drift. There is no current dependency-resolution failure because `relay-domain` has no dependencies.

**Potential correction:** Add `--locked` to reproducibility/CI-oriented Cargo and nextest entry points, while retaining an intentionally unlocked dependency-update workflow separately.

## Validation evidence

Commands were run from the repository root unless a different directory is stated.

| Command | Result |
|---|---|
| `rustc --version --verbose` | exit 0; `rustc 1.92.0 (ded5c06cf 2025-12-08)` |
| `cargo --version --verbose` | exit 0; `cargo 1.92.0 (344c4567c 2025-10-21)` |
| `rustup show active-toolchain` | exit 0; `1.92.0-x86_64-unknown-linux-gnu`, overridden by repository `rust-toolchain.toml` |
| `rustup component list --toolchain 1.92.0 --installed` | exit 0; Cargo, rustc, rust-std, rustfmt; **no Clippy** |
| `cargo metadata --locked --no-deps --format-version 1` | exit 0; one dependency-free member, edition 2024, Rust 1.92 |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo check --workspace --all-targets --all-features --locked` | exit 0 |
| `cargo test --workspace --all-targets --all-features --locked` | exit 0; 5 passed, 0 failed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | **exit 1**; pinned toolchain lacks `cargo-clippy` |
| `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings` | exit 0; supplemental newer-toolchain evidence |
| `cargo relay-check --locked` | exit 0 |
| `cargo relay-test --locked` | exit 0; nextest 0.9.140, 5 passed |
| `cargo relay-lint --locked` | **exit 1**; pinned toolchain lacks `cargo-clippy` |
| `(cd crates/relay-domain && cargo relay-check)` | exit 0 |
| `(cd crates/relay-domain && cargo relay-test)` | **exit 96**; relative `nextest.toml` not found |
| `cargo nextest run --profile ci -p relay-domain --all-targets --all-features` | **exit 96**; root config not discovered, profile `ci` unknown |
| `cargo nextest run --config-file nextest.toml --profile ci -p relay-domain --all-targets --all-features` | exit 0; 5 passed |
| `cargo deny --version` | unavailable (`cargo deny` not installed) |
| `just --version` | unavailable |
| `typos --version` | unavailable |

The absence of `cargo-deny`, `just`, and `typos` prevented native semantic execution of those tools; their TOML/surface syntax and interactions were inspected only. No missing tool was installed and no implementation file was edited.

## Overall disposition

**Phase-0 Rust foundation: pass with four medium corrections and one low hardening item.** The workspace structure, resolver/lint inheritance, dependency direction, unsafe prohibition, formatting, compilation, and existing tests are sound. Before calling the tooling reproducible and the V1 audio profile invariant-preserving, resolve the DTX contract, decide whether `AudioProfile` is actually a validated value object, include Clippy in the pinned toolchain, and make nextest configuration discovery independent of invocation directory.
