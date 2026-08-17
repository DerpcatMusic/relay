# Repository tooling foundation — Research and implementation evidence

**Date:** 2026-08-15  
**Task owner:** Prime Agent  
**Status:** Complete

## Scope

Establish the Phase 0 repository-owned developer-tool configuration in `justfile`,
`.cargo/config.toml`, `deny.toml`, `nextest.toml`, `typos.toml`, and
`.editorconfig`.

Non-goals: changing Rust or JavaScript manifests, source code, protocol files,
CI, generated files, or installing missing global tools.

## Acceptance criteria

- [x] Rust recipes and Cargo aliases select the `relay-domain` workspace member.
- [x] JavaScript recipes invoke existing package scripts recursively through pnpm.
- [x] The six repository configuration files exist and parse where a native tool is available.
- [x] Missing validators and blocked checks are recorded explicitly.
- [x] No manifest, product code, protocol, or CI file was changed for this task.

## Sources consulted

Exactly three targeted upstream configuration sources were checked.

| Source | Why it is authoritative | Accessed |
|---|---|---|
| <https://embarkstudios.github.io/cargo-deny/checks/index.html> | cargo-deny's project documentation defines its check families and states that `cargo deny check` runs all supported checks. | 2026-08-15 |
| <https://nexte.st/docs/configuration/> | cargo-nextest's project documentation defines repository profiles, CI settings, the default `.config/nextest.toml` location, and the `--config-file` override. | 2026-08-15 |
| <https://pnpm.io/cli/recursive> | pnpm's official CLI reference defines `-r`/`--recursive` behavior for workspace scripts. | 2026-08-15 |

## Findings

- `cargo deny check` is the upstream command for applying all configured dependency checks, so the root recipe does not duplicate individual check names.
- Nextest normally discovers `.config/nextest.toml`. This task's ownership contract instead requires root `nextest.toml`; therefore every repository-owned nextest entry point passes `--config-file nextest.toml` explicitly.
- The nextest profile keys used here were accepted by the installed `cargo-nextest 0.9.140`, and the default profile successfully ran all five current `relay-domain` tests.
- pnpm recursive script execution excludes the workspace root by default and applies to workspace projects. That is appropriate here: both current child packages define `build` and `typecheck`, while the root merely delegates recursively.
- The official pnpm page served during research documents the current 11/12 line, while the repository pins pnpm 10.30.3. The `-r run <script>` form is deliberately conservative, but local pnpm validation remains required because pnpm is unavailable.
- The Cargo alias file was loaded successfully: `cargo --list` displayed all three `relay-*` aliases.
- Generic TOML parsing proves syntax only. It does not replace cargo-deny or typos semantic validation.

## Potential corrections to the master plan

1. **Nextest config location:** the master plan names root `nextest.toml`, but upstream's discovery location is `.config/nextest.toml`. **Impact:** invoking `cargo nextest` without the repository recipe or alias will not load the root file. **Disposition:** retain the mandated file for Phase 0 and pass `--config-file nextest.toml`; consider correcting the master plan and relocating it in a separately owned planning/tooling change.
2. **Pinned pnpm documentation/validation:** the repository pins pnpm 10.30.3, while the checked unversioned official page currently presents 11/12. **Impact:** no observed syntax conflict, but execution is unverified locally. **Disposition:** do not change the package-manager pin; validate with pnpm 10.30.3 when it is available.
3. **License allow-list maturity:** the initial cargo-deny allow-list is intentionally narrow and currently has no third-party Rust dependencies to exercise it. **Impact:** future dependencies may require an evidence-backed license policy update. **Disposition:** do not pre-allow additional licenses; revise only when `cargo deny check` reports a concrete dependency need.

## Decisions applied

- Provide small, explicit `just` recipes rather than a bootstrap/install recipe; tooling remains user-managed.
- Make `check` aggregate `relay-domain` checking and recursive web typechecking.
- Apply `--all-targets --all-features` to Rust compile, lint, and nextest entry points; format only the named package.
- Add equivalent `relay-check`, `relay-test`, and `relay-lint` Cargo aliases.
- Use a strict dependency baseline: deny wildcard dependencies and unknown registries/git sources, warn on duplicate versions, and allow a limited set of common permissive licenses.
- Keep local nextest fail-fast with no retries; provide a non-fail-fast CI profile with two retries and JUnit output without wiring CI.
- Standardize UTF-8/LF/final newlines, two-space general indentation, and four-space Rust indentation.
- Exclude generated/vendor-heavy paths and the pnpm lockfile from typo scanning.

## Validation evidence

```text
$ python - <<'PY'
from pathlib import Path
import tomllib
for path in ('deny.toml', 'nextest.toml', 'typos.toml', '.cargo/config.toml'):
    tomllib.loads(Path(path).read_text(encoding='utf-8'))
    print(f'{path}: valid TOML')
PY
deny.toml: valid TOML
nextest.toml: valid TOML
typos.toml: valid TOML
.cargo/config.toml: valid TOML
```

```text
$ cargo --list
exit 0; listed relay-check, relay-test, and relay-lint with the configured expansions
```

```text
$ cargo check -p relay-domain --all-targets --all-features
exit 0; relay-domain checked successfully
```

```text
$ cargo nextest run --config-file nextest.toml -p relay-domain --all-targets --all-features
exit 0; 5 tests passed, 0 skipped
```

```text
$ cargo fmt -p relay-domain -- --check
initial exit 1; reported a formatting diff in crates/relay-domain/tests/domain.rs
No source file was changed by this task because Rust code is outside its ownership.

$ cargo fmt -p relay-domain -- --check
final exit 0 after the separately owned relay-domain work was formatted
```

```text
$ cargo clippy -p relay-domain --all-targets --all-features -- -D warnings
exit 1; cargo-clippy is not installed for toolchain 1.92.0
No component was installed.
```

```text
$ for tool in just cargo-deny typos pnpm editorconfig-checker; do
    if command -v "$tool" >/dev/null 2>&1; then "$tool" --version; else echo "$tool: unavailable"; fi
  done
just: unavailable
cargo-deny: unavailable
typos: unavailable
pnpm: unavailable
editorconfig-checker: unavailable
```

Consequently, the justfile, cargo-deny policy, typos policy, pnpm recipes, and
EditorConfig file could not receive native semantic/execution validation in this
environment. The missing tools were not installed.

## Deferred follow-ups

- Run `just --list`, `cargo deny check`, `typos`, `pnpm -r run typecheck`,
  `pnpm -r run build`, and an EditorConfig checker when their existing pinned/native tools are available.
- Re-run clippy after the pinned Rust toolchain already includes the clippy component.
- Decide whether to move `nextest.toml` to upstream's default `.config/nextest.toml` location through an explicitly owned plan correction.
