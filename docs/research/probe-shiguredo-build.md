# Shiguredo webrtc-rs reproducible build/admission probe

**Status: COMPLETE — HARD BLOCKERS; not admitted; no selection**  
**Probe time:** 2026-08-16 UTC  
**Isolation root:** `/tmp/relay-provider-probes/shiguredo`

This is a build/artifact admission probe, not a provider selection and not a runtime interoperability claim. The only repository file created or changed by this probe is this report. The throwaway crate, lockfile, downloaded assets, build outputs, and raw logs are under the isolation root.

## Pinned identity

| Layer | Pin | Verification |
|---|---|---|
| Rust wrapper | crate `shiguredo_webrtc = "=0.151.0"`; annotated tag object `9720e0829cd968a705f016fa046d25cf3e32cbeb` | GitHub tag API peels to commit `7052a80710d85eee8f3b081ad585e38d878c614e`; the crates.io package's `build.rs` and `src/api/data_channel.rs` are byte-identical to raw files at that commit. |
| Builder | `m151.7922.0.0` | Lightweight tag points to builder commit `0d51a21bbebc6eade24ae5d28d179be9a4b01732`. The wrapper package metadata names exactly this builder release. |
| libwebrtc | `M151.7922@{#0}` | `VERSIONS` from official `webrtc.ubuntu-24.04_x86_64.tar.gz` records `WEBRTC_COMMIT=f20ebb8adbf4fa781830e4384c61f732bd28a217`. |

The crates.io package is 210,731 bytes, SHA-256 `ec90f3d9c650ed7c2a51b4b86bb0f42a8dd02873ad7456eb77288257086b97c6`, matching its Cargo registry checksum. Its manifest declares Rust 1.93, Apache-2.0, and builder `m151.7922.0.0`.

## Official Linux x86_64 URL/hash verification

The expected hashes below were taken from the checked-in dossier before admitting the live metadata. For each asset, the official release URL exists, the live 105-byte `.sha256` content equals the dossier value, and the GitHub release API's payload `digest` equals the same value.

| Official `0.151.0` asset URL | Bytes | Dossier / live sidecar / API SHA-256 | Result |
|---|---:|---|---|
| https://github.com/shiguredo/webrtc-rs/releases/download/0.151.0/libwebrtc_c-ubuntu-22.04_x86_64.tar.gz | 20,357,124 | `564ef311e820083d67a6b8c413f8c5296eb81ecee16305a8ac913a80fa03bcb5` | MATCH |
| https://github.com/shiguredo/webrtc-rs/releases/download/0.151.0/libwebrtc_c-ubuntu-24.04_x86_64.tar.gz | 20,367,302 | `729f7a21224bd7e0c333177191e55eca705cbb86653a3294f78a3608d9976823` | MATCH |
| https://github.com/shiguredo/webrtc-rs/releases/download/0.151.0/libwebrtc_c-ubuntu-26.04_x86_64.tar.gz | 20,351,690 | `6e81cf59b161c84ec368073114005e624650c0f4bc372e720e852963e0fb11c2` | MATCH |

The Ubuntu 24.04 x86_64 payload was independently downloaded to the probe cache and computed as `729f7a…6823`; this exact cached byte stream, not a build-time network response, was served to the clean offline checks below. Raw verification: `linux-x86_64-metadata-verification.json`, `asset-download.log`, and `release-0.151.0.json`.

## Target availability metadata

“Available” below means only that the pinned build script maps the target and the official release metadata contains its payload. No foreign target was built, and this table makes no ABI/runtime claim.

| Required lane | Pinned build-script mapping | Official payload | Disposition |
|---|---|---|---|
| Windows x86_64 (`x86_64-pc-windows-msvc`) | `windows_x86_64` | `libwebrtc_c-windows_x86_64.tar.gz`, 96,252,644 bytes, SHA-256 `35c1051cddce3e89c59a760a0ea824d69f86cd503dadad3a6e5ab3c6baafec65` | AVAILABLE METADATA ONLY; not built |
| macOS arm64 (`aarch64-apple-darwin`) | `macos_arm64` | `libwebrtc_c-macos_arm64.tar.gz`, 177,091,393 bytes, SHA-256 `4c8369a50e4e2491e4cb8099403529fcb4daa34b563ee75da280e3bfd3be26f7` | AVAILABLE METADATA ONLY; not built |
| macOS x86_64 (`x86_64-apple-darwin`) | **absent**; the wildcard path panics as unsupported | **no `libwebrtc_c-macos_x86_64.tar.gz` asset** | **HARD BLOCKER** |
| Linux x86_64 (`x86_64-unknown-linux-gnu`) | Ubuntu 22.04, 24.04, or 26.04 selected by distro; `WEBRTC_C_TARGET` override exists | all three URL/hash-verified assets above | AVAILABLE only on the named Ubuntu lanes; see host caveat below |

The macOS x86_64 result was an explicit absence test against all 24 release assets plus the pinned `get_target_platform` match arms. It was not replaced by an arm64 payload, XCFramework slice, forced name, or cross-build. Raw result: `target-availability.json`.

## Archive target, toolchain, and probe environment

### Payload/build provenance

- `libwebrtc_c.a` is a current `ar` archive; sampled member `crypto_random.cc.o` is ELF64 little-endian, System V, AMD x86-64, relocatable.
- That object's `.comment` records `clang version 23.0.0git` at LLVM project commit `53d18800eda3b7407e53366f27ca78e922c6e0db`.
- The target-matching official builder input is `webrtc.ubuntu-24.04_x86_64.tar.gz`, 112,353,158 bytes. Independently computed SHA-256: `a9e5bc873c90aab6c6b96ea0b62bc02b16ef55d78dc80fc1633b03a07f5f221f`; the live GitHub API `digest` field agrees. There is still no Shiguredo checksum sidecar or signature for this builder payload, so an internal immutable digest remains necessary.
- Builder `VERSIONS` pins libwebrtc and also records libc++ `5abc7f839700f0f17338434e1c1c6a8c87c00c11`, libc++abi `8f11bb1d4438d0239d0dfc1bd9456a9f31629dda`, and libunwind `9fe0a380ee56ac20c938cb0c0c35c9b4f7c73339`.

### Actual probe host

- CachyOS rolling, x86_64, Linux `7.2.0-rc7-1-cachyos-rc`; **not an official Ubuntu lane** and no immutable build-image digest.
- rustc `1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6; Cargo `1.97.1 (c980f4866 2026-06-30)`.
- GCC/G++ 16.1.1, GNU ld 2.47, CMake 4.4.2. The prebuilt lane did not invoke CMake or compile C/C++.
- Default feature set; `source-build` disabled. `WEBRTC_C_TARGET=ubuntu-24.04_x86_64` was required because automatic detection correctly rejects CachyOS.

A clean `cargo check --offline --locked` with no override failed before any asset request with “unsupported Linux distribution; specify `WEBRTC_C_TARGET`.” Consequently, the forced-target success below is an artifact/link mechanics and ABI experiment, **not** an admission pass for the official Ubuntu 24.04 lane.

## Clean offline locked results

The minimal isolated crate uses only `shiguredo_webrtc = "=0.151.0"`. Its 7,718-byte `Cargo.lock` has SHA-256 `ffa1f1b7ea57448d92101354b5eba35f5e5e4642d3089d5efc8cfbfb2890304b` and locks the wrapper to registry checksum `ec90f3d…97c6`.

One initial `cargo fetch` populated the Rust package cache and was recorded as online. Each admission command then followed `cargo clean` and used both `--offline --locked` and `CARGO_NET_OFFLINE=true`. Cargo offline mode does **not** sandbox a build script: the wrapper still spawned `curl` twice for its hard-coded GitHub archive and sidecar URLs. To make those executions reproducible and fail-closed, `PATH` selected a local curl shim that:

1. accepted only the exact pinned Ubuntu 24.04 archive and sidecar URLs,
2. copied independently cached/hash-checked bytes,
3. rejected every other URL with exit 86, and
4. logged every invocation.

This demonstrates a packaging limitation: Cargo's offline flag alone cannot make the stock build offline; an internal mirror/interceptor or upstream local-prebuilt override is required.

| Clean command | Result | Build-script requests |
|---|---|---|
| `cargo check --offline --locked -vv` | PASS, 3.96 s | exact archive + sidecar, both satisfied locally |
| `cargo build --offline --locked -vv` | PASS, 4.49 s | exact archive + sidecar, both satisfied locally |
| linked native-symbol smoke (`random_string(16)`) | PASS, process exit 0 | proves the final link pulled native code rather than merely compiling Rust types |

Raw logs: `clean-offline-locked-check-pass.log`, `offline-curl-check-pass.log`, `clean-offline-locked-build-native.log`, and `offline-curl-build-native.log`.

## Link closure and artifact hashes

The pinned build script declared static `webrtc_c` plus Linux `m`, `dl`, `rt`, `X11`, and `pthread`. With linker as-needed behavior, the final native-symbol smoke executable's observed `DT_NEEDED`/`ldd` closure was `libm.so.6`, `libgcc_s.so.1`, `libc.so.6`, and the ELF loader; `dl`, `rt`, X11, and pthread were not retained by this deliberately narrow smoke. The executable's observed maximum GLIBC version reference is `GLIBC_2.34`; because the executable was linked on the unsupported current host, that is not a supported-distro floor claim.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| cached `libwebrtc_c-ubuntu-24.04_x86_64.tar.gz` | 20,367,302 | `729f7a21224bd7e0c333177191e55eca705cbb86653a3294f78a3608d9976823` |
| extracted/copied `libwebrtc_c.a` | 84,435,572 | `21e01411c0c6c57c1f68f27a6a786b3d90b6666d6c516dc54d31e517740b1342` |
| extracted `bindings.rs` | 220,239 | `608b1b0e7fb9a367ac7a2741e8ce6c5a154e6799d2a93e3eb1899eafa5949b40` |
| linked native-symbol smoke executable | 6,116,856 | `45f35fb2ef9fc55e725114ce44e002d92df2c5b020273dec4003c089a247cfd8` |
| official builder Ubuntu 24.04 x86_64 archive | 112,353,158 | `a9e5bc873c90aab6c6b96ea0b62bc02b16ef55d78dc80fc1633b03a07f5f221f` |

`asset-extracted-manifest.json` contains the path/size/SHA-256 for every extracted wrapper payload member. `linked-native-artifact-inspection.json` contains `file`, `ldd`, ELF dynamic/version data, and the smoke exit result.

## Public buffered amount API probe

**HARD BLOCKER confirmed.** Source search over the complete published `shiguredo_webrtc-0.151.0` crate found zero case-insensitive `buffered_amount` occurrences. Public `DataChannel` methods are label, state, send, close, observer registration/unregistration, and the raw pointer accessor. `DataChannelObserverHandler` exposes only state change and message callbacks.

A compiled negative API probe against the exact locked crate produced the expected compiler failures:

- E0599: no `DataChannel::buffered_amount`,
- E0599: no `DataChannel::set_buffered_amount_low_threshold`,
- E0407: neither `on_buffered_amount_change` nor `on_buffered_amount_low` is a member of `DataChannelObserverHandler`.

The native archive contains internal/upstream buffered-amount symbols, but unwrapped native implementation is not a public Rust capability. Evidence: `compiled-api-negative.log`, `data_channel.rs.pinned`, and `native-buffered-amount-symbols.txt`.

## Licenses, NOTICE, and provenance

The wrapper payload includes:

| File | Bytes | SHA-256 | Content role |
|---|---:|---|---|
| `LICENSE` | 10,168 | `bb4e4d49252e9c632d86d15490709a92c01371ae638861250fa07a1bdf487a93` | wrapper Apache-2.0 terms |
| `THIRD_PARTY_LICENSES.md` | 1,621 | `94239ea4b099fab935b695a67eb24043790d4cf17209354063156a4eae7ce761` | top-level WebRTC BSD terms only |

The wrapper payload has **no** `NOTICE`, `VERSIONS`, or `DEPS`. From the exact target-matching builder archive, the probe recovered:

| File | Bytes | SHA-256 |
|---|---:|---|
| `NOTICE` | 135,316 | `6a8c51914527d5ad0bb2ee120dd28f5672cf6a041507c93ec22f2ed0448251f6` |
| `VERSIONS` | 1,484 | `8063347b41d7f7ce806cd88cd123e9c5e2d12c0c83fea94a1fed08c365f2c2bb` |
| `DEPS` | 54 | `49938b48250407dbcb404b3d549c9a7a9b9d9ecfbcb76f1ce3352a0a611fc9d4` |

The comprehensive upstream `NOTICE` is 2,690 lines and must be reviewed and shipped with the exact statically linked target as applicable; the wrapper's small third-party file is not a substitute. No wrapper signature, certificate, SBOM, or SLSA provenance was established. **T0 licensing remains failed** until an audited target-matching notice/SBOM bundle is incorporated into real packaging.

## T0 manifest and scorecard

Filled copies are preserved as:

- `/tmp/relay-provider-probes/shiguredo/environment-manifest-v1.shiguredo.json`
- `/tmp/relay-provider-probes/shiguredo/scorecard-v1.shiguredo.json`

All unperformed fields are literal `MISSING`/`not_run`, rather than inferred passes. Summary:

| Hard gate | Status | Reason |
|---|---|---|
| `adapter_fit` | **FAIL** | public buffered amount, low-threshold setter, and low/amount-change observer callback absent |
| `browser_interop` | `not_run` | no browser/runtime work in this probe |
| `relay_security` | `not_run` | no TURN/TLS/runtime work in this probe |
| `recovery_lifecycle` | `not_run` | no lifecycle/runtime work in this probe |
| `licensing` | **FAIL** | wrapper asset omits comprehensive target NOTICE/DEPS/VERSIONS; no audited shipped SBOM/notice set |
| `packaging` | **FAIL** | required macOS x86_64 asset and build mapping absent; Linux success was on a forced unsupported host |
| `maintenance` | `not_run` | outside build probe scope |

Every weighted rating and the total remain `null`; failed hard gates prohibit weighted comparison. `eligibleForWeightedComparison=false`. **Disposition: rejected at build/admission hard gates, no provider selection.**

## Evidence inventory and missing work

Raw evidence is under `/tmp/relay-provider-probes/shiguredo`; key machine-readable files are the two filled T0 JSON documents, the release metadata snapshots, target availability results, Linux digest verification, extracted-member manifest, Cargo metadata, exact-pinned crate/lockfiles, curl/build logs, linked artifact inspection, and compiled API-negative log.

Still explicitly missing:

- matching real Windows x86_64, macOS arm64, and Ubuntu 22.04/24.04/26.04 build/link/run evidence;
- any macOS x86_64 payload/mapping (hard blocker, not merely untested);
- immutable build-image digests and OS/SDK symbol-floor inspection for supported lanes;
- browser/TURN/interoperability, recovery, security, stress, sanitizer, RSS/CPU, and full application-size results;
- final application license inventory, audited third-party notice bundle, and SBOM;
- signature/SLSA provenance and a stock upstream-supported build-time local mirror/prebuilt override.
