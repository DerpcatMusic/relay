# CI libopus matrix evidence

## Scope and disposition

This focused correction covers only `.github/workflows/ci-rust.yml`. The Rust workspace directly links the system `opus` library, so every matrix leg must make a 1.6.x development library visible to both the native linker and the test-process loader before any Cargo-backed step runs.

**Final disposition:** approve the workflow correction for hosted-runner validation. The three operating-system legs and all existing formatting, workspace-check, nextest, and Clippy gates remain enabled. Local structural checks can validate the workflow text, but only GitHub-hosted execution can prove the current images, package services, native toolchains, dynamic loaders, and codec tests work together.

## Official evidence (four sources)

1. [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) documents the `ubuntu-latest`, `windows-latest`, and `macos-latest` labels and warns that `-latest` images can change/migrate. It also directs readers to the runner-image inventories for installed software. Therefore image-preinstalled libopus must not be assumed, and a smoke check must detect image/package drift.
2. [Official Opus downloads](https://opus-codec.org/downloads/) identifies **libopus 1.6.1** as the stable source release and publishes SHA-256 `6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1`. The Ubuntu leg can therefore install an exact, integrity-checked 1.6.x release even when the distribution package is older.
3. [Homebrew's official `opus` formula](https://formulae.brew.sh/formula/opus) records stable **1.6.1**, the `brew install opus` command, and bottle availability for current macOS runner architectures. The workflow records Homebrew's resolved package version, exports the formula prefix's include/library metadata, and rejects a loaded version outside 1.6.x.
4. [Microsoft vcpkg's pinned Opus port](https://github.com/microsoft/vcpkg/blob/eb2d21971e9d95cc0688eaf7e221cd9e5c8ee6be/ports/opus/vcpkg.json) declares version **1.6.1**. Pinning vcpkg commit `eb2d21971e9d95cc0688eaf7e221cd9e5c8ee6be` makes the Windows package selection reproducible instead of inheriting whatever vcpkg checkout happens to be on the image.

Sources were checked on 2026-08-16. No secondary sources were used.

## Severity-ranked findings and corrections

### S1 — All original matrix legs reached Cargo without provisioning libopus

The original workflow installed Rust and immediately ran `cargo fmt`, `cargo check`, nextest, and Clippy. Ubuntu, macOS, and Windows therefore depended on undocumented image state; a clean image could fail native linking, while a machine with an older library could silently test the wrong codec.

**Potential correction:** provision and smoke-check libopus before the Rust installer and every explicit Cargo command.

**Applied correction:** OS-conditional install and native smoke-check steps now precede `taiki-e/install-action`. The original three-entry matrix, `--all-features`, `--all-targets`, workspace scope, and nextest codec-test path are unchanged.

### S1 — Ubuntu package availability does not itself establish the required 1.6.x line

A bare `apt-get install libopus-dev` would make CI depend on the moving Ubuntu archive and does not pin the repository's required 1.6.x implementation.

**Potential correction:** obtain the official stable release, verify its published digest, build its shared library, install it into a standard native prefix, and refresh the loader cache.

**Applied correction:** Ubuntu installs only build helpers with apt, then builds official 1.6.1 from the digest-checked tarball into `/usr/local`. `LIBRARY_PATH`, `LD_LIBRARY_PATH`, and `PKG_CONFIG_PATH` explicitly select that prefix. A small C program compiles, links, runs, calls `opus_get_version_string()`, and enforces `libopus 1.6.x`.

### S1 — Windows needs both an import-library path and a DLL search path

Installing a package without publishing its `lib` and `bin` directories would allow compilation or runtime to fail independently.

**Potential correction:** use a reproducible x64 dynamic vcpkg port, publish its import-library directory through `LIB`, publish its DLL directory through `GITHUB_PATH`, then perform an actual native link-and-run probe.

**Applied correction:** Windows bootstraps the pinned vcpkg tree, installs `opus:x64-windows`, records `vcpkg list`, exports `LIB` and the DLL path, and uses CMake/MSVC to link and execute a C version probe. The probe fails unless the loaded DLL reports 1.6.x.

### S2 — Homebrew's prefix is architecture-dependent and `macos-latest` can migrate

Hard-coding `/usr/local` or `/opt/homebrew` would break when the hosted label changes architecture.

**Potential correction:** resolve the formula prefix at runtime, publish its link/load/pkg-config paths, record the installed formula version, and verify the loaded library rather than trusting package metadata alone.

**Applied correction:** macOS uses `brew --prefix opus`, exports `LIBRARY_PATH`, `DYLD_FALLBACK_LIBRARY_PATH`, and `PKG_CONFIG_PATH`, prints `brew list --versions opus`, then compiles and executes the same native version probe with explicit include/library paths.

## Resulting matrix contract

| Runner | Acquisition/version record | Link visibility | Load visibility | Smoke gate |
| --- | --- | --- | --- | --- |
| Ubuntu | Official 1.6.1 tarball plus published SHA-256; apt only for build helpers | `/usr/local/lib` via standard prefix and `LIBRARY_PATH` | `ldconfig` plus `LD_LIBRARY_PATH` | C link/run; `opus_get_version_string()` must match 1.6.x |
| macOS | `brew install opus`; `brew list --versions opus` | resolved formula `lib` via `LIBRARY_PATH` and explicit probe `-L` | resolved formula `lib` via `DYLD_FALLBACK_LIBRARY_PATH` | C link/run; loaded version must match 1.6.x |
| Windows | vcpkg commit pinned to Opus 1.6.1; `vcpkg list` | x64 import library via `LIB` | x64 DLL directory via `GITHUB_PATH` | CMake/MSVC link/run; loaded version must match 1.6.x |

## Validation record

Run from the repository root on 2026-08-16:

```text
$ ruby -e 'require "yaml"; YAML.load_file(".github/workflows/ci-rust.yml"); puts "ok"'
ok
$ go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/ci-rust.yml
# no diagnostics; exit 0
```

A final static audit confirmed that the matrix still contains exactly `ubuntu-latest`, `windows-latest`, and `macos-latest`; each OS install and smoke step occurs before the Rust/cargo-nextest installer; and the four original Cargo quality/test commands are unchanged. This scoped task wrote only `.github/workflows/ci-rust.yml` and `docs/research/ci-libopus-matrix.md`; it did not edit manifests or source.

Hosted execution is **still required**. Local Linux syntax/action checks cannot substitute for the current GitHub Ubuntu, macOS, and Windows images or prove their package-manager/network/native-toolchain behavior and end-to-end codec test loading.
