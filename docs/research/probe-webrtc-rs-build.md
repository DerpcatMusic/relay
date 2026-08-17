# webrtc-rs v0.20.2 reproducible build / negative-gate probe

**Status:** COMPLETE — reproducible Linux build passes; mandatory TURN TCP/TLS gate fails; **STOP before live matrix**  
**Selection:** None. This probe does not select or integrate a provider.  
**Evidence root:** `/tmp/relay-provider-probes/webrtc-rs`  
**Run:** 2026-08-16T13:24:51Z–2026-08-16T13:30:45Z

## Result

The standalone crate pinned `webrtc = "=0.20.2"` and resolved the complete graph under a new archived lockfile. On the current Linux x86-64 host, a final clean sequence of locked `cargo check`, `cargo build`, and `cargo test` passed. The minimal API test constructs and closes a peer connection through the public v0.20.2 API.

The candidate nevertheless fails the mandatory relay-security gate. Two independent checks agree:

1. The exact pinned source checks `url.is_secure()` and non-UDP protocol before host resolution and skips both paths; its constructed TURN client is hard-coded to `TransportProtocol::UDP`.
2. A compiled deterministic test configures `turn:127.0.0.1:9?transport=tcp` and `turns:127.0.0.1:9?transport=tcp`, starts gathering, and observes the exact warnings `Skipping unsupported non-UDP TURN` and `Skipping unsupported secure TURN`, zero relay candidates, and immediate completion without live network access.

Therefore v0.20.2 has no TURN-over-TCP route and no TURN-over-TLS route. Per `transport-candidate-comparison.md`, the expensive Coturn/browser/impairment matrix was not attempted. Proceeding requires a separately pinned upgrade or patch identity that restores both mandatory routes.

## Immutable identity and integrity

| Item | Immutable identity | Bytes | SHA-256 |
|---|---|---:|---|
| crates.io package | `webrtc 0.20.2` | 275,485 | `116da0f0e617d01d91872ece8fdef0da42dfb39747b2fe48760ae544b52f2344` |
| upstream tag/revision | `webrtc-rs/webrtc v0.20.2` → `38e02d88a10a2afa9dd637acf93374a2bc8f3413` | 259,834 (GitHub source archive) | `afdbd346640255127e4af228df5eda56325f27ab8a00a6b378a7616b29f807f7` |
| crates.io core package | `rtc 0.20.2` | 7,517,094 | `f294ac22b05a087786d41f2fc90d8e2ac29523ed80f341137471a639ae4a653b` |
| core tag/revision | `webrtc-rs/rtc v0.20.2` → `efad79da22ba98c71dc5e78b6ece177120353741` | 8,232,940 (GitHub source archive) | `9f50b981866589d46459e0ab3b74a5e153a17f2299eb21145955ecc0e5ae93ff` |
| generated lockfile | standalone probe `Cargo.lock` | 60,508 | `7905635442f3f852d75491d0bab76fc0080be6d058b4779b9f8bee5088447c08` |

`git ls-remote` independently resolved both tags to those full commits (`logs/tag-resolution.log`). Cargo's lock checksums equal the archived `.crate` hashes. The audited upstream `turn_relayer.rs` and Cargo's registry copy are byte-identical: 31,306 bytes, SHA-256 `865423ca0563ea12807dc67b0b3ff7ef496ad5f3dcec4c0766c375fe77e3b30b` (`logs/source-registry-comparison.log`).

## Standalone build identity

The generated crate and all build output remain under `/tmp/relay-provider-probes/webrtc-rs`:

- `Cargo.toml`: exact `webrtc = "=0.20.2"`; SHA-256 `e000b35ead11287c0d6e8513fa43910aa264390e86d825c8f01868609ca36749`.
- Resolved candidate features: `webrtc/default`, exactly one runtime `webrtc/runtime-tokio`; core `rtc/default` + `rtc/ring`.
- All `webrtc`, `rtc`, and 15 `rtc-*` family packages resolve once at `0.20.2`.
- Full normal/build tree: `logs/cargo-tree.log`; features: `logs/cargo-tree-features.log`; duplicates: `logs/cargo-tree-duplicates.log`; Cargo metadata: `artifacts/cargo-metadata.json`.
- Current-target native closure: `ring 0.17.14` builds C/assembly and is statically linked. The test executables dynamically require only `libgcc_s`, `libm`, `libc`, and the Linux loader (`artifacts/built-executables-ldd.txt`). The metadata-only cross-target `wasm-bindgen-shared` link key was not built for this target.

### Host/toolchain

- Host: CachyOS rolling Linux, kernel `7.2.0-rc7-1-cachyos-rc`, `x86_64`.
- Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6; target `x86_64-unknown-linux-gnu`.
- Cargo: `1.97.1 (c980f4866 2026-06-30)`.
- Native tools: GCC/G++ 16.1.1, GNU ld 2.47, glibc 2.44; CMake 4.4.2 (present but not invoked by this graph).
- Build environment had no captured `CC`, `CXX`, `CFLAGS`, `CXXFLAGS`, `LDFLAGS`, `RUST*`, `CARGO*`, `PKG_CONFIG*`, or `OPENSSL*` overrides.
- Important reproducibility gap: this was a direct rolling-host build, not an immutable build image. The copied environment manifest records that field explicitly as missing rather than substituting the `/etc/os-release` hash.

Complete capture: `logs/environment.log`, `logs/linker-tool-identities.log`, and `environment-manifest-v1.json`.

## Clean locked gates

Final sequence, after `cargo clean` removed 1.3 GiB of prior target output:

| Gate | Result | Evidence log | Log SHA-256 |
|---|---|---|---|
| `cargo check --locked --all-targets` | PASS, exit 0 | `logs/cargo-check-locked.log` | `8f8f134427b6170450dd8227e86deacd46e8fd2c681fa3130ec9819948ba2e81` |
| `cargo build --locked --all-targets` | PASS, exit 0 | `logs/cargo-build-locked.log` | `20618a9e3374ce707b39cea44e1aad774aa5ec62c0a5b21287f95e34de917b76` |
| `cargo test --locked --all-targets -- --nocapture --test-threads=1` | PASS, 2/2 tests | `logs/cargo-test-locked.log` | `d2ad54c6eac6ec6c077314f959d4874ffbeb9f56f19c575397665065612bfce7` |

The two tests are:

- `minimal_api_builds_and_closes`: public API construction/close smoke test.
- `pinned_relayer_skips_tcp_and_tls_before_network_access`: deterministic mandatory-route negative.

An initial authoring attempt used obsolete root-level imports and failed, then a second locked attempt correctly refused an intentionally changed manifest until the lock was regenerated. Both diagnostic logs are preserved as `cargo-check-locked-attempt1-api-import-error.log` and `cargo-check-locked-attempt2-lock-update-needed.log`; neither is represented as a candidate failure. The final crate, lock, and clean sequence above are the reproducible result.

### Built debug test artifacts

These are probe executables, not shippable Relay packages:

| Executable | Bytes | SHA-256 |
|---|---:|---|
| `relay_webrtc_rs_negative_gate_probe-a5d158cea4d54879` | 97,238,536 | `55128518ba001e76ada50509fad8f56c552f709baac44589b6499ed5511264d2` |
| `turn_negative-da55a81a019c878a` | 98,314,040 | `e76fe5685f32c910ee5259ae5c4ff40de3ff882facb51fe66eaf51359a844375` |

Manifest: `artifacts/built-executables.json`.

## TCP/TLS negative gate

Pinned source at `src/peer_connection/transports/turn_relayer.rs` establishes:

- lines 255–258: every secure/`turns:` URL logs and skips;
- lines 260–263: every non-UDP protocol, including `transport=tcp`, logs and skips;
- both checks precede `resolve_host` at line 266, so the negative needs no TURN server;
- line 298: any constructed TURN client uses `TransportProtocol::UDP`.

`source-probe/assert_turn_filters.py` asserts those tokens and their order against the exact commit archive (`logs/source-turn-negative.log`, PASS). The compiled negative test then exercises the public configuration/gathering path (`logs/turn-negative-test.log` and the final test log, PASS). It emitted:

```text
Skipping unsupported non-UDP TURN url turn:127.0.0.1:9?transport=tcp
Skipping unsupported secure TURN url turns:127.0.0.1:9?transport=tcp
```

This is a hard-gate **failure**, not a successful capability test. There is no TLS handshake, certificate-chain/SAN/SNI behavior, allocation, selected relay pair, or live TCP/TLS transport to score.

## Licenses

- `webrtc 0.20.2`, `rtc 0.20.2`, and every pinned `rtc-*` component declare `MIT/Apache-2.0`; exact upstream `LICENSE-MIT` and `LICENSE-APACHE` files are archived and hashed.
- The locked metadata has 246 third-party packages and no package missing both a declared license expression and license-file field.
- `ring 0.17.14`, the current-target native-link package, declares `Apache-2.0 AND ISC` and carries additional upstream notice obligations.
- `artifacts/license-feature-inventory.csv` is the complete resolved declared-license/feature inventory; `artifacts/license-feature-summary.json` summarizes it.

This inventory is evidence, not a passed licensing gate: a production SBOM, complete notice bundle, source-offer review where applicable, and all-target packaging review remain missing.

## Copied T0 fields and scorecard

The untouched templates were copied from `tests/fixtures/transport` into the evidence root and completed without modifying the repository fixtures:

- `environment-manifest-v1.json` — status `stopped_hard_gate_failed`; exact candidate/core revisions, Cargo checksums, features, native closure, host tools, current target, retry policy, UTC bounds, and explicit missing/not-run fields.
- `scorecard-v1.json` — `relay_security = fail`; all other hard gates remain `not_run`; every rating and total remains `null`; weighted eligibility is `false`; result remains `not_evaluated`; no winner is named.

Known incomplete fields are explicit:

- no immutable build-image digest;
- Windows x86-64, macOS arm64, and macOS x86-64 not built;
- Chromium, Firefox, browser drivers, Safari, and Coturn absent/not run;
- no isolated live TURN configuration, TLS chain, relay port range, impairment profile, or random seed;
- no live state-transition archive, selected pair, DTLS/SCTP state, messages/bytes, reconnect time, RSS/CPU series, or sanitizer run;
- binary sizes exist only for Linux debug probe tests, not four-target release packages.

The only live-observation-like evidence is the deterministic gathering completion, zero relay-candidate assertion, exact route-rejection warnings, and normal test exit. Missing observations were not synthesized.

## Reproduction

```bash
cd /tmp/relay-provider-probes/webrtc-rs
cargo generate-lockfile
cargo tree --locked --edges normal,build
cargo tree --locked --all-features --edges features
cargo tree --locked --duplicates
python source-probe/assert_turn_filters.py
cargo clean
cargo check --locked --all-targets
cargo build --locked --all-targets
cargo test --locked --all-targets -- --nocapture --test-threads=1
```

The first command is for recreating the archived lock from registry state. Verification of this probe should instead retain the archived `Cargo.lock` and start at the locked commands. The full evidence set, including commands, source archives, package archives, licenses, logs, hashes, sizes, metadata, environment manifest, and scorecard, is indexed by `MANIFEST.sha256` and `MANIFEST.sizes` in the evidence root.

## Repository hygiene

No Relay source, schema, fixture, workspace manifest, or workspace lockfile was edited. The only repository change is this requested research report. Generated crate/build/evidence files are confined to `/tmp/relay-provider-probes/webrtc-rs`. No commit was created.
