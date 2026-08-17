# Rust domain foundation — Research and implementation evidence

**Date:** 2026-08-16  
**Task owner:** Prime Agent  
**Status:** Complete

## Scope

Create the Phase-0 virtual Rust workspace and dependency-free `relay-domain` crate. The crate owns only synchronous, platform-neutral value types and their validation. Async runtimes, codecs, transports, generated protocol code, and later product behavior are explicit non-goals.

## Acceptance criteria

- [x] Rust 1.92 toolchain and MSRV, edition 2024, and resolver 3 are explicit.
- [x] Workspace lint policy is inherited and unsafe Rust is forbidden.
- [x] `relay-domain` has no dependencies.
- [x] Master-plan session, route, fallback, connection, audio, and quality types exist.
- [x] Audio profile construction and boundary validation are tested.
- [x] Formatting, compilation, and tests pass.

## Sources consulted

Only the two targeted Cargo primary-source checks requested for this task were performed.

| Source | Why it is authoritative | Accessed |
|---|---|---|
| https://doc.rust-lang.org/cargo/reference/workspaces.html | Official Cargo reference for virtual workspaces, resolver selection, inherited package fields, and workspace lint inheritance | 2026-08-16 |
| https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section | Official Cargo manifest reference for lint tables, lint levels, and tool namespaces | 2026-08-16 |

## Findings

- A virtual workspace has no root package edition from which Cargo can infer a resolver, so the resolver must be explicit. The official example pairs `resolver = "3"` with edition 2024.
- `edition` and `rust-version` are supported in `[workspace.package]` and members opt in with `<key>.workspace = true`.
- `[workspace.lints]` defines inheritable lint configuration. A member activates it with `[lints] workspace = true`; Cargo documents workspace lint inheritance as respected since Rust 1.74, below this workspace's 1.92 MSRV.
- `unsafe_code = "forbid"` belongs under the Rust lint table. Cargo maps lint levels to rustc's `forbid`, `deny`, `warn`, and `allow` levels.
- Local master-plan sections 7, 13, and ADR 0004 establish the domain names, initial 5/10/20 ms profiles, and the V1 48 kHz stereo network contract. These were implementation inputs, not additional web research.

## Potential corrections to the master plan

1. **State the `FrameDuration` and `FecPolicy` variants explicitly.** Section 7 names both types but does not define them. Phase 0 uses `Ms5`, `Ms10`, `Ms20` and `Disabled`, `Enabled`, `Adaptive`, matching the plan's initial profile ranges while avoiding later codec behavior. **Disposition:** implemented as a small provisional domain vocabulary; the master plan should clarify before wire mapping.
2. **Specify `AudioProfile` validation rules and mutation expectations.** The plan shows public fields but does not say where invalid external values are rejected. **Disposition:** retained the planned public shape and added both `new` and `validate`; V1 validation requires 48 kHz, stereo, and a nonzero bitrate. A later protocol-boundary task should decide whether fields become private or wire conversion always calls `validate`.
3. **No Cargo correction required.** Resolver 3, edition 2024, inherited MSRV, and inherited lint syntax match current official Cargo documentation.

## Decisions applied

- The root is a virtual workspace containing only `crates/relay-domain`, with resolver 3 and shared edition/MSRV/lints.
- `rust-toolchain.toml` pins `1.92.0` with the minimal profile and rustfmt component.
- The crate repeats `#![forbid(unsafe_code)]` as a visible local guarantee in addition to the inherited workspace lint.
- Domain enum variants follow the master plan exactly where specified: `Connect/Link/Stream`, `Direct/TurnRelay/Sfu`, `Never/Ask/Auto`, all nine connection lifecycle states, and `UltraLowLatency/Balanced/Stable/Custom`.
- `AudioProfile` preserves the planned fields and has no codec or transport dependency. Validation enforces only the local V1 plan invariants; it does not perform codec negotiation.

## Validation evidence

The first `cargo fmt --all -- --check` found one multiline assertion formatting difference. `cargo fmt --all` corrected it, after which the complete requested validation sequence passed.

```text
$ rustc --version
rustc 1.92.0 (ded5c06cf 2025-12-08)

$ cargo --version
cargo 1.92.0 (344c4567c 2025-10-21)

$ cargo fmt --all -- --check
passed (no output)

$ cargo check --workspace --all-targets
Finished `dev` profile; relay-domain checked successfully

$ cargo test --workspace
5 integration tests passed; 0 failed
unit and doc test targets also passed

$ cargo metadata --no-deps --format-version 1
exit 0; one workspace package (`relay-domain`), edition 2024, rust-version 1.92, dependencies []
```

## Deferred follow-ups

- Map domain types to protobuf only when a protocol-boundary task owns that work.
- Revisit profile presets and exact codec bitrate limits after the audio-lab produces evidence.
- Add async/session orchestration, transport selection, and platform code only in their later phases.
