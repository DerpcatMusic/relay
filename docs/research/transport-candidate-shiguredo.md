# Shiguredo `webrtc-rs` — RELAY Phase-2 artifact and capability dossier

**Retrieval / evidence cutoff:** 2026-08-16 UTC  
**Research status:** Complete for official artifact/provenance and pinned-wrapper capability evaluation.  
**Decision status:** Evidence dossier complete; no candidate was built, run, selected, or rejected. Capability acceptance remains blocked by the explicit adapter and probe blockers below.  
**Scope:** Canonical Shiguredo `webrtc-rs` and the Shiguredo `webrtc-build` binary inputs that its release packages into `libwebrtc_c`.

## Evidence rules

- **Documented** means stated by Shiguredo or directly visible in a pinned manifest, workflow, source file, tag, or official release asset.
- **Observed artifact** means a fact read from the official release record, checksum sidecar, or archive at the cutoff.
- **Inference** is called out explicitly. **Unknown** means the examined official evidence does not make the guarantee.
- The ten official source groups are listed before the final probe checklist. No blogs, issue discussions, package indexes, or generated third-party summaries were used.

## Exact identity at the cutoff

| Layer | Exact identity | Meaning |
|---|---|---|
| Rust/native wrapper release | stable GitHub release/tag `0.151.0`, published 2026-08-10 03:13:16 UTC [S1] | Latest non-prerelease `webrtc-rs` release visible by the cutoff. |
| Tag identity | annotated tag object `9720e0829cd968a705f016fa046d25cf3e32cbeb`, peeled commit `7052a80710d85eee8f3b081ad585e38d878c614e` | The annotated tag is unsigned. Source references below pin the peeled commit rather than the moving `develop` branch. |
| Rust crate | package `shiguredo_webrtc` `0.151.0`, edition 2024, `rust-version = "1.93"` [S2] | The crate version and native release version are aligned. |
| Candidate native artifacts | `libwebrtc_c-*` assets attached to release `0.151.0` [S1] | These are release-profile combined static libraries plus generated `bindings.rs`, public C headers, and license files; Android also contains `webrtc.jar` [S6]. |
| Bundled Shiguredo WebRTC build | `m151.7922.0.0` [S2, S9] | Every `0.151.0` source build resolves its upstream binary from this exact `webrtc-build` release; the produced default prebuilt assets already incorporate it. |
| Upstream libwebrtc | `M151.7922@{#0}`, commit `f20ebb8adbf4fa781830e4384c61f732bd28a217` [S9] | The `VERSIONS` member in the official `m151.7922.0.0` archives records both the readable revision and commit. |
| Upstream builder source | `webrtc-build` tag `m151.7922.0.0`, commit `0d51a21bbebc6eade24ae5d28d179be9a4b01732`, published 2026-07-30 [S9] | This is the builder/provenance revision, distinct from the libwebrtc commit. |

A newer standalone builder release, `m152.7977.0.0` (builder commit `e91747f203f0998a8546cfa51d74b91ba650ee54`, libwebrtc commit `6f37672d358475cd17544121a12494da454d85fb`), was published on 2026-08-12 [S10]. It is **not** the libwebrtc bundled by `shiguredo_webrtc 0.151.0`; silently substituting it would change the candidate.

The wrapper release body says only “Release 0.151.0”; the manifest and checksums, rather than release prose, carry the useful artifact identity. The tag is source identity. The native payload identity is the target-specific SHA-256 below.

## Official `0.151.0` release asset matrix

The release publishes 12 payloads and one `.sha256` sidecar for every payload. Sizes are GitHub's exact asset byte counts; MiB is bytes / 1,048,576. Hashes are the exact lowercase values in the published sidecars as observed at the cutoff [S1].

| Release asset | Target / payload | Bytes (MiB) | Published SHA-256 |
|---|---|---:|---|
| `libwebrtc_c-android_arm64.tar.gz` | Android `aarch64-linux-android`; static lib + headers/bindings + `webrtc.jar` | 45,857,296 (43.73) | `ee415c625bea7ae6f778cd6a1c3525a7101a329b181e8d936ba0fd8f5eb74c94` |
| `libwebrtc_c-ios_arm64.tar.gz` | iOS `aarch64-apple-ios`; static lib | 136,583,420 (130.26) | `2a74af7faf2c2290a77ae884d53c46bd7b2d20b309152ab4894e46679f70ced8` |
| `libwebrtc_c-macos_arm64.tar.gz` | macOS `aarch64-apple-darwin`; static lib | 177,091,393 (168.89) | `4c8369a50e4e2491e4cb8099403529fcb4daa34b563ee75da280e3bfd3be26f7` |
| `libwebrtc_c-raspberry-pi-os_armv8.tar.gz` | Raspberry Pi OS 64-bit arm64/aarch64 (Debian 13) | 16,320,080 (15.56) | `3b2b896ce0c729703c0b35510d848e3f7e37e8cbef2026fab59724b718dfd819` |
| `libwebrtc_c-ubuntu-22.04_armv8.tar.gz` | Ubuntu 22.04 aarch64 | 16,314,133 (15.56) | `c336523d7e8e2aa00293fb34eadc3ff74e82597ec15f7b53bfabb1a247a2a468` |
| `libwebrtc_c-ubuntu-22.04_x86_64.tar.gz` | Ubuntu 22.04 x86-64 | 20,357,124 (19.41) | `564ef311e820083d67a6b8c413f8c5296eb81ecee16305a8ac913a80fa03bcb5` |
| `libwebrtc_c-ubuntu-24.04_armv8.tar.gz` | Ubuntu 24.04 aarch64 | 16,315,035 (15.56) | `54e2a9e1c3e57752b7c464b6da7b73d9cec4a728eb4a655e78a4c1022fb7b809` |
| `libwebrtc_c-ubuntu-24.04_x86_64.tar.gz` | Ubuntu 24.04 x86-64 | 20,367,302 (19.42) | `729f7a21224bd7e0c333177191e55eca705cbb86653a3294f78a3608d9976823` |
| `libwebrtc_c-ubuntu-26.04_armv8.tar.gz` | Ubuntu 26.04 aarch64 | 16,325,721 (15.57) | `45b1da80ac48cb23c6253fdefe420b3b371dcabe943ef15fbaeebebfbc5db0bc` |
| `libwebrtc_c-ubuntu-26.04_x86_64.tar.gz` | Ubuntu 26.04 x86-64 | 20,351,690 (19.41) | `6e81cf59b161c84ec368073114005e624650c0f4bc372e720e852963e0fb11c2` |
| `libwebrtc_c-windows_x86_64.tar.gz` | Windows `x86_64-pc-windows-msvc`; static `.lib` | 96,252,644 (91.79) | `35c1051cddce3e89c59a760a0ea824d69f86cd503dadad3a6e5ab3c6baafec65` |
| `libwebrtc_c.xcframework.zip` | Apple XCFramework: macOS arm64 + iOS arm64 | 311,519,183 (297.09) | `47ddb069e2ccbc26080d7d2378e88ddd9a1aad5ce66ef629c4ab57f660191dde` |

### Matrix boundary

For desktop/server use, the documented and published matrix is:

- **Windows:** Windows 11 x86-64 and Windows Server 2025 x86-64; target `x86_64-pc-windows-msvc`. No Windows arm64 release asset.
- **macOS:** macOS 15 Sequoia arm64 and macOS 26 Tahoe arm64; target `aarch64-apple-darwin`. No x86-64/universal macOS asset.
- **Linux:** Ubuntu 22.04, 24.04, and 26.04, each in x86-64 and arm64/aarch64; Raspberry Pi OS 64-bit arm64 on Debian 13. These are distro-specific archives, not a generic `linux-gnu` compatibility promise [S5].
- **Mobile, for completeness:** Android arm64 and iOS arm64. No Android x86/x86-64/armv7 release asset and no iOS simulator slice. The XCFramework contains the two published Apple device/desktop slices; it is not a universal simulator bundle [S6].

`armv8` in release filenames maps to Rust `aarch64-unknown-linux-gnu`; `arm64` is the user-facing architecture name. The build script auto-selects only the exact Ubuntu versions above, Raspberry Pi OS, macOS aarch64, Windows x86-64, iOS aarch64, and Android aarch64; an unsupported OS/architecture fails unless `WEBRTC_C_TARGET` is forced [S3]. **Inference:** forcing an archive name does not establish ABI compatibility on an unlisted distro.

## Artifact construction, integrity, and packaging implications

### What is in the archive

The release workflow creates a combined **static** library (`lib/libwebrtc_c.a`, or `lib/webrtc_c.lib` on Windows), `bindings.rs`, non-`.impl.h` C headers, wrapper `LICENSE`, and wrapper `THIRD_PARTY_LICENSES.md`. Android additionally contains `jar/webrtc.jar`. The XCFramework is assembled from the iOS and macOS static libraries and common public headers [S6].

The archives do **not** carry a machine-readable manifest tying the payload back to `0.151.0`, `m151.7922.0.0`, or `f20ebb…`; the release path and SHA-256 are therefore required parts of an internal lock record. No SBOM, signature, certificate, or SLSA provenance asset was published beside `0.151.0` [S1]. **Unknown:** reproducibility/bit-for-bit rebuildability is not claimed.

### Consumer download and trust boundary

The default crate build constructs a URL from `CARGO_PKG_VERSION`, downloads both `libwebrtc_c-<target>.tar.gz` and its sidecar from the same GitHub release, verifies SHA-256, extracts it into Cargo `OUT_DIR`, and statically links it [S3]. Consequences:

1. A clean build is network-dependent; there is no documented prebuilt mirror/path override. Cargo may retain a previously completed build output, but a clean offline build is **not** supported by this path.
2. The check catches transit corruption, but the expected hash is fetched from the same mutable release channel as the archive. The release workflow uploads with `--clobber` [S6]. A distributor should vendor the payload and pin the expected hash independently rather than treating the live sidecar as an immutable lock.
3. Only release-profile prebuilt libraries exist. `debug-build` without a local `webrtc-build` root is rejected [S3].
4. The compressed payload cost ranges from 15.56 MiB for Linux arm64 to 168.89 MiB for macOS arm64; Windows is 91.79 MiB and the combined XCFramework is 297.09 MiB. Because the library is static, the shipped executable size after linker dead stripping is **unknown** and must be measured; it will not equal the archive size.

### Upstream binary-input provenance

`source-build` maps each wrapper target to the corresponding official `m151.7922.0.0` archive (`webrtc.android.tar.gz`, `webrtc.ios.tar.gz`, `webrtc.macos_arm64.tar.gz`, the six Ubuntu archives, Raspberry Pi OS armv8, or `webrtc.windows_x86_64.zip`) [S4, S9]. The relevant upstream downloads range from 103–112 MiB on Linux arm64/x86-64 through 306 MiB on macOS and 702 MiB on Windows (GitHub byte sizes: 108,257,659–112,353,158; 321,004,814; and 736,553,896 respectively) [S9].

The upstream `m151.7922.0.0` release publishes no checksum sidecars or signatures, and the wrapper CMake uses `file(DOWNLOAD)` without `EXPECTED_HASH` [S4, S9]. Thus the final wrapper assets have published hashes, but the official workflow's downloaded libwebrtc input is not content-verified by a Shiguredo-published digest. This is a provenance limitation, not evidence that the final sidecar hashes are wrong.

### Runtime linkage

The wrapper links `webrtc_c` statically. The final program still links platform facilities [S3]:

- Linux: `m`, `dl`, `rt`, X11, and pthread.
- macOS: libc++ plus AVFoundation, AppKit, AudioToolbox, CoreAudio/CoreMedia, IOSurface, Metal/MetalKit, OpenGL, QuartzCore, ScreenCaptureKit, and VideoToolbox.
- Windows: WinMM, Winsock, DirectShow/DMO identifiers, IP Helper, Security Support Provider, and Windows Media codec DSP UUID libraries.
- Mobile: the listed Apple frameworks or Android `log`, OpenSLES, `m`, and `dl`.

**Inference:** there is no Shiguredo shared library to co-ship on the desktop targets, but OS deployment, SDK/framework availability, Windows CRT choice, and static-code size remain application packaging constraints.

## Toolchains and compatibility floor

| Concern | Documented identity / boundary | Confidence |
|---|---|---|
| Rust MSRV | Rust 1.93 minimum; edition 2024 [S2, S5] | Declared. |
| Rust used for release | Workflow runs `rustup update stable`; no repository `rust-toolchain.toml` pins a release compiler [S6] | Exact Rust patch/version used on 2026-08-10 is **unknown**. MSRV is not proof that release artifacts were produced with 1.93. |
| C/C++ language and build system | CMake minimum 4.2; wrapper targets C20/C++20 [S4] | Declared. |
| Non-Windows wrapper compiler | WebRTC/Chromium Clang and libc++ selected by pinned source metadata; libc++ commit `5abc7f839700f0f17338434e1c1c6a8c87c00c11`, libc++abi `8f11bb1d4438d0239d0dfc1bd9456a9f31629dda`, libunwind `9fe0a380ee56ac20c938cb0c0c35c9b4f7c73339` [S4, S9] | Source revisions exact; human Clang version string is **unknown** from the published Shiguredo manifest. |
| Chromium support repos | build `8edf031b7f329916f82f99e0b27e8e265760cbae`, buildtools `0d39be5a3f129cf1f35e7812108a2184e2193315`, tools `38e7450b95eaad2b581aac90ccdb6e4b4ffec2dc` [S9] | Exact archive `VERSIONS` values. |
| Windows wrapper | Windows 2025 runner targeting MSVC x86-64; source-build docs require Visual Studio 2022 or 2026 Desktop C++ and “C++ Clang tools,” using MSVC for compilation and libclang only for bindgen [S5, S6] | Exact MSVC/Windows SDK patch versions are **unknown**. CMake selects the static multithreaded MSVC runtime [S4]. |
| Apple | wrapper release jobs use `macos-26`; upstream `webrtc-build` uses macOS 26 with Xcode 26.0 [S6, S9] | Exact Xcode 26.0 is documented upstream. Upstream package metadata says macOS deployment target 14 and iOS 14.0; wrapper iOS CMake objects set deployment target 16.0 [S2, S4, S9]. Effective iOS artifact floor is therefore at least 16.0 (**inference**, validate with `otool`). |
| Android | API/platform 24, command-line tools `14742923`, NDK `27.2.12479018`, ABI `arm64-v8a` [S2, S3] | Exact declared inputs. |
| Linux | Ubuntu release jobs run on the corresponding x86-64 image; arm64 assets cross-build on Ubuntu 24.04 against generated target sysroots [S6] | Compiler numeric version and glibc symbol floor are **unknown** until artifact inspection/probe. |

For default prebuilt consumption the README says CMake, libclang, and a C++ compiler are unnecessary; the archive already includes generated Rust bindings [S5]. Build-time tools still include network/download, archive extraction, and SHA-256 utilities [S3].

## Runtime, threads, callbacks, and ownership

### What the API establishes

- The application constructs three `Thread` objects: network (with socket server), worker, and signaling; starts them; passes borrowed pointers into `PeerConnectionFactoryDependencies`; and retains the threads alongside the factory. Setter documentation explicitly says the threads must already be started and lifecycle management belongs to the caller [S5, S7].
- `PeerConnectionFactory` and `PeerConnection` are marked `Send + Sync` because their C++ objects are accessed through libwebrtc sequence-enforcing proxies. `ConnectionContext` is also exposed as `Sync` because wrapper operations route through the signaling thread [S7].
- Factory construction moves the C++ dependency object and, for the context-returning path, performs creation with a blocking call on the signaling thread. **Ownership rule:** the threads must outlive factory/context/peer connections. The README holder's field order drops factory/context before the threads [S5, S7].
- Refcounted WebRTC objects are represented by scoped reference wrappers; callbacks that deliver a transceiver, receiver, or data channel transfer a refcounted handle into an owned Rust wrapper. ICE candidate delivery is only an `IceCandidateRef<'_>` borrowed for the callback [S7]. Retaining that borrowed candidate is not allowed; serialize/copy it during the call.
- Observer handlers require `Send` and are stored in a Rust `Box` transferred to C++ as `user_data`. The C++ observer's destruction invokes `OnDestroy`, at which point Rust reconstructs and drops the box. Construction failure reclaims it immediately [S7].
- One-shot stats callbacks are `FnOnce + Send + 'static` and reclaim their boxed state when invoked. **Unknown:** the examined API does not document a cancellation/destroy callback if a stats result never arrives; verify close/drop behavior.

### Guarantees not present in the official Rust contract

- Exact callback thread for peer, data-channel, audio, video, and stats events: **unknown**. `Send` permits cross-thread delivery but does not promise signaling-thread delivery or serialization.
- Callback reentrancy and whether separate callback types can overlap: **unknown**. The wrappers take `&mut` access to raw handler state and rely on libwebrtc's callback discipline; this needs a stress probe.
- The Rust `PeerConnection::close` contract says peer-connection observer callbacks cease after `close`; that is **documented** for that observer only. Quiescence of data-channel observers, outstanding stats callbacks, and destruction callbacks relative to close/drop remains **unknown**, as does whether the close guarantee is synchronous at return [S7].
- Panic containment: the `extern "C"` trampolines call handlers directly and contain no `catch_unwind` [S7]. Treat callbacks as no-panic; the process outcome on panic must not be left to unwinding across FFI.
- The Rust type system does not encode every native lifetime. In particular, `PeerConnectionDependencies::new(&observer)` stores a native observer pointer without a Rust lifetime parameter [S7]. **Inference:** the observer must outlive the peer connection; prove correct drop ordering and late-callback behavior.
- `Thread::stop` is documented to stop and join. Dropping a still-started thread, pending-task draining, and cross-object shutdown ordering remain **unknown**; call `stop` explicitly only after peers, channels, observers, contexts, and factories are closed/dropped, then prove the sequence under stress [S7].

These are integration obligations, not a verdict on libwebrtc's internal sequencing.

## License, notices, and redistribution

### Declared licenses

- The Rust wrapper and Shiguredo C/C++ wrapper are Apache-2.0 (`license = "Apache-2.0"`) [S2]. Preserve the Apache license, copyright/patent terms, existing notices, and mark modified files where the license requires it. The pinned wrapper tree has no root `NOTICE` file.
- Bundled libwebrtc is under its BSD-style WebRTC license: source redistributions retain copyright/conditions/disclaimer; binary redistributions reproduce them in documentation and/or other materials; Google/contributor names cannot endorse without permission [S8].
- The upstream builder states that its binaries do not include H.264 or H.265 codecs [S9]. Do not infer those codecs from generic libwebrtc capability.

This section is a compliance inventory, not legal advice.

### Notice propagation gap in the wrapper artifacts

The official upstream `webrtc.ubuntu-24.04_x86_64.tar.gz` inspected at the cutoff contains `webrtc/NOTICE` (135,316 bytes) covering WebRTC and bundled third parties, plus `VERSIONS` and `DEPS` [S9]. The wrapper release workflow, however, stages only the wrapper's `LICENSE` and `THIRD_PARTY_LICENSES.md`; the latter contains only the top-level WebRTC BSD text. It does not copy the upstream archive's comprehensive `NOTICE`, `VERSIONS`, or `DEPS` into `libwebrtc_c-*` [S6, S8].

That is a concrete redistribution/package-completeness risk because `libwebrtc_c` statically incorporates upstream code while its distributed wrapper archive omits the comprehensive upstream third-party notice bundle. A downstream distributor should recover and ship the notice set from the exact `m151.7922.0.0` target archive, review it against the final linked application, and not assume the small wrapper `THIRD_PARTY_LICENSES.md` is exhaustive. **Unknown:** Shiguredo does not state in the examined material that the wrapper notice file alone is sufficient for every statically incorporated dependency.

A second packaging detail: `Cargo.toml`'s crate `include` list includes `LICENSE` but not `THIRD_PARTY_LICENSES.md` [S2]. Therefore the Rust source package and the native GitHub archive do not necessarily carry the same compliance files. Confirm the actually downloaded crate contents before relying on Cargo packaging for notices.

## Official sources (10 maximum)

1. **[S1]** Shiguredo `webrtc-rs` [release `0.151.0`](https://github.com/shiguredo/webrtc-rs/releases/tag/0.151.0), including its 12 payload assets and 12 checksum sidecars.
2. **[S2]** Pinned [`Cargo.toml`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/Cargo.toml) at peeled release commit `7052a807…` (crate identity and Apache-2.0 declaration).
3. **[S3]** Pinned [`build.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/build.rs) (target selection, prebuilt download/hash verification, toolchain inputs, and link directives).
4. **[S4]** Pinned wrapper [`webrtc/CMakeLists.txt`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/webrtc/CMakeLists.txt) (upstream archive mapping/download, C/C++ standard, compiler/runtime choices).
5. **[S5]** Pinned [`README.md`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/README.md) (supported systems, MSRV, dependencies, licensing, and factory/thread example).
6. **[S6]** Pinned Shiguredo workflow group: [release](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/.github/workflows/release.yml) and [CI](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/.github/workflows/ci.yml) (release matrix/runners/archive composition/hashes/XCFramework/`--clobber`, and the exact native/source-build CI scope).
7. **[S7]** Pinned wrapper capability-source group at the peeled commit: Rust [`peer_connection.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/api/peer_connection.rs), [`jsep.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/api/jsep.rs), [`data_channel.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/api/data_channel.rs), [`stats.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/api/stats.rs), [`thread.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/rtc_base/thread.rs), and [`ssl_certificate.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/rtc_base/ssl_certificate.rs); companion C/C++ implementation under immutable [`webrtc/src/webrtc_c/api`](https://github.com/shiguredo/webrtc-rs/tree/7052a80710d85eee8f3b081ad585e38d878c614e/webrtc/src/webrtc_c/api); pinned [`src/tests.rs`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/src/tests.rs), [`README.md`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/README.md), and [`examples`](https://github.com/shiguredo/webrtc-rs/tree/7052a80710d85eee8f3b081ad585e38d878c614e/examples). This group is the sole basis for capability claims.
8. **[S8]** Pinned wrapper [`THIRD_PARTY_LICENSES.md`](https://github.com/shiguredo/webrtc-rs/blob/7052a80710d85eee8f3b081ad585e38d878c614e/THIRD_PARTY_LICENSES.md) (the WebRTC BSD notice actually staged with native archives).
9. **[S9]** Official `webrtc-build` [`m151.7922.0.0` release/tag and artifacts](https://github.com/shiguredo-webrtc-build/webrtc-build/releases/tag/m151.7922.0.0), including archive `VERSIONS`, `DEPS`, and `NOTICE`.
10. **[S10]** Official distinct newer `webrtc-build` [`m152.7977.0.0` release/tag](https://github.com/shiguredo-webrtc-build/webrtc-build/releases/tag/m152.7977.0.0).

## Capability acceptance checklist

Artifact identity and pinned wrapper-surface evaluation are complete. The following runtime outcomes remain intentionally unclaimed until the exact later build/probe manifest below is executed:

- [ ] Prove factory/peer/observer/thread drop order, explicit close/stop/join sequence, callback quiescence, and no use-after-free under repeated create/close cycles.
- [ ] Record the OS thread ID for every peer, ICE, data-channel, stats, audio, and video callback; test serialization, overlap, reentrancy, and callback-to-API calls.
- [ ] Install a no-panic callback boundary and prove a handler failure cannot unwind through C/C++ or strand boxed callback state.
- [ ] Validate ICE trickle, restart, continual gathering, candidate removal, selected-pair changes, IPv4/IPv6, mDNS, and network handover.
- [ ] Validate STUN/TURN over UDP, TCP, and TLS; credentials; certificate verification; HTTP proxy behavior; and failure/error observability.
- [ ] Verify offer/answer, glare/rollback, renegotiation, BUNDLE/RTCP-mux, transceiver direction, codec preference, and SDP round trips against the relay's signaling model.
- [ ] Inventory actually exposed audio codecs and prove Opus negotiation/settings; do not assume H.264/H.265 availability for video.
- [ ] Prove external/custom audio ingress and egress format, sample rate, channels, frame cadence, clock ownership, backpressure, mute, and device-less operation.
- [ ] Audit audio/video callbacks for realtime safety: allocations, locks, blocking, thread priority, buffer borrowing, and frame lifetime.
- [ ] Exercise data-channel ordered/unordered and reliable/partial-reliability modes, negotiated channels, maximum message size, buffered amount/backpressure, and close races.
- [ ] Validate stats completeness and cadence for RTT, loss, jitter, bitrate, candidate pair, codec, audio level, and outbound/remote-inbound correlation; test outstanding stats requests during close.
- [ ] Inspect each binary for minimum glibc/macOS/iOS/Windows SDK requirements, imported symbols, static CRT behavior, and unexpected dynamic dependencies.
- [ ] Measure clean-download, extracted, Cargo-cache, incremental-build, final executable, and installer sizes for every relay target; confirm linker dead stripping/LTO behavior.
- [ ] Make a clean offline build from an internally mirrored archive and independently pinned SHA-256; fail closed on a modified archive or sidecar.
- [ ] Generate/ship an exact third-party notice and SBOM set from the `m151.7922.0.0` target archive and verify that it covers the final statically linked binary.


## Pinned `0.151.0` transport capability evaluation

This capability pass is scoped to the immutable, peeled source commit recorded above for Shiguredo WebRTC build `0.151.0`. A behavior is **documented** only when the pinned wrapper source, its tests, examples, or shipped documentation exposes it; **inference** means the wrapper shape permits or implies it but the pinned project does not demonstrate the complete transport behavior; **unknown** means the evidence set does not establish it. No unexposed upstream libwebrtc behavior is credited.

### Offer/answer, rollback, glare, and trickle

* **Offer and answer — documented at wrapper surface:** the native peer-connection wrapper exposes asynchronous offer/answer creation and asynchronous local/remote description application with success/failure callbacks; description setters consume the Rust `SessionDescription`. An adapter must serialize operation state and correlate every callback. This is necessary plumbing, but it is not evidence of Relay's required collision policy [S7].
* **Rollback — representation documented, behavior unverified / hard blocker:** `SdpType::Rollback` is public, and the generic `SessionDescription::new(type, sdp)` can request that type before `set_local_description` or `set_remote_description`. No pinned test or example constructs/applies rollback or proves the required empty-SDP form, state transitions, and error behavior. The surface is therefore present, but successful rollback is not credited until the target/browser glare probes pass [S7].
* **Glare handling — unknown / hard blocker:** no pinned wrapper test or example demonstrates simultaneous offers, polite/impolite negotiation, rollback-on-collision, or deterministic recovery. Relay must supply and validate its own perfect-negotiation state machine; the candidate cannot pass negotiation acceptance until the cross-browser glare probe succeeds.
* **Trickle candidates — documented at wrapper surface:** local ICE candidates arrive through the peer-connection observer callback, and remote candidates can be added through the wrapper. Candidate delivery is callback-driven and must be marshalled off the callback thread before touching higher-level state.
* **End-of-candidates marker — unknown / hard blocker:** an empty/null candidate callback is not, by itself, proof that the public remote-candidate API accepts the browser-compatible end marker. The adapter must not silently drop or synthesize completion until a pinned native↔browser probe establishes the exact representation in both directions.
* **ICE restart — partially documented, operationally unverified:** offer options expose an ICE-restart request at the wrapper boundary. Successful credential rotation, new candidate gathering, media/data continuity, and recovery after an adverse network change remain integration-probe requirements.
* **Configuration changes — partially documented:** the wrapper exposes peer-connection configuration and a configuration update operation, but the safe timing, server replacement semantics, and effects during gathering/checking are not established by the pinned evidence. Treat mid-session reconfiguration as unsupported until probed.

### Immediate adapter acceptance consequences

The wrapper is not yet an acceptable transport adapter solely because SDP and candidates are exposed. **Hard blockers** are: proven rollback representation or an explicitly validated no-rollback collision strategy; deterministic glare recovery; an exact remote end-of-candidates representation; bounded callback-to-owner marshalling; and a teardown protocol that prevents callbacks from reaching closed or destroyed adapter state. The remaining subsections pin the source evidence and define the mandatory build/probe matrix rather than upgrading inference to support.


### ICE servers, restart, configuration, authentication, and certificate policy

| Capability | Pinned evidence | Classification / acceptance consequence |
|---|---|---|
| ICE server list | `IceServer::add_url`, a configuration-owned server vector, username, and password setters are public. A unit test constructs `stun:192.0.2.1:3478` and `turn:192.0.2.2:3478?transport=udp` and round-trips list lengths [S7]. | **Documented configuration surface**, not a connectivity test. Username/password expose long-term credential inputs; expiry/refresh behavior is **unknown**. |
| STUN | The pinned test passes a `stun:` URL through the wrapper [S7]. | URI admission is **documented**; DNS, IPv4/IPv6, server-reflexive gathering, and failure reporting are **unknown** until Coturn probes. |
| TURN/UDP | The pinned test passes `turn:…?transport=udp`, username, and password [S7]. | Configuration is **documented**; allocation, permission/channel bind, credential rejection, relay-only success, and refresh are **unknown**. |
| TURN/TCP | A pinned WHIP/WHEP source comment shows a `turn:…?transport=tcp` Link example, but no wrapper test connects with it [S7]. | Intent is visible, but runtime support is **unknown** and must not be inferred from libwebrtc. |
| TURN/TLS | `TlsCertPolicy::{Secure, InsecureNoCheck}`, per-server TLS client identity, and a callback-based certificate verifier are exposed [S7]. No pinned test/example uses a `turns:` server. | Surface is **partially documented**; TLS transport, SNI/hostname verification, trust roots, failure codes, and whether the dependency-level verifier applies to the required TURN path are **unknown**. |
| Certificate policy | Secure and certificate-check-bypass enum values are public; the unit test only round-trips the enum and configures `InsecureNoCheck` without connecting [S7]. | The default is not credited. Relay acceptance requires explicit `Secure` and a bad-CA/wrong-host/expired-certificate rejection probe. `InsecureNoCheck` is prohibited outside an isolated negative-test lane. |
| ICE transport policy | The Rust enum exposes `Relay`, while other native enum values are not named by the safe wrapper [S7]. | Relay-only is **documented**. A full named `all` policy is absent; default behavior is **inference**, not a wrapper guarantee. |
| Restart | `PeerConnectionOfferAnswerOptions::set_ice_restart(bool)` is public [S7]. | Request surface **documented**; changed ICE credentials, regathering, selected-pair replacement, data continuity, and failed-restart recovery are **unknown**. |
| Reconfiguration | `PeerConnection::set_configuration` returns a wrapper error, but the configuration exposes only its limited server/type/data-section setters [S7]. | Method **documented**; replacement during gathering/connected/restart and credential rotation semantics are **unknown**. |

ICE observation is reasonably shaped but incomplete: the observer exposes standardized ICE state, gathering state, concrete candidates, and structured candidate errors (address, port, URL, code, text). It does not expose candidate removal or selected-pair-change callbacks [S7]. Those must be obtained from stats, if present, or treated as unavailable.

For trickle specifically, each local callback contains a non-null borrowed `IceCandidateRef`; its MID, m-line index, and candidate string must be copied before callback return. Gathering `Complete` can be translated into a **local signaling end marker by adapter policy** (inference). Remote admission requires `IceCandidate::new(mid, index, candidate)` followed by `add_ice_candidate`; there is no nullable/empty remote end-of-candidates method. Therefore exact browser end-marker handling is a **hard blocker**: the adapter must explicitly prove whether consuming the signaling marker without a native call is interoperable, rather than pretending the wrapper accepted it [S7].

### Reliable ordered data channel and backpressure

The pinned surface can create/receive a channel, choose `ordered`, set a protocol string, observe state/messages, send bytes with a binary flag and boolean result, close, and register/unregister an observer [S7]. The C++ shim copies the outgoing slice into a native `DataBuffer` before `Send` returns, so the Rust input borrow does not escape the call. A `true` result only documents native acceptance of that call; it is not delivery acknowledgement.

Reliability is not independently configurable: `DataChannelInit` exposes neither maximum retransmits nor maximum packet lifetime, negotiated mode, nor channel ID. **Inference:** leaving unexposed native defaults unchanged is intended to produce an in-band, reliable channel, and `set_ordered(true)` requests ordering. The pinned wrapper does not test that contract against a peer, so reliable/ordered delivery, reconnection behavior, message limits, and browser compatibility remain runtime obligations.

More importantly, `DataChannel` exposes **no `buffered_amount` getter**, **no buffered-amount-low threshold setter**, and its observer has **no buffered-amount-change/low callback** [S7]. This makes a bounded, event-driven Relay send queue impossible without modifying/upgrading the wrapper or polling an added native surface. Repeatedly calling `send` until its boolean becomes false cannot supply a byte budget, fair scheduling, or a wake-up condition. This is an **adapter backpressure hard blocker**, not merely a missing optimization. A passing candidate must first expose byte-accurate buffered amount plus a low-water notification/threshold, then prove a strict application queue cap, resume behavior, close-race behavior, and no unbounded retry loop.

### Stats

`PeerConnection::get_stats` accepts one `FnOnce + Send + 'static` callback, and `RTCStatsReport` exposes only conversion of the whole report to JSON [S7]. This is **documented snapshot access**. There is no typed iteration, selector, cancellation handle, timeout, or delivery error. The pinned tests do not validate a peer-connection report or its JSON members. Consequently RTT, candidate-pair/transport, bytes/bitrate, loss, jitter, codec, audio level, data-channel, local/remote correlation, timestamp units, and closed-state coverage are all **unknown** until a captured-report probe checks exact keys and semantics.

The Rust callback box is reclaimed only inside the delivery trampoline. The exposed callback table has no destruction/cancellation callback. If native delivery never occurs during concurrent close/drop, the wrapper provides no visible reclamation path. That close-race/leak case is a **teardown hard blocker**; the later probe must include outstanding stats calls and demonstrate callback completion or bounded cancellation/reclamation.

### Callback threads, close, and destruction sequencing

**Documented:** handlers are required to be `Send`; peer/data-channel observer state is boxed and released by each native observer's destruction callback; concrete peer callback values are refcounted except the borrowed ICE candidate; `PeerConnection::close` is irreversible and its Rust contract says peer-connection observer callbacks stop; data-channel observer unregister and close methods exist; `Thread::stop` stops and joins [S5, S7].

**Inference:** libwebrtc proxy objects make calling some peer/channel methods across threads viable, but this does not establish callback thread identity, serialization, non-reentrancy, or callback/API call safety. No such guarantee is credited.

**Unknown / blocked:** peer close does not state whether quiescence is synchronous at return; it says nothing about a separately registered data-channel observer, a one-shot stats callback, or observer destruction timing. The observer parameters are stored as native pointers without Rust lifetime ties. Callback trampolines invoke user handlers directly without panic containment. Relay therefore requires an owner-thread gate and this destruction sequence: stop new work; invalidate a shared callback gate; unregister every data-channel observer; close channels; close the peer; wait for adapter-owned callbacks/stats to settle under a deadline; drop channels and their observers; drop peer then peer observer/dependencies; drop factory/context; finally call `stop`/join on signaling, worker, and network threads. This ordering is an **adapter design requirement**, not a behavior already proven by Shiguredo. Timeout, late-callback, double-destruction, and handler-panic paths must be stress-tested.

### Browser interoperability and CI evidence

The pinned examples are native WHIP/WHEP applications. They demonstrate native offer creation, local-description application, parsing an HTTP-delivered answer, remote-description application, ICE-server configuration, and stats logging; they do not exercise glare, rollback, trickle end markers, data-channel backpressure, or browser peer interop [S7].

The pinned CI runs Rust format/check/test/clippy with source builds on Ubuntu 22.04/24.04/26.04, macOS 15/26, and Windows 2025; cross-builds and smoke-runs Linux arm64/Raspberry Pi outputs and builds iOS/Android [S6]. Repository-wide pinned searches contain no Chrome/Chromium, Firefox, Safari/WebKit, Playwright, Selenium, browser, Coturn, or interoperability job. Thus **browser interoperability evidence is absent**, and CI coverage must not be described as browser compatibility.

### Capability disposition and hard blockers

The pinned wrapper has enough documented surface to prototype offer/answer, concrete trickle candidates, ICE server configuration/restart requests, sends, and JSON stats. Capability evaluation is nevertheless **complete with blockers**, not “unknown pending broad research.” Before acceptance, all of these must close:

1. Glare recovery with a proven rollback application or a proven collision strategy; rollback presence alone is insufficient.
2. Exact local and remote end-of-candidates signaling behavior against browsers.
3. TURN/UDP, TURN/TCP, and TURN/TLS connectivity with authenticated allocations, credential refresh/rejection, and secure certificate validation.
4. Byte-bounded data-channel backpressure. The missing `buffered_amount` and low-threshold event are a wrapper/API hard blocker requiring a wrapper change or different release.
5. Stats close/cancellation ownership and a verified minimum JSON field set.
6. Callback-thread marshalling, panic containment, synchronous quiescence protocol, and destruction-order stress results.
7. Browser interoperability on the pinned browser matrix; official Shiguredo CI supplies none.
8. Redistribution completeness: independently locked native artifacts and the target-matching upstream comprehensive `NOTICE`/license set. The release archive's small notice set is not sufficient evidence [S1, S8, S9].

## Exact later build and probe manifest (execution required; not performed here)

A later authorized run is valid only if it writes one immutable evidence bundle containing the identities and results below. “Latest,” floating tags, an unrecorded browser auto-update, or a live checksum sidecar invalidates the run.

### Identity and build lanes

* Lock wrapper tag object `9720e0829cd968a705f016fa046d25cf3e32cbeb`, peeled commit `7052a80710d85eee8f3b081ad585e38d878c614e`, crate `0.151.0`, builder `m151.7922.0.0`/`0d51a21bbebc6eade24ae5d28d179be9a4b01732`, and libwebrtc `f20ebb8adbf4fa781830e4384c61f732bd28a217`. Record Rust/Cargo exact versions, linker/compiler versions, OS image ID, SDK/NDK/Xcode/MSVC versions, lockfile hash, adapter commit, and all build flags.
* Independently download/vendor and hash-check every payload against the matrix above: Android arm64; iOS arm64; macOS arm64; Raspberry Pi OS armv8; Ubuntu 22.04/24.04/26.04 each on armv8 and x86-64; Windows x86-64; and the macOS+iOS XCFramework. Do not refetch the expected digest from the release during the build.
* For each of the eleven target archives, perform a clean offline adapter link and native smoke run on the matching real OS/architecture (Android and iOS on real arm64 devices, not simulator). Validate the XCFramework independently for archive members, headers, slice identities, and consumer link on its macOS-arm64 and iOS-arm64 slices; it is a bundle lane, not a thirteenth capability target.
* Capture source/archive SHA-256, extracted file manifest, static-library hash, final binary hash/size, imported OS libraries/symbol floors, test log, and crash/leak/sanitizer artifacts. Any forced target on an unlisted distro is a separate experiment and cannot satisfy a published lane.

### Interoperability peers and network fixture

* Native↔native plus native↔browser offerer/answerer in both directions. Browser lanes are Chromium stable, Firefox stable, Firefox ESR, and Safari on each supported Apple desktop line (macOS 15 and macOS 26). Before the run, freeze and record the exact browser product version/build ID, browser binary SHA-256 or signed package identity, driver version, OS image, and launch flags; never record only a channel name. Mobile interoperability adds Chrome on the pinned Android device/build and Safari/WKWebView on the pinned iOS device/build.
* Freeze one official Coturn build by source commit and container/package SHA-256 (a tag alone is invalid), its complete redacted configuration hash, realm, certificate-chain hash/SPKI, and test-account issuance/expiry. Expose separate STUN/UDP, TURN/UDP, TURN/TCP, and TURN/TLS listeners plus an intentionally bad-certificate TLS listener. Record server logs and allocations keyed to each case.
* Run authenticated positive and negative cases: valid long-term username/password; wrong username; wrong password; expired/time-limited credential; refresh across expiry; DNS and literal IPv4/IPv6; relay-only; TLS valid CA/hostname, unknown CA, wrong hostname, and expired certificate. Require `Secure`; run `InsecureNoCheck` only to prove the negative fixture can otherwise connect.

### Negotiation, trickle, data, and stats cases

* Baseline offer/answer in both roles; second renegotiation; simultaneous offers at stable and while setting descriptions; local and remote rollback attempts with the exact empty-SDP representation; duplicate/out-of-order answers; and recovery without a stuck signaling state.
* Trickle every candidate before and after remote description, preserve MID/m-line index/candidate exactly, signal local gathering completion once, consume the browser end marker exactly once, and test duplicate/late/end-before-candidate markers. Repeat with ICE restart and configuration replacement, proving changed ICE credentials and selected-pair replacement.
* On the reliable ordered channel, verify ordered delivery hashes for empty, small, maximum-admitted, and fragmented large messages in both directions. Saturate the send path while the peer stops reading; prove the adapter's byte cap, send rejection handling, low-water wake-up, fairness, and bounded memory. This lane cannot start until buffered-amount/low-threshold exposure exists.
* Snapshot stats before connection, connected on each candidate transport, during saturation/loss/restart, and after close. Preserve raw JSON and assert the Relay-required RTT, local/remote candidate, selected pair/transport, bytes, loss, jitter, codec, data-channel, and timestamp fields. Issue stats concurrently with close/drop and account for every callback allocation.

### Adverse and teardown cases

* Repeat each desktop/browser transport through: 5% and 20% packet loss; 100/500 ms added latency; 10/50 ms jitter; 1% reordering; 256/64 kbit/s shaping; 10-second outage; UDP blocked (forcing TCP/TLS); TURN restart; credential expiry; DNS failure; IPv4↔IPv6 and interface handover; browser reload/crash; and signaling duplication/reordering. Record ICE states, gathering events/errors, selected-pair stats, restart latency, delivery hashes, memory peak, and terminal result.
* For every release runtime target run at least 10,000 create/open/send/close/drop cycles, plus close in each signaling/ICE/data state. Race close with offer/answer callbacks, candidate callbacks, saturated sends, remote close, stats delivery, observer unregister, and process/network teardown. Record every callback OS thread ID and adapter owner-thread hop; fail on post-gate handler entry, panic across FFI, leak growth, deadlock, double free, sanitizer finding, or thread remaining after explicit stop/join.

### Integrity, provenance, and NOTICE gate

* Verify the annotated tag peels to the locked commit; archive hashes equal the independently stored matrix; extracted library/bindings/header hashes are recorded; source-build inputs match `m151.7922.0.0`; and the upstream target archive's `VERSIONS` reports the locked libwebrtc commit. Reject release-asset replacement even if the live sidecar changes with it.
* Inventory each final static binary against the exact target archive's upstream `NOTICE`, `DEPS`, and license files. Ship the wrapper Apache-2.0 license, WebRTC BSD terms, complete target-matching third-party notices, modification notices where applicable, and generated SBOM. Diff the shipped notice inventory against linked components and fail on an uncovered component.
* Record that the wrapper release has no signature/SBOM/SLSA provenance and that its packaged `THIRD_PARTY_LICENSES.md` omits the comprehensive upstream `NOTICE`; do not convert absence into a passing attestation. Archive the fetched release metadata, expected hashes, verification log, notice/SBOM output, and final evidence-bundle hash.

Passing this manifest would close runtime blockers for this exact candidate only. It would not prove later Shiguredo/libwebrtc releases or unlisted targets.
