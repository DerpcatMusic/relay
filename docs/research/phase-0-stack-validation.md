# Phase 0 stack validation

**Checked:** 2026-08-15 UTC  
**Status:** Complete for Phase 0 foundation selection

> [!IMPORTANT]
> Version and “latest” claims are time-sensitive. In this record, **current upstream** means the version reported by an authoritative upstream source on the checked date; **installed locally** means an executable or package actually observed in this checkout. A manifest pin is a **selected version**, not proof that its executable is installed.

## Scope

Validate only the Phase 0 foundation choices that are sensitive to current upstream state:

- stable Rust and the Rust floor implied by Truce;
- Astro and pnpm;
- the Buf CLI and v2 configuration;
- `cargo-nextest`, `cargo-llvm-cov`, and `cargo-deny`.

This task does not adopt Truce, alter product architecture, edit manifests or implementation files, or promise that a dated version remains “latest.” Truce remains provisional and acceptance-gated elsewhere in the plan.

## Acceptance criteria

- [x] Use primary/upstream sources and record their retrieval date.
- [x] Distinguish current upstream, selected/pinned, and installed-local versions.
- [x] Verify that the proposed Rust floor is compatible with Truce's published install requirement.
- [x] Verify that the checked-in Astro and pnpm selections match current stable releases on the checked date.
- [x] Verify that the Buf configuration uses the current v2 schema and recognized policy categories.
- [x] Verify current releases and Rust requirements for the three Rust QA tools.
- [x] State explicit potential corrections and final Phase 0 decisions.
- [x] Change documentation only.

## Primary/upstream sources

Exactly five targeted upstream checks were made.

| Check | Primary/upstream sources | Retrieved | Evidence used |
|---|---|---:|---|
| 1. Rust + Truce | [Rust stable channel manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml); [Truce install guide](https://truce.audio/docs/guide/install/) | 2026-08-15 | Rust stable was `1.97.1`; Truce says “Install Rust 1.92+.” |
| 2. Astro | [Astro `latest` package metadata](https://registry.npmjs.org/astro/latest); [official upgrade guide](https://docs.astro.build/en/upgrade-astro/) | 2026-08-15 | Stable package was `7.2.2`; its engine metadata requires Node `>=22.12.0`, npm `>=9.6.5`, or pnpm `>=7.1.0`. |
| 3. pnpm | [pnpm `latest` package metadata](https://registry.npmjs.org/pnpm/latest); [official installation guide](https://pnpm.io/installation); [official CI guide](https://pnpm.io/continuous-integration) | 2026-08-15 | Stable package was `11.22.0` with Node `>=22.13`; pnpm 12 was still labeled a release candidate. Current CI guidance installs pnpm itself rather than using Corepack. |
| 4. Buf | [official latest release](https://github.com/bufbuild/buf/releases/tag/v1.72.0); [official v2 `buf.yaml` reference](https://buf.build/docs/configuration/v2/buf-yaml/) | 2026-08-15 | CLI release was `v1.72.0`; v2 config supports `STANDARD` lint and `FILE` breaking policy. |
| 5. Rust QA tools | Official releases/tagged manifests: [nextest `0.9.143`](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.143) / [manifest](https://github.com/nextest-rs/nextest/blob/cargo-nextest-0.9.143/Cargo.toml), [llvm-cov `0.8.7`](https://github.com/taiki-e/cargo-llvm-cov/releases/tag/v0.8.7) / [manifest](https://github.com/taiki-e/cargo-llvm-cov/blob/v0.8.7/Cargo.toml), [deny `0.20.2`](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.20.2) / [manifest](https://github.com/EmbarkStudios/cargo-deny/blob/0.20.2/Cargo.toml) | 2026-08-15 | Releases and declared Rust requirements recorded below. |

The npm registry links above are the packages' published distribution metadata. GitHub release and tagged-manifest links point to the tools' upstream repositories. Mutable `latest`/stable endpoints are dated deliberately.

## Findings

### 1. Rust stable and Truce's Rust floor

The current stable channel reported `rustc 1.97.1 (8bab26f4f 2026-07-14)`. The repository selects and locally resolves `1.92.0`, while the official Truce install guide requires **Rust 1.92+**. Therefore:

- the repository's `rust-version = "1.92"` and exact `1.92.0` toolchain satisfy Truce's published installation floor;
- `1.92.0` is a stable Rust release, but it is **not current stable** as of this check;
- “stable channel,” “current stable,” and “MSRV” must not be used interchangeably;
- the guide provides an installation floor, not a durable guarantee for every future Truce release. Recheck the selected Truce revision before adoption.

The Phase 0 choice of Rust 1.92 as the declared compatibility floor is coherent. If the project promises MSRV support, CI should eventually test the exact floor and a current-stable lane. This validation does not require adding Truce during Phase 0.

### 2. Astro

Astro `7.2.2` was the current stable registry version and is both selected and present in `apps/web/node_modules`. The master plan's **Astro 7** choice and the exact `7.2.2` selection are current on the checked date.

The package's published engine requirements also reveal a cross-stack constraint: pnpm 11 requires Node `>=22.13`, which is slightly stricter than Astro's Node `>=22.12.0`. If Node is pinned later, use at least `22.13` for this exact pair.

### 3. pnpm

pnpm `11.22.0` was the current stable registry version; pnpm 12 remained a release candidate and is not the stable comparison point. The root `packageManager: "pnpm@11.22.0"` selection is therefore current on the checked date.

No `pnpm` executable was found on this environment's `PATH`. That does **not** invalidate the selected version or lockfile, but it means “pnpm 11.22.0 is installed locally” would be false.

The initial draft's Corepack recommendation needed correction. The current official pnpm CI guide says earlier examples used Corepack, but now recommends installing pnpm itself to avoid a Node shim on every invocation. Keep the single version authority in `packageManager`, and follow the current official CI mechanism when CI is added rather than hard-coding Corepack as policy.

### 4. Buf

Buf CLI `v1.72.0` was current upstream. This does not conflict with `version: v2` in `proto/buf.yaml`: **CLI release v1.x and configuration schema v2 are separate version domains**.

The checked-in configuration matches the official v2 reference:

- `version: v2` is the current configuration family;
- `STANDARD` is a recognized lint category;
- `FILE` is a recognized breaking-change category.

No `buf` executable was found locally, and the repository does not yet pin a Buf CLI version. Configuration inspection can be completed now; executable lint/breaking validation remains an environment/CI setup concern.

### 5. Rust test, coverage, and dependency-policy tools

| Tool | Current upstream | Declared Rust requirement | Installed locally | Compatibility with project Rust 1.92 |
|---|---:|---:|---:|---|
| `cargo-nextest` | `0.9.143` | `1.91` | `0.9.140` | Yes |
| `cargo-llvm-cov` | `0.8.7` | `1.87` | Not installed | Yes |
| `cargo-deny` | `0.20.2` | `1.88.0` | Not installed | Yes |

All three current releases can run with the selected Rust 1.92 toolchain according to their tagged manifests. The earlier general warning that CI utilities *might* require a newer compiler remains good policy guidance, but it is not an observed blocker for these checked releases.

`cargo-nextest` is suitable as the preferred CI runner while ordinary `cargo test` remains useful as a baseline compatibility path. `cargo-llvm-cov` remains suitable for LLVM coverage and requires the appropriate `llvm-tools` Rust component when executed. `cargo-deny` is suitable for advisory, license, ban, and source policy; its checked-in policy should be treated as project policy rather than an upstream default.

## Installed, current, and selected inventory

| Tool | Installed locally | Current upstream on 2026-08-15 | Selected/pinned in repository | Result |
|---|---:|---:|---:|---|
| Rust | `1.92.0` | `1.97.1` stable | toolchain `1.92.0`; `rust-version = "1.92"` | Compatible with Truce's published `1.92+` floor; not current stable. |
| Astro | `7.2.2` package present | `7.2.2` | `7.2.2` | Matches. |
| pnpm | Not found on `PATH` | `11.22.0` stable | `packageManager` `11.22.0` | Selection matches; installation not proven. |
| Buf | Not found on `PATH` | CLI `v1.72.0` | No CLI pin; config schema `v2` | Schema is current; CLI validation unavailable locally. |
| `cargo-nextest` | `0.9.140` | `0.9.143` | Config exists; executable unpinned | Installed version is older than current. |
| `cargo-llvm-cov` | Not installed | `0.8.7` | No executable pin | Install/pin before coverage CI. |
| `cargo-deny` | Not installed | `0.20.2` | Policy exists; executable unpinned | Install/pin before audit CI. |

## Explicit potential corrections

1. **Clarify the master plan's Rust wording.** “Rust 1.92+ for Truce” is supported, but `1.92.0` should be described as the selected floor/toolchain, not as current stable. Current stable was `1.97.1` on the check date.
2. **Do not turn Truce's present install guide into an indefinite MSRV guarantee.** Recheck the exact Truce revision when the provisional plugin-shell bakeoff reaches adoption.
3. **Keep Astro 7 / `7.2.2`.** No correction was required on the checked date, but the exact-current claim is dated.
4. **Keep pnpm `11.22.0`; do not jump to pnpm 12 while it is an RC.** Also require Node `>=22.13` if a Node pin is introduced.
5. **Replace “use Corepack” guidance with “follow pnpm's current official CI installation guidance.”** `packageManager` remains the repository's version authority; it does not prove local installation.
6. **Do not “correct” `buf.yaml` from v2 to v1 because the Buf CLI is v1.x.** Those numbers describe different things. The actual gap is the missing CLI pin/install.
7. **Pin CI utility versions or immutable installer/action revisions when CI is created.** The repository currently has policies/configuration but not executable pins for Buf, nextest, llvm-cov, or deny.
8. **Do not claim that current QA tools exceed the project MSRV.** The checked releases declare Rust 1.91, 1.87, and 1.88.0 respectively, all within Rust 1.92.

## Decisions

| Decision | Status | Reason |
|---|---|---|
| Select Rust `1.92` as the Phase 0 compatibility floor and exact `1.92.0` toolchain | Accepted | Meets Truce's published `1.92+` requirement; current-stable testing can be a separate lane. |
| Keep Truce provisional and outside the Phase 0 core | Accepted | The Rust prerequisite is validated, but provider acceptance still requires the later bakeoff. |
| Use Astro `7.2.2` | Accepted as a dated pin | It matched current stable and the local package on the check date. |
| Use pnpm `11.22.0` via the root `packageManager` field | Accepted as a dated pin | It matched current stable; pnpm 12 was an RC. Use current official CI setup rather than mandating Corepack. |
| Use Buf v2 configuration with `STANDARD` lint and `FILE` breaking policy | Accepted | Matches the official v2 reference. Pin/install the CLI separately. |
| Use nextest, llvm-cov, and deny as repository/CI tools | Accepted | Current releases are maintained and compatible with Rust 1.92; executable pins remain follow-up work. |

## Validation record

### Upstream checks

All upstream endpoints queried during the five targeted checks returned HTTP 200. The source table uses human-readable release/tag links corresponding to the queried release APIs and tagged raw manifests. Parsed results:

```text
Rust stable:          1.97.1 (8bab26f4f 2026-07-14)
Truce install floor:  Rust 1.92+
Astro latest stable:  7.2.2
pnpm latest stable:   11.22.0 (pnpm 12 documented as RC)
Buf latest CLI:       v1.72.0
cargo-nextest:        0.9.143; rust-version 1.91
cargo-llvm-cov:       0.8.7; rust-version 1.87
cargo-deny:           0.20.2; rust-version 1.88.0
```

### Local observations

```text
$ rustc --version
rustc 1.92.0 (ded5c06cf 2025-12-08)

$ cargo nextest --version
cargo-nextest 0.9.140 (a9fef2964 2026-07-05)

$ pnpm --version
not found on PATH

$ buf --version
not found on PATH

$ cargo llvm-cov --version
error: no such command: llvm-cov

$ cargo deny --version
error: no such command: deny
```

Astro installation was observed separately from `apps/web/node_modules/astro/package.json` as `7.2.2`, because the missing pnpm executable prevented using `pnpm exec`.

### Outcome

**Pass with documented follow-ups.** The Phase 0 stack is internally coherent, the checked selections match upstream constraints, and the plan's exact version claims are now dated and separated from local installation state. Follow-up CI work should pin/install Buf and the Rust QA utilities, add the appropriate `llvm-tools` component for coverage, and consider separate Rust-floor/current-stable lanes. No implementation file was changed by this validation task.
