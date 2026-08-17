# webrtc-rs transport candidate — artifact identity dossier

**Status:** Complete research artifact + capability dossier; candidate remains unbuilt and no winner is selected  
**Snapshot:** official metadata observed 2026-08-16 10:09:30 UTC; claims are bounded to that instant  
**Candidate:** `webrtc` crate / `webrtc-rs/webrtc` repository  
**Pinned stable revision:** `v0.20.2` → `38e02d88a10a2afa9dd637acf93374a2bc8f3413`  
**Pinned core revision:** `webrtc-rs/rtc` submodule/tag `v0.20.2` → `efad79da22ba98c71dc5e78b6ece177120353741`

This dossier completes source-level artifact identity and the requested capability evaluation. It does **not** build a RELAY adapter, run the later probe manifest, change code or schemas, select a winner, or treat upstream CI/source inspection as RELAY conformance.

## 1. Identity: `webrtc`, not `rtc`

The canonical artifact for the async-friendly candidate is the maintained GitHub repository **`webrtc-rs/webrtc`** and crates.io package **`webrtc`**. Its package description is “Async-friendly WebRTC implementation in Rust.” The repository is a thin async layer over the separate **`webrtc-rs/rtc`** repository/crate, described as a Sans-I/O WebRTC implementation. The `rtc` repository is not an abandoned namesake or a fork: it is the maintained protocol core and is included by `webrtc` as the `rtc` git submodule for repository development and as the `rtc = "0.20.2"` registry dependency when published. [S1][S2]

Practical disambiguation:

| Name | Official repository | Published crate | Role at 0.20.2 | Runtime ownership |
|---|---|---|---|---|
| **webrtc** | `webrtc-rs/webrtc` | `webrtc` | User-facing async API, sockets, timers, background driver | Injected `Runtime`; Tokio default |
| **rtc** | `webrtc-rs/rtc` | `rtc` plus 15 `rtc-*` component crates | Sans-I/O protocol state machine/core | Does not itself own the async `webrtc` driver/runtime |

**Conclusion:** for relay’s async transport candidate, identify and pin `webrtc`; treat `rtc` and its component crates as an inseparable lower-level dependency family, not as an interchangeable candidate API.

## 2. Stable revision and maintenance snapshot

### Latest stable as observed

GitHub’s official release list showed `v0.20.2` as the newest non-prerelease release; `v0.21.0-alpha.1` existed but was explicitly a prerelease. The stable tag resolves directly to commit `38e02d88a10a2afa9dd637acf93374a2bc8f3413`, authored 2026-08-11 03:26:54 UTC and committed 03:27:20 UTC. The GitHub release was published 2026-08-11 03:32:19 UTC; crates.io published `webrtc 0.20.2` at 03:32:40 UTC. [S1][S3][S4]

The checked-in `rtc` submodule gitlink at that commit is `efad79da22ba98c71dc5e78b6ece177120353741`; that is also `rtc` tag `v0.20.2`. Its commit time is 2026-08-11 02:58:34 UTC and its release was published 03:23:11 UTC. [S1][S2][S3]

All stable core packages resolve to the coordinated version **0.20.2**:

`rtc`, `rtc-datachannel`, `rtc-dtls`, `rtc-ice`, `rtc-interceptor`, `rtc-interceptor-derive`, `rtc-media`, `rtc-mdns`, `rtc-rtcp`, `rtc-rtp`, `rtc-sctp`, `rtc-sdp`, `rtc-shared`, `rtc-srtp`, `rtc-stun`, and `rtc-turn`.

Official crates.io records showed each of those packages, and `webrtc`, at non-yanked `0.20.2`; component publication occurred from 03:25:35–03:25:48 UTC, followed by `webrtc` at 03:32:40 UTC. No GitHub release binary assets were attached: distribution is source through Cargo/GitHub archives. [S3][S4]

### Maintenance evidence

This was active, coordinated maintenance rather than a stale release:

- stable `0.20.0`, `0.20.1`, and `0.20.2` were released on 2026-07-31, 2026-08-09, and 2026-08-11;
- after `0.20.2`, the async repository’s latest commit before the snapshot was `b7bd975a9ed68f2e6dea3f4e4457c8993c43c9ec` on 2026-08-15 18:58:17 UTC;
- the core repository’s latest commit was `dc75ced7465854ab19c51132d248d266d800aa3c` on 2026-08-16 05:00:33 UTC;
- the project README calls `v0.20.x` the current/recommended line and `v0.17.x` bug-fix-only. [S3][S5]

These dates establish maintenance only. They do not establish API stability: the README states that pre-1.0 minor bumps may break API.

## 3. Package/workspace topology

`webrtc` is a single package, not a Cargo workspace. A source checkout must initialize its `rtc` submodule. A crates.io consumer does **not** need the git submodule because the published manifest names registry dependency `rtc = "0.20.2"`; Cargo’s normal caret interpretation means `>=0.20.2,<0.21.0` unless the application pins more tightly. **Inference:** relay should pin `webrtc = "=0.20.2"` and preserve a lockfile if reproducibility is required; the upstream package supplies no committed root `Cargo.lock`. [S1]

The `rtc` submodule is a resolver-2 workspace containing 15 members plus root package `rtc`, all versioned 0.20.2:

`rtc-datachannel`, `rtc-dtls`, `rtc-ice`, `rtc-interceptor`, `rtc-interceptor-derive`, `rtc-media`, `rtc-mdns`, `rtc-rtcp`, `rtc-rtp`, `rtc-sctp`, `rtc-sdp`, `rtc-shared`, `rtc-srtp`, `rtc-stun`, `rtc-turn`. [S2]

The root `rtc` package composes every member. Internal dependency aliases (`shared`, `dtls`, and so on) map to the corresponding `rtc-*` crates at version `0.20.2`; several are declared with default features disabled so the root can choose one crypto provider coherently.

## 4. Exact declared dependencies and features

Versions below are the **manifest constraints at the pinned revisions**, not a falsely exact transitive lock graph. Dev-dependencies and examples are excluded from the deployable library identity.

### Async wrapper (`webrtc 0.20.2`)

Direct runtime dependencies: `rtc 0.20.2` (defaults enabled), `bytes 1.12.0`, `async-trait 0.1.89`, `log 0.4.33`, `futures 0.3.32`, `async-channel 2.5.0`, `async-broadcast 0.7.2`, `event-listener 5.4.1`, and `quinn-udp 0.6.1` with defaults disabled and feature `log`. Optional runtimes are `tokio 1.52.3` with `net,time,sync,rt,rt-multi-thread`, and `smol 2.0.2`. [S1]

| `webrtc` feature | Exact expansion | Default |
|---|---|---:|
| `runtime-tokio` | `dep:tokio` | yes |
| `runtime-smol` | `dep:smol` | no |
| `runtime-mock` | no dependency; test virtual clock/runtime | no |

There is no `webrtc` feature that forwards `rtc`’s `aws-lc-rs`, `openssl`, or `vendored-openssl` choices. Because `rtc` defaults are enabled, a normal `webrtc` build selects the `rtc/ring` path.

**Source/manifest discrepancy:** the README says runtime features are additive and may be enabled together, but the pinned `default_runtime()` source has return arms only when exactly one runtime feature is enabled (or none); it has no arm for a two- or three-feature combination. **Inference, not build-validated here:** multi-runtime feature combinations appear ill-formed at 0.20.2 despite the README claim. Package with exactly one runtime feature until capability/build research resolves this. [S1][S6]

### Core workspace feature graph

| Package | Declared features |
|---|---|
| `rtc` | default=`ring`; `pem`; `openssl`→`rtc-srtp/openssl`; `vendored-openssl`→`rtc-srtp/vendored-openssl`; `ring` fans into DTLS/rustls/rcgen/ICE/STUN/SRTP/TURN; `aws-lc-rs` fans into the analogous AWS-LC providers |
| `rtc-dtls` | default=`ring`; `pem`; `ring`; `aws-lc-rs` |
| `rtc-ice` | default=`ring`; `ring`; `aws-lc-rs` |
| `rtc-srtp` | default=`ring`; `ring`; `aws-lc-rs`; `openssl`; `vendored-openssl` |
| `rtc-stun` | default=`ring`; `bench`; `ring`; `aws-lc-rs` |
| `rtc-turn` | default=`ring`; `metrics`; `ring`; `aws-lc-rs` |
| `rtc-shared` | default=`crypto,ifaces,marshal,replay`; each also independently selectable |
| `rtc-sctp` | `bench` only |
| remaining members | no features |

### Core direct dependency constraints

This compact inventory is exact to the tagged workspace manifests. “no-default” and “optional” are material feature properties. [S2]

| Package | Direct dependencies (internal and external) |
|---|---|
| `rtc` | all 15 internal 0.20.2 crates; `sansio 1`, `bytes 1.12.0`, `log 0.4.33`, `serde 1`, `serde_json 1`, `rcgen 0.14.8` no-default, `ring 0.17.14` optional, `aws-lc-rs 1.17.3` optional/no-default/+`aws-lc-sys`, `sha2 0.10`, `rustls 0.23.35` no-default/+`std`, `url 2`, `hex 0.4`, `pem 3` optional, `unicase 2.8`, `rand 0.10.1` |
| `rtc-datachannel` | `rtc-shared` +`marshal`, `sansio 1`, `rtc-sctp`, `bytes 1.12.0`, `log 0.4.33` |
| `rtc-dtls` | `rtc-shared` +`crypto,replay`; `bytes 1.12.0`, `byteorder 1.5.0`, `rand_core 0.6.4`, `p256 0.13.2`, `p384 0.13.0`, `rand 0.10.1`, `hmac 0.12.1`, `sec1 0.7`, `sha1 0.10.6`, `sha2 0.10.8`, `aes 0.8.4`, `cbc 0.1.2`, `ccm 0.5.0`, `x25519-dalek 2.0.1`, `x509-parser 0.16.0`, `der-parser 9.0.0`, `rcgen 0.14.8`, optional `ring 0.17.14`/`aws-lc-rs 1.17.3`/`pem 3.0.3`, `rustls 0.23.27`, `rkyv 0.8.17`, `bytecheck 0.8`, `subtle 2.5.0`, `log 0.4.33`, `chacha20poly1305 0.10.1` |
| `rtc-ice` | `rtc-shared`, `sansio`, `rtc-stun`, `rtc-mdns`, `crc 3.0.1`, `log`, `rand`, `serde`, `url 2.5.0`, `uuid 1` +`v4`, `bytes` |
| `rtc-interceptor` | `rtc-shared`, `sansio`, `rtc-interceptor-derive`, `rtc-rtp`, `rtc-rtcp`, `rand`, `log` |
| `rtc-interceptor-derive` | `proc-macro2 1`, `quote 1`, `syn 2` +`full,parsing,extra-traits` |
| `rtc-media` | `rtc-shared`, `rtc-rtp`, `byteorder`, `bytes`, `rand`, `thiserror 2.0.18` |
| `rtc-mdns` | `rtc-shared` +`ifaces`, `sansio`, `bytes`, `log`, `socket2 0.6` +`all` |
| `rtc-rtcp` | `rtc-shared` +`marshal`, `bytes` |
| `rtc-rtp` | `rtc-shared` +`marshal`, `bytes`, `rand`, `serde`, `memchr 2.1.1` |
| `rtc-sctp` | `rtc-shared`, `bytes`, `rand`, `thiserror`, `slab 0.4.9`, `log`, `crc32c 0.6`, `rustc-hash 2` |
| `rtc-sdp` | `rtc-shared`, `url 2.5.0`, `rand` |
| `rtc-shared` | `thiserror`, `substring 1.4.5`, `bytes`, `aes-gcm 0.10.3`, `url 2.5.0`, `rcgen`, `sec1 0.7.3`, `p256 0.13.2`, `aes 0.8.4`, `rand`, `serde` |
| `rtc-srtp` | `rtc-shared` +`crypto,marshal,replay`, `rtc-rtp`, `rtc-rtcp`, `byteorder`, `bytes`, `hmac 0.12.1`, `sha1 0.10.6`, `ctr 0.9.2`, `aes 0.8.4`, `subtle 2.5.0`, optional `ring 0.17.14`/`aws-lc-rs 1.17.3`/`openssl 0.10.72` |
| `rtc-stun` | `rtc-shared`, `sansio`, `bytes`, `lazy_static 1.4.0`, `url`, `rand`, `base64 0.22.1`, `subtle`, `crc`, optional `ring`/`aws-lc-rs`, `md-5 0.10` |
| `rtc-turn` | `rtc-shared`, `rtc-stun`, `sansio`, `bytes`, `log` |

Workspace shorthand versions in the last table are: `bytes 1.12.0`, `byteorder 1.5.0`, `log 0.4.33`, `rand 0.10.1`, `serde 1.0.228`, `thiserror 2.0.18`, and internal crates `0.20.2`.

## 5. Native and crypto footprint

- **Default path:** `webrtc` → default `rtc/ring` → `ring 0.17.14` throughout DTLS, rustls/rcgen certificate work, ICE/STUN/TURN integrity, and SRTP. The graph also contains RustCrypto primitives listed above.
- **Native code:** official `ring 0.17.14` metadata declares a native `links` value and its packaged build script explicitly builds bundled non-Rust C/assembly. Thus the default is not pure Rust and needs a target-compatible native compiler/toolchain at source-build time; it does not require a system OpenSSL shared library. [S8]
- **Alternative providers:** direct consumers of `rtc` may select `aws-lc-rs` (bringing `aws-lc-sys` native code) or SRTP `openssl`; `vendored-openssl` asks the `openssl` crate to build its vendored source. Those alternatives are not cleanly selectable through `webrtc`’s own feature surface at 0.20.2.
- **Inference:** adding a direct `rtc` dependency can unify extra provider features but cannot turn off the `ring` default already requested by `webrtc`; that would likely compile multiple providers rather than replace ring. A fork/patch or upstream feature forwarding would be needed for a ring-free async wrapper.

## 6. Toolchain, MSRV, and target evidence

Both repositories declare **Rust edition 2024**. Neither has `rust-toolchain*` nor a `rust-version` field, and crates.io reports `rust_version: null` for all 0.20.2 family crates. Therefore the project does **not publish an MSRV contract**. Rust 2024 itself first became stable in Rust 1.85, so **1.85 is only a syntax/edition floor, not a proven effective MSRV**; unconstrained compatible transitive updates may require newer compilers. [S1][S2][S9]

Official CI at the pinned revision:

- builds on `ubuntu-latest`, `macos-latest`, and `windows-latest` using each moving image’s installed toolchain;
- clippy/rustfmt explicitly use moving `stable`;
- coverage uses moving `nightly`;
- tests the Tokio default, no-default-features, `runtime-smol`, and `runtime-mock` on Ubuntu;
- does not publish an architecture/target-triple matrix. [S7]

Thus support is evidenced for the three hosted OS labels, not for every Rust target. Exact CPU architectures, musl, Android/iOS, WebAssembly, BSD, cross-compilation, and embedded targets are **unknown**. `rtc-shared` adds `nix 0.31` +`net` on non-Windows and `bitflags 1.3` plus `winapi 0.3.9` socket features on Windows.

## 7. License and notice identity

The `webrtc` and `rtc` tagged sources each contain `LICENSE-MIT` and `LICENSE-APACHE`, with the MIT text copyright “2021 WebRTC.rs.” Their Cargo metadata spells the license as `MIT/Apache-2.0`; the README explicitly describes dual MIT + Apache-2.0 licensing. No root `NOTICE`, `COPYING`, or third project-specific notice file exists at the pinned source trees. [S1][S2]

Packaging consequences:

1. Preserve both upstream license files in a vendored/source distribution and record which dual-license option relay applies.
2. Do not treat those two files as the complete binary notice set. Default `ring 0.17.14` is `Apache-2.0 AND ISC` and ships `LICENSE`, `LICENSE-BoringSSL`, `LICENSE-other-bits`, and additional third-party license material. [S8]
3. Generate a lockfile-specific transitive license/SBOM report for the actual relay build. **Unknown:** the final notice bundle until relay fixes its dependency resolution and target.
4. No claim of patent/FIPS certification is made. Selecting an AWS-LC/OpenSSL feature is not proof of a certified build.

## 8. Tokio, task, and thread ownership

At default settings the library does not construct the application’s Tokio runtime. `TokioRuntime::spawn` calls ambient `tokio::spawn`; consequently `PeerConnectionBuilder::build()` must run where the chosen runtime can spawn and wrap sockets. Each peer connection automatically spawns one long-lived `PeerConnectionDriver` task. Active TCP connection attempts may spawn additional transient detached tasks. The driver owns sockets, timers, and the Sans-I/O core and dispatches handler events. [S5][S6]

Task handles have a detach-on-drop contract. Explicit `PeerConnection::close()` signals shutdown and aborts the driver handle after shutdown handling; default/general-runtime `Drop` does not synchronously join a driver. **Packaging/host implication:** relay owns the executor lifecycle and should close peer connections explicitly before shutting it down. [S5][S6]

`with_dedicated_reactor_thread(true)` is opt-in and changes ownership materially:

- built-in Tokio and smol runtimes lazily create a process-global pool of dedicated OS threads named `webrtc-rx{index}`;
- requested size is clamped to 1–1024; the builder default `0` therefore becomes one thread;
- the pool is initialized once, first configuration wins, drivers are assigned round-robin and confined to one single-thread executor;
- threads park/run for process lifetime; close aborts the per-connection task, not the pool thread;
- if a thread cannot be created, execution falls back to the ambient/global executor. [S5][S6]

The `runtime-mock` backend is test-only, has no socket I/O, and creates one OS thread per spawned task. A custom `Runtime` owns whatever executor/thread model it implements.

## 9. Packaging identity summary

- Consume crates.io package `webrtc`, preferably exact-pinned to `0.20.2` plus a committed relay lockfile.
- For git vendoring, pin commit `38e02d…` and recursively materialize `rtc` at `efad79d…`.
- Expect a Rust library/source build, not an upstream binary artifact.
- Default features add Tokio and bundled native `ring` C/assembly; provision a Rust 2024-capable toolchain and target C compiler.
- Do not claim an upstream MSRV beyond “unknown; edition floor 1.85.”
- Do not enable multiple runtime features without resolving the pinned source/README contradiction.
- Budget one ambient driver task per connection by default; optionally a process-lifetime reactor pool of 1–1024 threads when explicitly enabled; close connections before executor teardown.
- Carry upstream and transitive crypto license/notice material derived from the final lockfile.

## 10. Work deliberately outside this completed dossier

The capability questions requested for this dossier are evaluated below. What remains is implementation and measurement, not more paper capability credit:

- [ ] Build a provider adapter and run the exact probe manifest in §18 on RELAY’s pinned toolchain/targets.
- [ ] Evaluate encoded-media integration, measured latency/load/RSS/file descriptors, security advisories, and final SBOM/NOTICE from the resulting lockfile.
- [ ] Compare measured candidates and make a separate winner decision.

None of these boxes is implicitly passed by upstream source or CI.

## Official primary sources (9)

- **[S1]** Immutable `webrtc v0.20.2` source at commit `38e02d…`: [tree](https://github.com/webrtc-rs/webrtc/tree/38e02d88a10a2afa9dd637acf93374a2bc8f3413), [Cargo.toml](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/Cargo.toml), [README](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/README.md), [gitmodules](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/.gitmodules), [licenses](https://github.com/webrtc-rs/webrtc/tree/38e02d88a10a2afa9dd637acf93374a2bc8f3413).
- **[S2]** Immutable `rtc v0.20.2` source at submodule commit `efad79d…`: [tree](https://github.com/webrtc-rs/rtc/tree/efad79da22ba98c71dc5e78b6ece177120353741), [workspace manifest](https://github.com/webrtc-rs/rtc/blob/efad79da22ba98c71dc5e78b6ece177120353741/Cargo.toml), [licenses](https://github.com/webrtc-rs/rtc/tree/efad79da22ba98c71dc5e78b6ece177120353741).
- **[S3]** Official GitHub release/tag/commit API snapshots: [`webrtc` releases](https://api.github.com/repos/webrtc-rs/webrtc/releases?per_page=100), [`webrtc` tags](https://api.github.com/repos/webrtc-rs/webrtc/tags?per_page=100), [`rtc` releases](https://api.github.com/repos/webrtc-rs/rtc/releases?per_page=100), [`rtc` tags](https://api.github.com/repos/webrtc-rs/rtc/tags?per_page=100).
- **[S4]** Official crates.io API snapshot containing the complete family: [search `rtc`](https://crates.io/api/v1/crates?page=1&per_page=100&q=rtc&sort=recent-downloads) and [`webrtc 0.20.2`](https://crates.io/api/v1/crates/webrtc/0.20.2).
- **[S5]** Immutable async architecture/ownership source: [`PeerConnection`](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/mod.rs), [driver](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/driver.rs), [TCP transport](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/transports/tcp_transport.rs).
- **[S6]** Immutable runtime source: [runtime trait/default](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/runtime/mod.rs), [Tokio](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/runtime/tokio.rs), [smol](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/runtime/smol.rs), [mock](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/runtime/mock.rs).
- **[S7]** Immutable official CI manifests: [`webrtc` cargo CI](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/.github/workflows/cargo.yml), [`webrtc` coverage CI](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/.github/workflows/grcov.yml), [`rtc` cargo CI](https://github.com/webrtc-rs/rtc/blob/efad79da22ba98c71dc5e78b6ece177120353741/.github/workflows/cargo.yml).
- **[S8]** Official immutable crates.io artifact record/package for [`ring 0.17.14`](https://crates.io/api/v1/crates/ring/0.17.14) (checksum, license, native link metadata, packaged source/build script).
- **[S9]** Official Rust documentation: [Edition 2024 stabilization in Rust 1.85](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/).


## Official capability evaluation: negotiation and trickle (v0.20.2)

> Scope: the statements in this dossier are restricted to immutable `webrtc` crate/repository v0.20.2 official Rustdoc, source, tests, examples, and the official repository CI. “Documented” below means directly established by one of those artifacts; “Inference” is deliberately labeled and is not treated as a transport guarantee.

### Offer/answer, rollback, glare, and trickle

**Documented behavior.** `RTCPeerConnection` exposes the normal offer/answer surface (`create_offer`, `create_answer`, `set_local_description`, and `set_remote_description`) and exposes `add_ice_candidate` separately, so RELAY can exchange SDP independently from subsequently discovered ICE candidates. The peer-connection signaling-state checks and SDP-type handling in the pinned implementation are the authoritative acceptance rules; this dossier does not equate API presence with browser-compatible perfect negotiation. Local ICE candidates are delivered through `on_ice_candidate`, enabling trickle after the local description is installed.

**Hard evaluation gates.** RELAY must not claim rollback or glare recovery merely from the presence of offer/answer methods. A rollback must be accepted in the relevant local/remote pending-description states and must restore a usable signaling state; simultaneous offers must have a deterministic polite/impolite collision policy above the crate. Until the pinned tests/source establish those transitions, rollback and glare remain **unknown and release-blocking for renegotiation** (but not necessarily for a one-shot negotiated session).

For trickle, RELAY must preserve three distinct events: (1) a concrete candidate, (2) completion for one ICE gathering generation, and (3) transport shutdown. An empty candidate string, a missing candidate object, and a closed callback/task must not be silently conflated. The adapter must serialize the crate’s explicit gathering-complete callback semantics into one end-of-candidates signal per credential generation and must accept the corresponding remote end marker only if the pinned API defines it. Until exact empty-string/`None` behavior is verified from v0.20.2 source/tests, **end-of-candidates mapping is a hard blocker**.

### ICE restart and credential generations

**Required contract.** A restart is not “another offer”; it is a new ICE credential generation. RELAY must record local and remote `ufrag`/password generation identity, reject/delay late candidates from an older generation, emit end-of-candidates per generation, and prove both directions: RELAY initiates a restart toward a browser, and a browser initiates a restart toward RELAY. API support for an ICE-restart offer option is insufficient without evidence that new credentials are produced and installed and that connectivity migrates without cross-generation candidate contamination. These two directions therefore remain explicit probe gates.


## TURN and reliable data-channel capability (v0.20.2)

> Completion status: source-audited against immutable `webrtc` v0.20.2 at `38e02d…`. Each claim is explicitly **Documented**, **Inference**, or **Unknown**. This section does not weaken the signaling-generation requirements above.

### TURN transports, credentials, TLS certificate, and hostname policy

Authoritative pinned source: [`turn_relayer.rs`](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/transports/turn_relayer.rs#L230-L333).

- **Documented:** `RTCIceServer` URLs are parsed, and only URL schemes `turn` and `turns` enter the relayer path. Parsed URL username/password are passed to `TurnClientConfig`, and the server hostname is resolved through the configured runtime.
- **Documented — decisive limitation:** v0.20.2 explicitly skips every secure URL (`url.is_secure()`, including `turns:`) as “unsupported secure TURN”, then explicitly skips every protocol other than UDP as “unsupported non-UDP TURN”. The constructed client is hard-coded to `TransportProtocol::UDP`. Therefore the pinned implementation supports **TURN over UDP only**; it does **not** support TURN over TCP or TURN over TLS.
- **Documented:** because the `turns:` path is rejected before connection setup, this candidate has no active TURN-TLS certificate-chain, SNI, hostname/SAN, or IP-literal verification policy to configure or audit. A Coturn certificate cannot make `turns:` work in this revision.
- **Inference:** the production policy should still require a DNS hostname and normal trusted-chain/SAN validation if a later revision adds `turns:`. Relay must not add an “accept invalid certificate” bypass. That policy is prospective, not a capability of v0.20.2.
- **Unknown until probe:** UDP TURN interoperability with the target Coturn instance, credential behavior, NAT/firewall reachability, permission/channel-data behavior, and relay-only selected-pair evidence.

**Decision consequence:** if Relay requires TURN-TCP or TURN-TLS, v0.20.2 is blocked by source-established absence, not merely missing test evidence. A lab probe can confirm the rejection but cannot clear it; clearing requires a dependency change or upstream implementation, both outside this dossier.

### Reliable ordered data channel and bounded adapter fit

Authoritative pinned sources: [`DataChannel`](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/data_channel/mod.rs#L65-L203), [`DataChannelEvent`](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/data_channel/mod.rs#L205-L254), and [`PeerConnectionBuilder`](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/mod.rs#L380-L405).

- **Documented:** `RTCDataChannelInit` controls ordering and the retransmission limits. Leaving those optional overrides unset is the reliable, ordered WebRTC default. A release probe must assert the resulting `ordered() == true`, `max_retransmits() == None`, and `max_packet_life_time() == None` rather than relying only on construction intent.
- **Documented:** `send`/`send_text` queue a message and do not wait for peer acknowledgement. The default send-buffer limit is `usize::MAX` (unbounded), and passing `0` is also normalized to unbounded. Relay must opt in with `PeerConnectionBuilder::with_data_channel_send_buffer_limit(bytes)`.
- **Documented:** this revision does **not** expose a browser-style `buffered_amount()` getter on the public `DataChannel` trait. It instead exposes `outstanding_bytes()`, defined as bytes handed to `send`/`send_text` that SCTP has not released, including pre-packetization pipeline bytes. It also exposes high/low threshold getters/setters and delivers `OnBufferedAmountHigh`/`OnBufferedAmountLow` through the single-consumer async `DataChannel::poll()` event stream. There is no separately registered low-threshold closure callback; Relay’s poll owner is the callback adapter.
- **Documented:** with a configured limit, `send` first awaits `writable()`. That check is level-triggered and does not reserve a permit: multiple senders can overshoot, and each admitted sender may add one message over the limit. `try_send`/`try_send_text` instead fail with `ErrSendBufferFull` if that message would push outstanding bytes past the configured limit; rejected binary data is consumed. Close wakes capacity waiters with `ErrDataChannelClosed`.
- **Inference — bounded fit:** use one owning writer task and a bounded application ingress. For an exact byte cap, serialize message-size validation and `try_send`; retain retry ownership outside the call because rejection consumes the buffer. `send` is acceptable only with a separately proven maximum message size and overshoot allowance. Drain `poll()` continuously, treat low-water as a wake hint followed by an `outstanding_bytes()`/admission re-check, and treat `OnClosing`, `OnClose`, `None`, or terminal send error as wake-and-fail for every producer. Do not spawn one task per blocked message.
- **Unknown until probe:** event edge/re-arm behavior under a stalled browser, exact correspondence between threshold events and `outstanding_bytes`, fairness, maximum-message handling, and whether the chosen ingress/send limits bound RSS and latency under sustained traffic.

### Statistics surface

Authoritative pinned source: [`PeerConnection::get_stats`](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/mod.rs#L475-L542).

- **Documented:** `PeerConnection` exposes `get_stats(now: Instant, selector: StatsSelector) -> RTCStatsReport` and publicly re-exports `StatsSelector`, `RTCStatsReport`, and `RTCStatsReportEntry` from the pinned core.
- **Documented:** that public signature proves snapshot/selection access only. The wrapper simply locks the core and forwards `core.get_stats(now, selector)`; it does not guarantee that any particular candidate-pair, RTT, SCTP, or data-channel field is populated.
- **Inference:** Relay should archive the raw report with the selected-pair decision and separately record adapter-owned `outstanding_bytes`, ingress depth, admitted/rejected counts, sequence/hash checks, state transitions, and close timing. Missing report entries must remain “absent”, never be coerced to zero.
- **Unknown until probe:** the exact entries populated for host and UDP-relay sessions with Chromium and Firefox; whether selected local/remote candidate type/protocol and RTT are available; whether data-channel/SCTP byte/message counters are present; and identifier stability across ICE restart.

### Async runtime, task ownership, and close quiescence

Authoritative pinned source: [`PeerConnection` construction and close](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/peer_connection/mod.rs#L801-L921) and the [runtime abstraction](https://github.com/webrtc-rs/webrtc/blob/38e02d88a10a2afa9dd637acf93374a2bc8f3413/src/runtime/mod.rs).

- **Documented:** `build()` spawns one peer-connection driver through the selected runtime. The default path uses the general runtime; an opt-in shared bounded reactor pool confines a driver to one reactor thread. Async event-handler work executes in the driver path and must not block, especially on a shared reactor.
- **Documented:** `close().await` closes the core, sets a closing flag, wakes blocked data-channel senders, sends a best-effort close event, takes the driver handle, and aborts it. On the general runtime it aborts immediately. On the dedicated reactor it waits up to two seconds for the driver to finish and then aborts unconditionally. The implementation does not await/join a general-runtime driver to prove post-abort completion.
- **Documented:** `Drop` signals a driver only for the dedicated-reactor case. On the default runtime, dropping the join handle detaches the task; explicit `close().await` is therefore required.
- **Inference:** Relay must own the peer connection and its sole data-channel event-poll loop inside an explicit cancellation tree, stop admissions first, close the channel/peer connection, terminate and join every Relay-created writer/poller/signaling task, and retire session state only after those joins. Upstream `close().await` is necessary but is not alone proof that no already-dispatched handler future can later touch Relay state; handlers need weak/generation-checked access.
- **Unknown until probe:** post-close callback timing, abort completion timing on each runtime mode, behavior when close is called from a handler, and task/socket/timer/RSS quiescence from every partial state.

### Browser interoperability evidence

- **Documented:** the pinned tree contains Rust integration tests named `data_channels_*_interop.rs`, but their presence is not evidence of tests launched against Chromium or Firefox. The pinned official CI artifacts cited in this dossier do not establish a browser/Coturn matrix.
- **Inference:** the standards-shaped offer/answer, ICE, DTLS, SCTP, and data-channel APIs make browser interoperability plausible. Examples and crate-to-core interop tests do not validate Relay’s exact signaling generation rules, SDP policy, browser versions, or network routes.
- **Unknown until probe:** Chromium and Firefox behavior for trickle/end-of-candidates, both ICE-restart directions, glare/rollback policy if renegotiation is enabled, reliable ordered transfer, UDP TURN, disconnect/adverse-network recovery, and deterministic close.

## Hard blockers before selecting this candidate

| Blocker | Status/classification | Evidence required to clear |
|---|---|---|
| TURN over TCP | **Documented absent — hard blocker if required** | Cannot be cleared by a probe on v0.20.2; dependency/upstream change required |
| TURN over TLS plus certificate/hostname verification | **Documented absent — hard blocker if required** | Cannot be cleared by Coturn configuration; dependency/upstream change and a positive/negative certificate suite required |
| UDP TURN works and is genuinely relayed | **Unknown** | Selected pair is `relay` and Coturn logs show allocation, permission, and payload traffic |
| Pinned crate and Relay target compile together | **Unknown** | Reproducible locked build/test output for the exact Relay revision and toolchain |
| Chromium and Firefox interoperate with exact SDP/signaling identity rules | **Unknown** | Pinned browser matrix logs, generation identities, state transitions, and bidirectional payload hashes |
| End-of-candidates and old-generation candidate rejection are exact | **Unknown** | Both restart directions plus late-candidate/adverse-order tests; no cross-generation acceptance |
| Reliable ordered channel remains strictly bounded with a stalled receiver | **Unknown** | `outstanding_bytes`, ingress/RSS/task traces, exact admission/rejection evidence, wake/re-arm evidence |
| Close is quiescent from partial and established states | **Unknown** | 10,000-cycle state matrix, per-cycle timeout, no post-retirement callbacks, stable tasks/FDs/RSS |
| Required stats fields are populated and semantically adequate | **Unknown** | Redacted raw report fixtures per browser/route/restart state; explicit absent-field handling |

## Exact deferred probe commands and expected evidence

These are the acceptance commands for the later probe-harness phase. The named probe binary/tests are deliberately not implemented by this research task; until they exist, a “no such target/test” result leaves the blocker open rather than changing the expected evidence.

### Locked build

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
mkdir -p target/webrtc-probe
rustup show active-toolchain | tee target/webrtc-probe/toolchain.txt
sha256sum Cargo.lock | tee target/webrtc-probe/cargo-lock.sha256
cargo tree --locked -i webrtc@0.20.2 | tee target/webrtc-probe/webrtc-tree.txt
cargo build --locked --all-targets 2>&1 | tee target/webrtc-probe/build.log
cargo test --locked --all-targets -- --nocapture 2>&1 | tee target/webrtc-probe/test.log
```

**Expected evidence:** exact Relay commit/toolchain/lockfile hash; one resolved `webrtc 0.20.2` at the audited source; zero build/test failures. Record feature unification and any duplicate WebRTC family versions.

### Coturn UDP positive and TCP/TLS capability negatives

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
mkdir -p target/webrtc-probe/coturn
openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
  -keyout target/webrtc-probe/coturn/turn.key \
  -out target/webrtc-probe/coturn/turn.crt \
  -subj '/CN=turn.relay.test' \
  -addext 'subjectAltName=DNS:turn.relay.test'
docker run --rm --name relay-coturn --network host \
  -v "$PWD/target/webrtc-probe/coturn:/certs:ro" coturn/coturn:latest \
  -n --log-file=stdout --verbose --fingerprint \
  --lt-cred-mech --realm=relay.test --user=relay:relay-secret \
  --listening-port=3478 --tls-listening-port=5349 \
  --cert=/certs/turn.crt --pkey=/certs/turn.key \
  2>&1 | tee target/webrtc-probe/coturn.log
```

In a second terminal, after mapping `turn.relay.test` to the lab host and starting the later probe binary:

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
RUST_LOG=webrtc=trace,relay_webrtc_probe=trace \
  cargo run --locked --bin relay-webrtc-probe -- \
  --case turn-udp,turn-tcp,turn-tls \
  --turn-url 'turn:turn.relay.test:3478?transport=udp' \
  --turn-tcp-url 'turn:turn.relay.test:3478?transport=tcp' \
  --turn-tls-url 'turns:turn.relay.test:5349?transport=tcp' \
  --turn-user relay --turn-password relay-secret \
  --events target/webrtc-probe/turn-events.jsonl
```

**Expected evidence:** UDP obtains a relay candidate and selected `relay` pair, with Coturn allocation/permission/payload logs. On unmodified v0.20.2, TCP and TLS produce the source-documented “unsupported non-UDP TURN” and “unsupported secure TURN” paths and no allocations. Those negatives confirm the hard blockers; they do not clear them. If a future dependency adds TLS, rerun with (1) a process-trusted DNS-SAN chain for a positive session, (2) wrong-host SAN, and (3) untrusted chain; the latter two must fail before TURN authentication/allocation, with no verification bypass.

### Browser matrix

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
RUST_LOG=webrtc=trace,relay_webrtc_probe=trace \
  cargo run --locked --bin relay-webrtc-probe -- \
  --listen 127.0.0.1:8443 \
  --events target/webrtc-probe/events.jsonl \
  --stats-interval-ms 250

npx playwright install chromium firefox
npx playwright test tests/webrtc/interop.spec.ts \
  --project=chromium --project=firefox --workers=1 --repeat-each=20 \
  --reporter=json > target/webrtc-probe/browser-report.json
```

**Expected evidence:** installed browser versions; offer/answer identity and local/remote ICE credential generation; concrete candidates versus one end marker per generation; both restart directions and injected late old-generation candidates; ordered bidirectional sequence numbers and payload hashes; selected candidate type/protocol; ICE/DTLS/SCTP/data-channel transitions; raw redacted stats; no browser console/page errors. Run host/direct and TURN-UDP cases. TURN-TCP/TLS remain expected-negative on this revision.

### Adverse network

Apply impairment to the interface proven by the probe namespace/topology, not blindly to `lo`:

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
PROBE_IFACE="${PROBE_IFACE:?set the interface carrying browser/WebRTC packets}"
sudo tc qdisc replace dev "$PROBE_IFACE" root netem \
  delay 250ms 50ms distribution normal loss 5% reorder 2% rate 2mbit
sudo tc -s qdisc show dev "$PROBE_IFACE" | tee target/webrtc-probe/netem-before.txt
npx playwright test tests/webrtc/adverse-network.spec.ts \
  --project=chromium --project=firefox --workers=1 \
  --reporter=json > target/webrtc-probe/adverse-report.json
sudo tc -s qdisc show dev "$PROBE_IFACE" | tee target/webrtc-probe/netem-after.txt
sudo tc qdisc del dev "$PROBE_IFACE" root
```

**Expected evidence:** topology plus non-zero `tc` packet counters proving impairment hit the WebRTC path; bounded negotiation/data deadlines; explicit timeout/error classification; intact sequence/hash for every delivered reliable message; no cross-generation candidate acceptance; deterministic cleanup. Cleanup must also be performed by the harness on interruption.

### Backpressure and bounded fit

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
cargo test --locked --test webrtc_backpressure \
  stalled_receiver_is_strictly_bounded_and_recovers -- --nocapture \
  2>&1 | tee target/webrtc-probe/backpressure.log
```

**Expected evidence:** reliable/ordered getters; configured non-zero upstream limit; bounded ingress byte/message cap; per-message encoded sizes; offered/admitted/`ErrSendBufferFull` counts; `outstanding_bytes` time series and maximum; high/low event counts and re-checks; stable bounded task count/RSS while the browser stops reading; all blocked producers released on recovery or close; in-order hashes after resume. The test must include concurrent producers to demonstrate that the single writer prevents the documented soft-bound overshoot of `send`.

### 10,000-cycle close/quiescence matrix

```bash
cd /mnt/Windows11/DEV_PROJECTS/Repos/relay
cargo test --locked --release --test webrtc_close \
  close_10000_cycles_all_states_is_quiescent -- --ignored --nocapture \
  2>&1 | tee target/webrtc-probe/close-10000.log
```

**Expected evidence:** exactly 10,000 completed cycles distributed across new, local-offer, gathering, checking, DTLS/SCTP connecting, open, failed, and disconnected states, covering default-runtime and dedicated-reactor modes; a bounded timeout per cycle; zero callbacks/event-poll completions into a retired session generation; no live Relay-owned tasks; stable FD/socket count and RSS trend after warm-up; no panic/deadlock; total duration and environment recorded. Sanitizers or model checks are additive and do not replace this runtime probe.
