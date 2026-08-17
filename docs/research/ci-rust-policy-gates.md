# Rust CI policy gates — research and implementation evidence

**Date:** 2026-08-16  
**Status:** Implemented and locally exercised; hosted execution remains **unproven**

## Scope

This task changes only `.github/workflows/ci-rust.yml` and this evidence record. It adds the two Rust policy gates now required by the checked-in audio and license work:

1. an exact `cargo-deny 0.20.2` Linux install and one blocking combined check of `licenses advisories sources bans`; and
2. one locked, all-feature, all-target, workspace-wide nextest run using Cargo's release profile.

The existing Ubuntu/macOS/Windows debug test matrix, strict Clippy command, and every libopus acquisition, checksum, environment, and smoke-check line remain intact. `deny.toml`, manifests, lockfiles, dependencies, Rust source, and audio source are explicit non-goals.

## Pending / hosted status

- [x] Inspect the Rust workflow, `deny.toml`, `rust-toolchain.toml`, relevant plans, and prior audio/license evidence.
- [x] Select exact tool/action pins and a non-tripled release-test matrix location.
- [x] Parse/lint the workflow and exercise the exact cargo-deny policy command locally.
- [x] Exercise equivalent locked debug/release workspace commands locally.
- [ ] **UNPROVEN HOSTED:** GitHub must still run the pinned action and the Linux policy/release gates, and re-prove all three libopus runner paths.
- [ ] **CURRENT TREE BLOCKER OUTSIDE SCOPE:** the strict formatting and Clippy commands currently expose issues in `crates/relay-audio/tests/virtual_hours.rs`; this CI-only task did not change Rust/audio code to mask them.

## Primary sources (four maximum)

1. [cargo-deny 0.20.2 release](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.20.2) — immutable requested tool release and platform archives.
2. [cargo-deny checks documentation](https://embarkstudios.github.io/cargo-deny/checks/index.html) — defines the `licenses`, `advisories`, `sources`, and `bans` families and states that `check` applies configured dependency policy.
3. [`taiki-e/install-action` at pinned commit `288e7469…`](https://github.com/taiki-e/install-action/tree/288e746965032cfcc232e09af2daf5f23c14d780) — release 2.86.1 documents exact `tool@version`, `fallback: none`, and default SHA-256 verification; its pinned cargo-deny manifest records the 0.20.2 Linux archive digest.
4. [cargo-nextest running-tests documentation](https://nexte.st/docs/running/) — documents `--release` as building optimized release-mode artifacts and `--locked` as requiring the lockfile to remain current.

## Findings and potential corrections

### P1 — Dependency policy existed locally but had no hosted gate

`deny.toml` already denies wildcard dependencies and unknown registries/git sources, checks advisories, and carries the approved license allowlist. Prior project-license evidence passed all four checks with cargo-deny 0.20.2, but `.github/workflows/ci-rust.yml` never installed or ran that tool.

**Potential correction:** install exactly 0.20.2 on Linux and run all four named checks together as a blocking command. Do not copy upstream examples that make advisories non-blocking, and do not edit warnings/allowances merely to make CI green.  
**Applied:** `cargo deny --locked check licenses advisories sources bans` is now an ordinary failing step on only the Linux matrix leg.

### P1 — A release-mode test proof was missing

The existing three-OS nextest matrix compiled tests only in the default test profile. Optimized builds can expose overflow, numeric, FFI, or optimizer-sensitive behavior not represented by debug-only proof.

**Potential correction:** add `--release` to a second locked workspace nextest invocation. Running it on every matrix leg would approximately triple the new release-build cost while duplicating the optimizer/profile proof.  
**Applied:** the release run is placed on `runner.os == 'Linux'`. Linux is already a required matrix leg, provisions and smoke-checks exact libopus 1.6.1 before Cargo, and can therefore execute the complete audio workspace in release mode. Windows and macOS retain their existing debug all-target/all-feature tests for platform-specific proof. This adds one release compilation, not three.

### P2 — A mutable installer reference would undermine the new exact tool pin

An exact `cargo-deny@0.20.2` input alone would still delegate installation behavior to a moving action tag.

**Potential correction:** pin the new installer use to an immutable commit and retain checksum verification with fallback disabled.  
**Applied:** the new step uses `taiki-e/install-action@288e746965032cfcc232e09af2daf5f23c14d780` (release 2.86.1), `tool: cargo-deny@0.20.2`, `checksum: true`, and `fallback: none`. The pinned action manifest records Linux archive SHA-256 `9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f`. Existing action references were not broadened into this focused change.

### P2 — Existing native and lint gates must not be weakened

The workflow's OS-specific libopus setup is evidence-backed and sensitive to linker/loader paths. Strict Clippy is repository policy.

**Potential correction:** add the gates without refactoring native setup, collapsing the three-OS debug matrix, or relaxing `-D warnings`.  
**Applied:** all libopus install/smoke steps are textually preserved, and the Clippy command remains exactly `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`.

## Decisions applied

- Keep one existing `rust` matrix job rather than introduce a separate checkout/toolchain/native-setup job.
- Install and execute cargo-deny only where it is used: Linux.
- Name all four dependency-policy families explicitly in one command and keep advisories blocking.
- Pass `--locked` to both the cargo-deny graph resolution and release nextest invocation.
- Put the additional release-mode run only on Linux; keep the existing debug nextest run on all three operating systems.
- Pin the new installer action by full commit SHA, the tool by exact version, and its downloaded archive through the action's pinned SHA-256 manifest with fallback disabled.
- Make no changes to `deny.toml`, toolchain policy, dependencies, Rust/audio code, or libopus provisioning.

## Exact validation

Run from the repository root on the available Linux host.

### Workflow syntax and semantics — PASS

```text
$ python -c 'import yaml; yaml.safe_load(open(".github/workflows/ci-rust.yml"))'
exit 0

$ go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/ci-rust.yml
exit 0; no diagnostics
```

A static assertion also confirmed the matrix still names exactly `ubuntu-latest`, `windows-latest`, and `macos-latest`; the libopus steps precede Cargo; and strict Clippy is unchanged.

### Exact cargo-deny installation artifact and policy command — PASS

```text
$ sha256sum /tmp/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz
9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f  /tmp/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz

$ /tmp/cargo-deny-0.20.2/.../cargo-deny --version
cargo-deny 0.20.2

$ PATH=/tmp/cargo-deny-0.20.2/...:$PATH cargo deny --locked check licenses advisories sources bans
advisories ok, bans ok, licenses ok, sources ok
exit 0
```

The run emitted the existing non-failing `license-not-encountered` warnings for BSD-2-Clause, BSD-3-Clause, and ISC. No deny policy was changed or suppressed.

### Equivalent locked workspace commands

```text
$ rustc --version
rustc 1.92.0 (ded5c06cf 2025-12-08)

$ pkg-config --modversion opus
1.6.1

$ cargo check --locked --workspace --all-targets --all-features
exit 0

$ cargo nextest run --locked --profile ci --workspace --all-targets --all-features
exit 0; 165 passed, 1 skipped

$ cargo nextest run --locked --release --profile ci --workspace --all-targets --all-features
exit 0; release profile optimized; 165 passed, 2 skipped
```

The available local nextest was 0.9.140; CI retains its pre-existing exact 0.9.143 pin. The command surface is equivalent, but the hosted 0.9.143 execution is not claimed by this local result.

### Preserved strict gates against the current concurrent tree — FAIL, not weakened

```text
$ cargo fmt --all -- --check
exit 1; formatting diff in crates/relay-audio/tests/virtual_hours.rs

$ cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
exit 101; dead-code fields device_rate_hz, packet_ms, and drift_ppm in the same test file
```

These failures are valuable evidence that the original strict gates remain effective. The failing Rust/audio test file is outside this task's ownership, so it was not formatted, annotated, or otherwise edited here. The added cargo-deny and release nextest gates themselves pass locally.

## Hosted limitations

GitHub Actions was not executed here. The following remain explicitly **UNPROVEN** until a hosted run succeeds:

- resolution and execution of the SHA-pinned install action on the current Ubuntu image;
- action-side checksum verification/path publication for cargo-deny 0.20.2;
- the exact CI-pinned nextest 0.9.143 release run;
- Ubuntu/macOS/Windows libopus provisioning and loader behavior on current runner images;
- the complete workflow after the out-of-scope current-tree format/Clippy failures are resolved by their owner.
