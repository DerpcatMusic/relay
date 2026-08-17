# RELAY Phase 0 — Foundation Execution Plan

**Date:** 2026-08-15  
**Parent specification:** [RELAY master plan](2026-08-15-relay-master-plan.md)  
**Status:** In progress — first foundation slice validated 2026-08-16; Phase 0 exit gates remain open

## Objective

Create a reproducible monorepo foundation whose Rust, TypeScript, protocol, documentation, and validation surfaces can evolve independently without introducing product behavior prematurely.

## Mandatory task contract

Every executable task is deliberately narrow and is complete only when it:

1. defines scope and acceptance criteria;
2. researches current primary/upstream sources before implementation;
3. writes `docs/research/<task>.md` with sources, findings, and potential corrections to the master plan;
4. changes only its declared ownership area;
5. records exact validation commands and results;
6. leaves unrelated product phases unimplemented.

Use [`docs/research/TEMPLATE.md`](../research/TEMPLATE.md) for the evidence record.

## Work graph

```text
Repository policy ─┬─> Rust workspace + relay-domain
                   ├─> pnpm workspace + Astro shell
                   ├─> Protobuf/Buf schema skeleton
                   ├─> repository tooling
                   └─> foundational ADRs

All foundation tasks ─> integrated Phase 0 verification ─> first baseline commit
```

The five foundation branches are independent at file level and may run concurrently. Integrated verification starts only after their evidence records exist.

## Focused tasks

### F0.1 — Repository policy and evidence contract

**Owns:** `README.md`, `docs/plans/`, `docs/research/README.md`, `docs/research/TEMPLATE.md`  
**Acceptance:** master plan preserved; this execution plan exists; every later task has a mandatory evidence format.

### F0.2 — Rust workspace and boring domain center

**Owns:** root Rust manifests/toolchain and `crates/relay-domain/`  
**Acceptance:** workspace metadata resolves; domain types have no async/platform dependencies; unsafe is forbidden; unit tests pass; research record exists.

### F0.3 — Protocol schema skeleton

**Owns:** `proto/`  
**Acceptance:** Buf configuration uses the current schema; V1 package and envelopes follow compatible evolution rules; lint passes when Buf is available; no generated files are committed; research record exists.

### F0.4 — Web workspace shell

**Owns:** root pnpm manifests, `apps/web/`, `packages/web-rtc/`  
**Acceptance:** dependency graph installs from a lockfile; Astro build succeeds; WebRTC module remains framework-independent and behavior-free; research record exists.

### F0.5 — Repository developer tooling

**Owns:** `justfile`, `.cargo/config.toml`, `deny.toml`, `nextest.toml`, `typos.toml`, `.editorconfig`  
**Acceptance:** configs parse with available native tools; missing tools are documented rather than silently assumed; research record exists.

### F0.6 — Foundational ADRs

**Owns:** `docs/adr/0001` through `0006`  
**Acceptance:** each ADR records context, decision, consequences, and validation gates; provisional choices remain acceptance-gated; research record exists.

### F0.7 — Integrated verification

**Owns:** fixes required to integrate F0.1–F0.6; does not add product behavior.  
**Acceptance:** run the available root checks; inspect dependency direction; review all proposed corrections; record `docs/research/phase-0-integration.md`; clean Git status contains only intended source files.

## Exit gate

Phase 0 is complete only when:

- Rust workspace checks and tests pass on the local platform;
- the web workspace installs and builds using its lockfile;
- protocol and repository configs validate with available tools;
- all foundation research records identify potential corrections explicitly;
- the next executable plan is limited to Phase 1 audio-lab work;
- no plugin, transport provider, billing provider, or production backend has been prematurely coupled into the core.

## Deferred plans

Create separate executable plans after their prerequisite gates:

1. Phase 1 audio engine and deterministic audio-lab;
2. Phase 2 native transport bake-off and probe;
3. Phase 3 standalone Connect;
4. Phase 4 browser Link and signaling;
5. Phase 5 plugin shell;
6. Phase 6 TURN and route policy;
7. Phase 7 fan-out;
8. authentication, billing, provider insurance, hardening, and release plans.


## Remaining Phase 0 exit work

The first integrated slice does **not** complete the master plan's full Phase 0. The following focused tasks remain:

### F0.8 — CI and cross-platform proof

Create the initial Rust, web, and contract workflows with pinned tool installation. Prove the basic Rust workspace on Ubuntu, Windows, and macOS rather than inferring portability from Linux.

### F0.9 — Testkit seed

Add only the deterministic Phase-0 test seams needed by Phase 1: a fake monotonic clock and deterministic audio source/sink vocabulary. Network simulation remains part of the audio/transport plans.

### F0.10 — Contract generation/golden boundary

Choose generated-code ownership and add Rust/TypeScript golden-message tests before application consumers depend on the schema. Establish the first Buf breaking baseline only after that contract is committed.

### F0.11 — Bootstrap and repository policy

Implement `just bootstrap`, add CODEOWNERS/CI ownership, and execute `cargo-deny` and `typos` in the managed environment. Record their versions and results.

Phase 0 exits only after F0.8–F0.11 and the master plan's three-OS compile gate pass.
