# Bootstrap and Repository Policy Research

**Date:** 2026-08-16  
**Task owner:** Prime Agent  
**Status:** Complete with one manifest-owned policy blocker

## Scope

Implement only the Phase-0 repository bootstrap and ownership policy:

- add `.github/CODEOWNERS`, including explicit automation ownership;
- make the root `justfile` expose `bootstrap`, `check`, `test`, and `contracts` entry points;
- pin non-toolchain CLIs and keep installation separate from validation;
- execute or parse the policy surfaces without changing CI workflows, Cargo/package manifests, product source, or Protobuf definitions.

No bootstrap shell script was added because Cargo and pinned `npx` invocations are sufficient. No command pipes a network response into a shell.

## Acceptance criteria

- [x] `just bootstrap` installs pinned Rust CLI tools and the frozen pnpm dependency graph without running repository validation.
- [x] Repeating dependency bootstrap is safe and makes no lockfile change.
- [x] `just check`, `just test`, and `just contracts` are explicit repository entry points.
- [x] pnpm `11.22.0`, Buf `1.72.0`, cargo-nextest `0.9.143`, cargo-deny `0.20.2`, and typos-cli `1.49.0` are literal repository pins.
- [x] `.github/CODEOWNERS` assigns a default owner and explicit `.github/workflows/` ownership.
- [x] `typos` executes successfully in an isolated pinned tool environment.
- [ ] `cargo deny check` is clean. The command executed, but correctly failed because the workspace crates have no license metadata; manifests are outside this task's allowed edits.

## Primary sources consulted

Exactly four primary tool/platform documents were consulted.

| Source | Finding used | Accessed |
|---|---|---|
| [GitHub: About code owners](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners) | GitHub recognizes `.github/CODEOWNERS`, uses the last matching pattern, and requires named users/teams to have repository write access. | 2026-08-16 |
| [just manual](https://just.systems/man/en/) | A justfile can define variables, recipe dependencies, a strict shell, and discoverable recipe documentation. | 2026-08-16 |
| [Cargo: `cargo install`](https://doc.rust-lang.org/cargo/commands/cargo-install.html) | `--locked` uses the published lockfile when present; Cargo skips an installed package when its version/source/features/profile/target are already current and reinstalls only when those inputs differ. | 2026-08-16 |
| [npm: `npx`](https://docs.npmjs.com/cli/v11/commands/npx/) | An exact package specifier selects that exact version, and `--yes` suppresses the install prompt for unattended execution. | 2026-08-16 |

Version selection also reuses the repository's already-reviewed evidence in `docs/research/phase-0-stack-validation.md`; that local evidence is not an additional external source.

## Findings and decisions

### Bootstrap is installation, not validation

`bootstrap` depends only on:

1. `bootstrap-tools`, which runs pinned, locked `cargo install` commands for nextest, cargo-deny, and typos-cli; and
2. `bootstrap-dependencies`, which uses the package-manager pin already declared by the repository to run `pnpm install --frozen-lockfile` through `npx`.

It deliberately does not invoke `check`, `test`, `contracts`, cargo-deny, or typos. The completion message directs the developer to those separate validation recipes. Re-running the frozen pnpm installation twice reported `Already up to date`; Cargo's documented install freshness behavior makes the exact-version tool installs idempotent as well.

The pinned Rust toolchain supplies Cargo, rustfmt, and Clippy. Buf and pnpm run through exact-version `npx` specs rather than requiring mutable global npm installs.

### Repository entry points

- `check` aggregates Rust formatting/check/lint, dependency policy, web typechecking, contract validation, and spelling.
- `test` aggregates workspace-wide nextest and any web package test scripts.
- `contracts` uses the pinned Buf CLI for formatting/lint/build/generation and checks generated-output drift.
- Rust compile/lint/test recipes cover the whole Cargo workspace rather than only the original `relay-domain` member.

### Ownership policy

`CODEOWNERS` gives all paths a default `@DerpcatMusic` owner, then makes automation, repository policy, architecture, contract, and implementation surfaces explicit. The repository has no configured Git remote, so GitHub could not verify that account's write access locally.

### Typos policy

Pinned typos-cli initially reported false positives inside generated Protobuf descriptor strings and the recorded Rust commit hash `ded5c06cf`. `typos.toml` now excludes the generated protocol directory and ignores that exact immutable hash. A second pinned execution passed. This does not suppress ordinary prose or source-code spelling checks.

### Cargo dependency policy blocker

Pinned cargo-deny executed all four policy families. Advisories, bans, and sources passed; licenses failed because `relay-domain`, `relay-protocol`, and `relay-testkit` had no license expressions. The now-expanded workspace should be given deliberate workspace-level license metadata in a separately owned Cargo-manifest task rather than weakening `deny.toml` or excluding first-party crates from policy.

## Native tool availability

Before isolated validation, PATH contained:

```text
just: unavailable
cargo-deny: unavailable
typos: unavailable
cargo: 1.92.0
node: 24.18.1
npx: 12.0.2
pnpm: unavailable
buf: unavailable
cargo-nextest: 0.9.140
```

For syntax and policy validation only, pinned binaries were installed under `/tmp/relay-bootstrap-just` and `/tmp/relay-policy-tools`. Those isolated locations were not added to PATH and did not convert the missing native tools into global prerequisites.

## Exact validation evidence

### justfile syntax and expansion

```text
$ /tmp/relay-bootstrap-just/bin/just --version
just 1.58.0

$ /tmp/relay-bootstrap-just/bin/just --fmt --check
exit 0

$ /tmp/relay-bootstrap-just/bin/just --list
exit 0; listed bootstrap, check, test, contracts, and their component recipes

$ /tmp/relay-bootstrap-just/bin/just --dry-run bootstrap
cargo install --locked --version 0.9.143 cargo-nextest
cargo install --locked --version 0.20.2 cargo-deny
cargo install --locked --version 1.49.0 typos-cli
npx --yes pnpm@11.22.0 install --frozen-lockfile
exit 0
```

### Idempotent dependency bootstrap

```text
$ /tmp/relay-bootstrap-just/bin/just bootstrap-dependencies
Lockfile is up to date, resolution step is skipped
Already up to date
exit 0

$ /tmp/relay-bootstrap-just/bin/just bootstrap-dependencies
Already up to date
exit 0
```

### Rust and web validation

```text
$ cargo fmt --all -- --check
exit 0

$ cargo check --locked --workspace --all-targets --all-features
exit 0

$ cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
exit 0

$ cargo nextest run --locked --workspace --all-targets --all-features
exit 0; 18 tests passed

$ npx --yes pnpm@11.22.0 -r --if-present run test
exit 0; TypeScript golden test passed

$ npx --yes pnpm@11.22.0 -r run typecheck
exit 0; all three package scripts passed, Astro reported zero diagnostics
```

### Contract and repository-policy validation

The non-generating Buf checks were executed directly to respect this task's prohibition on editing generated source:

```text
$ cd proto && npx --yes @bufbuild/buf@1.72.0 lint
exit 0

$ cd proto && npx --yes @bufbuild/buf@1.72.0 build
exit 0

$ /tmp/relay-policy-tools/bin/typos --version

 typos-cli 1.49.0

$ /tmp/relay-policy-tools/bin/typos
exit 0

$ /tmp/relay-policy-tools/bin/cargo-deny --version
cargo-deny 0.20.2

$ /tmp/relay-policy-tools/bin/cargo-deny check
exit 4; advisories ok, bans ok, sources ok, licenses failed because relay-domain, relay-protocol, and relay-testkit 0.1.0 are unlicensed
```

TOML parsing with Python 3.11 `tomllib` passed for `deny.toml`, `typos.toml`, `.cargo/config.toml`, and `.config/nextest.toml`.

## Potential corrections to the master plan

1. **Document the `just` prerequisite.** The master plan begins developer UX with `just bootstrap`, but `just` cannot install itself from inside that recipe. Add a prerequisite statement (Cargo-based installation is sufficient); a second bootstrap shell is not justified.
2. **Add deliberate workspace license metadata before treating cargo-deny as green.** The Phase-0 policy gate currently exposes a real manifest omission. Correct the Cargo manifests in their own ownership scope; do not bypass the license check.
3. **Verify the CODEOWNERS identity after attaching the GitHub remote.** `@DerpcatMusic` matches the local Git identity, but GitHub write access cannot be proved without repository metadata.
4. **Generated drift requires a tracked baseline.** `git diff --exit-code` cannot detect wholly untracked generated files before the first baseline commit. The contract-generation task must establish tracked ownership before relying on that drift gate.

## Changed files owned by this task

- `.github/CODEOWNERS`
- `justfile`
- `typos.toml`
- `docs/research/bootstrap-policy.md`

No CI workflow, Cargo/package manifest, product source file, or Protobuf definition was edited by this task.
