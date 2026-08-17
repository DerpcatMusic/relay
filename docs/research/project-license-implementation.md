# Project license implementation

## Primary sources

1. [Mozilla Public License 2.0](https://www.mozilla.org/MPL/2.0/) — Mozilla's official license page and its linked [plain-text license](https://www.mozilla.org/media/MPL/2.0/index.815ca599c9df.txt).
2. [Cargo workspaces: the `package` table](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table) — Cargo documents `license` as inheritable with `license.workspace = true`.
3. [npm `package.json`: `license`](https://docs.npmjs.com/cli/v11/configuring-npm/package-json#license) — npm requires a single SPDX license expression in the `license` field.

## Potential corrections

- Do not use a paraphrase, a short notice, `MPL-2.0-only`, or a legacy license object: the approved identifier is the SPDX expression `MPL-2.0`, and the repository needs the complete canonical text.
- Do not duplicate the Rust license string in member manifests. Workspace inheritance keeps one authoritative value while ensuring every package reports it through Cargo metadata.
- Private npm packages still describe project artifacts, so all four project-owned manifests should agree with the root license. Dependency manifests under `node_modules` are not project metadata and must not be edited.
- The dependency-license allowlist must include the project's approved MPL-2.0 expression; all other policy remains intact.

## Decisions

- Installed Mozilla's canonical MPL 2.0 plain text verbatim as root `LICENSE` (SHA-256 `fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85`).
- Declared `license = "MPL-2.0"` in `[workspace.package]` and `license.workspace = true` in all twelve current Rust packages.
- Declared `"license": "MPL-2.0"` in the root, web app, web RTC, and protocol npm manifests.
- Added only `MPL-2.0` to the cargo-deny license allowlist. A subsequent cargo-deny 0.20.2 bans run exposed wildcard internal path dependencies, so every internal path edge now also declares exact version `=0.1.0`; no external dependency range was widened. Audio source, generated files, and all other policy remain unchanged.

## Exact validation

Run from the repository root:

```sh
curl -fsSL https://www.mozilla.org/media/MPL/2.0/index.815ca599c9df.txt -o /tmp/MPL-2.0.txt
cmp --silent LICENSE /tmp/MPL-2.0.txt
sha256sum LICENSE
cargo metadata --locked --format-version 1
cargo check --locked --workspace --all-targets --all-features
/tmp/cargo-deny-0.20.2/.../cargo-deny check licenses
/tmp/cargo-deny-0.20.2/.../cargo-deny check advisories
/tmp/cargo-deny-0.20.2/.../cargo-deny check sources
/tmp/cargo-deny-0.20.2/.../cargo-deny check bans
npx --yes pnpm@11.22.0 install --frozen-lockfile
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```

Also parse the Cargo metadata result to assert that the twelve workspace packages all report `MPL-2.0`, and parse the four project-owned `package.json` files to assert the same exact identifier. Results are recorded after execution below.

### Results

- Canonical `LICENSE` comparison: **PASS**; SHA-256
  `fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85`.
- Locked Cargo metadata: **PASS**; all 12 workspace packages report `MPL-2.0`.
- npm manifest parse: **PASS**; all 4 project-owned packages report `MPL-2.0`.
- Frozen pnpm 11.22.0 install: **PASS**.
- cargo-deny 0.20.2 `licenses`, `advisories`, `sources`, and `bans`: **PASS**.
  The first bans run correctly rejected unversioned internal path dependencies;
  after adding exact `=0.1.0` versions, the second run passed without weakening
  `wildcards = "deny"`.
- Locked workspace check and strict Clippy were run by the implementation agent
  before the final exact-version correction and passed. They are rerun as part
  of the next coherent audio integration gate.

## Disposition

The prior missing-project-license blocker is resolved. Cargo/npm metadata and the
root canonical text agree on MPL-2.0, and the unchanged strict cargo-deny policy
passes all four check classes with cargo-deny 0.20.2.

