# CI foundation — Research and implementation evidence

**Date:** 2026-08-16  
**Task owner:** Prime Agent  
**Status:** Complete

## Scope

Create only the initial Phase-0 GitHub Actions workflows:

- `.github/workflows/ci-rust.yml`
- `.github/workflows/ci-web.yml`
- `.github/workflows/ci-contracts.yml`

This task also owns this evidence record. Non-goals were manifest, source, `justfile`,
CODEOWNERS, protocol, generated-code, release, deployment, and product changes.

## Acceptance criteria

- [x] Rust formatting, checking, tests, and Clippy run on Ubuntu, Windows, and macOS.
- [x] The portable Rust matrix uses a pinned cargo-nextest installation.
- [x] Web CI uses the repository's exact Node and pnpm versions, a frozen install,
      typechecking, and a production build.
- [x] Contract CI runs Buf format, lint, and build with an exact Buf CLI version.
- [x] Buf breaking detection is not enabled before a committed baseline exists.
- [x] Every action reference is pinned to a major release line.
- [x] Workflow token permissions are read-only and explicitly declared.
- [x] The YAML parses locally and passes actionlint.

## Sources consulted

Research stopped after four upstream action source sets.

| Source | Why it is authoritative | Accessed |
|---|---|---|
| <https://github.com/actions/setup-node/tree/v4> | The action's own README and metadata define `node-version-file`, pnpm caching, and `cache-dependency-path`. | 2026-08-16 |
| <https://github.com/pnpm/action-setup/tree/v4> | pnpm's maintained action documents exact-version installation and the relationship with the root `packageManager` field. | 2026-08-16 |
| <https://github.com/taiki-e/install-action/tree/v2> | The install action documents exact `tool@version` pins and Ubuntu, macOS, and Windows support; its supported-tool table lists Rust and cargo-nextest on all three. | 2026-08-16 |
| <https://github.com/bufbuild/buf-action/tree/v1> | Buf's maintained action defines exact CLI version selection, setup-only operation, and controls for comments/push/breaking behavior. | 2026-08-16 |

Existing repository evidence supplied the selected cargo-nextest `0.9.143` release
(`phase-0-stack-validation.md`) and the already-pinned Rust, Node, pnpm, and Buf
configuration. No fifth action/tool source was added.

## Findings

- `rust-toolchain.toml` pins Rust `1.92.0` and includes `rustfmt` and `clippy`.
  The workflow repeats that exact toolchain pin in the installer and pins
  cargo-nextest `0.9.143`; the installer supports GitHub-hosted Linux, macOS,
  and Windows runners.
- `.config/nextest.toml` is in nextest's discovery location and contains a `ci`
  profile. Therefore the matrix can run `cargo nextest run --profile ci`
  without a path-sensitive override.
- `.node-version` pins Node `24.18.1`, while root `package.json` pins
  `pnpm@11.22.0`. The web workflow uses both exact values and hashes
  `pnpm-lock.yaml` for setup-node's pnpm store cache. The cache does not replace
  `pnpm install --frozen-lockfile`.
- The old `buf-setup-action` is deprecated. The consolidated `buf-action@v1`
  can be restricted to `setup_only: true`; this installs Buf without enabling
  its default publishing, PR comments, or breaking checks. Buf CLI `1.72.0` is
  pinned explicitly.
- There is no committed schema baseline: the repository has no initial commit,
  and Phase F0.10 deliberately owns establishment of the first contract
  baseline. Running `buf breaking` now would produce a false foundation rather
  than compare two published contract states.
- All three workflows need only repository reads. Each therefore declares only
  `permissions: contents: read`; Buf PR commenting is disabled explicitly.

## Potential corrections to the master plan

1. **Use the consolidated Buf action.** Any implementation guidance that assumes
   `buf-setup-action` should be corrected because upstream marks it deprecated.
   **Impact:** adopting a deprecated installer adds avoidable maintenance risk.
   **Disposition:** applied `bufbuild/buf-action@v1` in setup-only mode.
2. **Do not infer a Buf breaking baseline from the first uncommitted tree.** The
   Phase-0 plan already assigns the first baseline to F0.10, so F0.8 must not
   enable breaking detection early. **Impact:** a premature comparison is
   meaningless or fails because no Git base exists. **Disposition:** format,
   lint, and build are enabled now; the workflow contains an explicit comment
   marking breaking detection as deferred until the first schema baseline.
3. **Cross-platform proof requires hosted execution.** Local Linux success
   cannot satisfy the plan's three-OS proof by itself. **Impact:** portability
   remains an open exit gate until GitHub runs the new matrix. **Disposition:**
   the three-runner matrix is implemented; actual hosted results are deferred,
   not claimed.

## Decisions applied

- Trigger each independent workflow on both pushes and pull requests.
- Cancel superseded runs for the same workflow/ref and use bounded job timeouts.
- Run the complete Rust gate on each of `ubuntu-latest`, `windows-latest`, and
  `macos-latest`: format, check, nextest, and Clippy with warnings denied.
- Use workspace-wide Cargo selectors with `--locked`, all targets, and all
  features so new workspace members do not silently escape CI.
- Install exact Rust/cargo-nextest, Node/pnpm, and Buf CLI versions while pinning
  third-party actions to their requested major release lines.
- Keep contract checks as explicit CLI steps so the absence of a breaking step
  is visible and auditable.

## Validation evidence

### YAML syntax and GitHub Actions semantics

```text
$ ruby -e 'require "yaml"; ARGV.each { |f| YAML.safe_load_file(f, permitted_classes: [], aliases: false); puts "#{f}: valid YAML" }' .github/workflows/*.yml
.github/workflows/ci-contracts.yml: valid YAML
.github/workflows/ci-rust.yml: valid YAML
.github/workflows/ci-web.yml: valid YAML
```

```text
$ GOBIN=<temporary-directory> go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.7
$ <temporary-directory>/actionlint .github/workflows/*.yml
exit 0; no diagnostics
```

The first shell attempt scoped `GOBIN` only to `go install` and then tried the
empty shell variable path `/actionlint`; validation was corrected by storing the
temporary directory in a separate shell variable, passing it as `GOBIN`, and
invoking that explicit path. This was a validation-command correction only; no
workflow content changed because of it.

### Rust commands on the available Linux host

```text
$ rustc --version
rustc 1.92.0 (ded5c06cf 2025-12-08)

$ cargo nextest --version
cargo-nextest 0.9.140 (local executable; CI pins 0.9.143)

$ cargo fmt --all -- --check
exit 0

$ cargo check --locked --workspace --all-targets --all-features
exit 0

$ cargo nextest run --locked --profile ci --workspace --all-targets --all-features
exit 0; 6 tests passed

$ cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
exit 0
```

### Web commands with the repository pins

```text
$ node --version
v24.18.1

$ npx --yes pnpm@11.22.0 --version
11.22.0

$ npx --yes pnpm@11.22.0 install --frozen-lockfile
exit 0; lockfile already up to date

$ npx --yes pnpm@11.22.0 typecheck
exit 0; web-rtc TypeScript and Astro checks passed with 0 diagnostics

$ npx --yes pnpm@11.22.0 build
exit 0; web-rtc validation and Astro static build passed
```

### Contract commands with the CI-pinned Buf version

```text
$ go run github.com/bufbuild/buf/cmd/buf@v1.72.0 --version
1.72.0

$ go run github.com/bufbuild/buf/cmd/buf@v1.72.0 format --diff --exit-code proto
exit 0; no formatting diff

$ go run github.com/bufbuild/buf/cmd/buf@v1.72.0 lint proto
exit 0

$ go run github.com/bufbuild/buf/cmd/buf@v1.72.0 build proto
exit 0
```

## Limitations

- GitHub Actions itself was not run from this local environment. Action download,
  cache behavior, and the exact pinned action/tool installation paths remain to
  be proven by the first hosted run.
- Only Linux commands were executable locally. Windows and macOS proof is the
  purpose of the new matrix and must not be claimed before hosted results exist.
- Local Rust execution used cargo-nextest `0.9.140`; CI installation of the
  pinned `0.9.143` remains a hosted-run check.
- Buf was validated with the exact CLI version through `go run`, not through
  `buf-action`; setup-only action behavior remains a hosted-run check.
- Buf breaking detection is intentionally absent until F0.10 creates a real,
  committed comparison baseline.

## Deferred follow-ups

- Observe the first hosted run on all three Rust operating systems and correct
  only evidence-backed portability failures.
- Add Buf breaking detection in the task that establishes the first committed
  schema baseline, with an explicit and stable `--against` target.
- Add CODEOWNERS/CI ownership only in its separately scoped repository-policy
  task.
