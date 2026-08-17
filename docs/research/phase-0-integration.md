# Phase 0 integration — Research and implementation evidence

**Date:** 2026-08-16  
**Task owner:** Prime Agent integration, informed by independent GPT-5.6 subagent reviews  
**Status:** Complete for this foundation slice; overall Phase 0 remains open

## Scope

Integrate the independently produced Rust/domain, protocol, web, tooling, ADR, and stack-validation foundations. Correct evidence-backed Phase-0 issues without beginning audio DSP, native transport, plugin, control-plane, or billing implementation.

## Acceptance criteria

- [x] Rust formatting, compilation, ordinary tests, Clippy, and nextest pass on the pinned toolchain.
- [x] Frozen web install, typecheck, and static build pass.
- [x] Protobuf formatting, lint, and build pass.
- [x] Generated outputs are ignored and removed from the source tree.
- [x] Independent reviews have source-backed findings and explicit potential corrections.
- [x] High-impact Phase-0 findings are corrected or explicitly deferred.

## Sources consulted

The integration used the primary sources and evidence captured in:

- [`review-rust-tooling.md`](review-rust-tooling.md): Cargo, rustup, and nextest official references;
- [`review-protocol-adrs.md`](review-protocol-adrs.md): Protobuf, W3C WebRTC, and RFC 8827;
- [`review-web-foundation.md`](review-web-foundation.md): npm registry metadata for Astro, TypeScript, and `@astrojs/check`;
- the six task-local implementation research records in this directory.

No review conclusion was accepted without re-running the relevant native repository command.

## Findings and applied corrections

### Rust/domain

1. **Pinned Clippy was missing.** Added `clippy` to the Rust 1.92 minimal toolchain; pinned-toolchain Clippy now passes.
2. **`AudioProfile` allowed invalid post-construction mutation.** Made fields private and added read-only accessors so construction preserves the validated value-object invariant.
3. **V1 DTX-off was documented but unenforced.** Added `DtxUnsupported` and a rejection test.
4. **Nextest policy was path-sensitive and not automatically discovered.** Moved configuration from root `nextest.toml` to upstream's `.config/nextest.toml` location and removed the relative override.
5. **Reproducible Cargo commands did not require the lockfile.** Added `--locked` to repository check/test/lint entry points.

### Web

1. **Astro checker/TypeScript peer range mismatch.** Upgraded `@astrojs/check` from 0.9.6 to 0.9.10 and refreshed the lockfile.
2. **Node baseline was implicit.** Declared `>=22.12.0` and pinned the current development version in `.node-version`.
3. **The placeholder session was constructed during Astro prerender.** Removed build-time construction; browser lifecycle remains intentionally unimplemented.
4. **Source-first `web-rtc` packaging remains provisional.** It is private and validated with `tsc --noEmit`; emitting portable JS/declarations is deferred until a real browser interface exists.

### Protocol/security

1. **Resume/replay exchange was absent.** Added typed initial-join/resume entry, rotated resume token, current revision, ordered replay events, and full-renegotiation outcome.
2. **Signaling security was under-specified.** Strengthened ADR 0003 and added `docs/protocols/signaling-v1.md` with TLS/WSS, ticket/token binding, server-derived identity, authorization, redaction, rate/size, version, and replay requirements.
3. **Capabilities were free-form and incomplete.** Replaced generic codec/maps with typed Opus frame, bitrate, FEC, DTX, TURN-TLS, ICE restart, tracks, and channel capabilities; the RTP clock remains fixed at 48 kHz.
4. **Premature QUIC leaked into V1.** Removed and reserved the value.
5. **ICE presence/generation semantics were lossy.** Added optional locator fields and username fragment plus normative cross-field rules.
6. **Export telemetry used raw session/peer naming.** Changed fields to explicitly short-lived export correlation pseudonyms and documented the export boundary.
7. **Breaking detection has no baseline yet.** Deferred until the first committed schema baseline exists.

### Repository hygiene

The initial root-anchored ignore rules failed for nested pnpm/Astro output, and a review generation run exposed `proto/gen`. Ignore rules now cover nested `node_modules`, `dist`, `.astro`, and generated protocol output; all generated directories were removed after validation.

## Potential corrections to the master plan

1. Show nextest at `.config/nextest.toml`, or require an explicit anchored `--config-file` everywhere. **Applied to repository; master retained as historical specification.**
2. Treat DTX-off as a V1 invariant rather than only a default while `AudioProfile::new` claims V1 validation. **Applied.**
3. Replace generic capability examples with typed microsecond frame durations and typed transport/media flags. **Applied.** Fractional 2.5 ms representation is preserved even though the initial product policy selects 5/10/20 ms.
4. Promote signaling TLS/auth/identity/redaction and resume behavior into the protocol security contract, not only control-plane implementation detail. **Applied.**
5. Pin a Node baseline alongside pnpm/Astro. **Applied.**
6. Keep generated code out of the initial repository until consumers exist. **Applied; generation remains a validation command only.**

## Validation evidence

```text
Rust 1.92.0:
  cargo fmt --all -- --check                                      PASS
  cargo check --workspace --all-targets --all-features --locked  PASS
  cargo test --workspace --all-targets --all-features --locked   PASS (6 tests)
  cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
                                                                  PASS
  cargo nextest run --locked --profile ci                         PASS (6 tests)
  (cd crates/relay-domain && cargo relay-test)                    PASS (6 tests)

Node 24.18.1 / pnpm 11.22.0:
  npx pnpm install --frozen-lockfile                              PASS
  npx pnpm -r run typecheck                                      PASS (0 diagnostics)
  npx pnpm -r run build                                          PASS (1 static page)

Protocol:
  npx @bufbuild/buf format --diff --exit-code                     PASS
  npx @bufbuild/buf lint                                          PASS
  npx @bufbuild/buf build                                         PASS
```

`cargo-deny`, `just`, and `typos` were not installed locally, so they were not falsely reported as executed. Their configuration/native execution remains a CI/bootstrap follow-up.

## Deferred follow-ups

- Complete Phase 0 CI/cross-platform, testkit seed, contract golden-test, and bootstrap tasks before declaring the phase exited.
- Commit a protocol baseline, then enable Buf breaking comparison against main/release.
- Decide whether the private `web-rtc` package remains source-first or emits distribution artifacts.
- Implement `just bootstrap` and CI installation of missing repository tools.
- Begin only the focused Phase 1 audio-lab plan after its research gate.
