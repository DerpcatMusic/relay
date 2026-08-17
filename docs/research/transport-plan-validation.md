# Transport Plan Validation

## Scope

Validate `docs/plans/2026-08-15-relay-transport-plan.md` against a tightly bounded set of primary sources for the named native WebRTC transport candidates: libdatachannel/libnice, Shiguredo libwebrtc, and webrtc-rs. This record evaluates factual claims and decision implications only; it does not edit the master plan or implementation code.

## Validation criteria

- Candidate maintenance and upstream dependency posture are stated accurately.
- Platform/build/toolchain claims are supported by current primary documentation or repository metadata.
- Data-channel, ICE, DTLS/SCTP, and callback/API claims that affect the bakeoff are evidence-backed.
- Risks and spike gates in the plan follow from the cited evidence rather than unsupported assumptions.
- Any correction is phrased narrowly enough to apply to the plan without prematurely selecting a transport.

## Source table

| # | Candidate | Primary source | Evidence consulted | Plan implication |
|---|---|---|---|---|
| 1 | libdatachannel / libnice | [libdatachannel README](https://github.com/paullouisageneau/libdatachannel/blob/master/README.md) (accessed 2026-08-15 22:13 UTC) | Project identity, supported platforms, C binding, selectable TLS and ICE backends, dependencies, examples, compatibility/features | Confirms this is an embeddable native peer/data-channel candidate; validates separate dependency and backend inventory, browser tests, and provider-neutral callback/state mapping. |
| 2 | Shiguredo libwebrtc | [Shiguredo WebRTC-Build README](https://github.com/shiguredo-webrtc-build/webrtc-build/blob/master/README.md) (accessed 2026-08-15 22:13 UTC) | Artifact purpose and contents, distributed targets, deprecated targets, release/tag scheme, top-level license | Identifies a maintained embeddable libwebrtc binary/header distribution, not a distinct WebRTC implementation or Sora-only client; macOS x86_64 is explicitly discontinued. |
| 3 | webrtc-rs | [webrtc-rs README](https://github.com/webrtc-rs/webrtc/blob/master/README.md) (accessed 2026-08-15 22:14 UTC) | Architecture, async/runtime model, feature/provider selection, release lines, backpressure claim, build/test flow, licensing | Confirms a Rust-native candidate with explicit async-driver/runtime implications; reveals meaningful stable-v0.20 versus v0.21-alpha pinning tradeoff and pre-1.0 API risk. |
| 4 | libnice | [libnice project site](https://libnice.freedesktop.org/) (accessed 2026-08-15 22:15 UTC) | Scope, WebRTC ICE interoperability claim, implemented ICE/STUN/TURN/trickle standards, IP support, partial ICE-TCP support, latest release listing | Confirms libnice is a GLib ICE backend/dependency rather than a complete peer-connection candidate; supports backend-specific relay/connectivity probing and careful distinction between ICE-TCP and TURN-over-TCP. |

## Findings

### 1. libdatachannel README

The upstream project describes itself as a standalone C++ implementation of WebRTC Data Channels and Media Transport with C bindings, supporting GNU/Linux, Android, FreeBSD, macOS, iOS, and Windows. It explicitly targets direct native-to-browser connectivity and documents Firefox, Chromium, and Safari compatibility. Its examples expose local-description, local-candidate, peer-state, data-channel, and message callbacks matching the broad shape assumed by Gate 0.

The README also makes backend variability material: TLS may use GnuTLS, Mbed TLS, or OpenSSL; ICE defaults to libjuice but may use libnice; SCTP uses usrsctp; media adds libsrtp. It claims Trickle ICE, STUN/TURN, IPv6, SDP/JSEP, and SCTP-over-DTLS support, but does not by itself prove all mandatory transport variants, certificate-failure behavior, ICE restart, stats coverage, or cross-platform parity.

**Validation result:** the plan accurately characterizes libdatachannel and correctly requires an exact build/dependency inventory plus empirical interoperability, TURN/TLS, recovery, packaging, and licensing gates. The plan should ensure the probe pins and reports the chosen ICE and TLS backends because those are score-affecting configurations, not incidental build details.

### 2. Shiguredo WebRTC-Build README

Shiguredo's repository says it builds WebRTC for multiple environments and distributes a WebRTC library (`webrtc.lib` or `libwebrtc.a`), headers, and the underlying WebRTC commit hash. Current listed builds include Windows x86_64/arm64, macOS arm64, Ubuntu x86_64/arm64 variants, Android arm64, and iOS arm64. It explicitly lists macOS x86_64 as discontinued since June 2022. Releases/tags encode the libwebrtc milestone, branch, commit position, and Shiguredo release revision.

This is primary evidence for an embeddable native artifact, but it is a packaged build of libwebrtc rather than a separately described “Shiguredo native WebRTC stack/SDK.” It is not Sora-service-only. Its README does not promise a stable wrapper API, C ABI, or the exact adapter capabilities in Gate 0; the probe would integrate the native libwebrtc API and must assess its C++ ABI/toolchain/runtime surface. The repository's Apache-2.0 license statement applies to WebRTC-Build itself and is not sufficient evidence for all code inside the distributed libwebrtc artifact.

**Validation result:** T3's conditional validation is justified, but the evidence now permits a narrower candidate identity: Shiguredo WebRTC-Build, pinned by release/tag and underlying libwebrtc commit. Required-target wording must not imply macOS x86_64 support from this artifact. The plan's transitive-license and packaging gates remain essential.

### 3. webrtc-rs README

The project identifies itself as an async-friendly Rust WebRTC implementation. Its documented architecture is a Sans-I/O `rtc` protocol core plus a thin async `webrtc` layer whose automatically spawned background driver owns sockets, drives timeouts, and dispatches events. Tokio is the default runtime backend; smol and a mock runtime are features, and custom runtimes implement a trait. Crypto providers are also selectable. These are direct reasons to keep T4's runtime/executor, ownership, shutdown, and dependency analysis.

The README distinguishes a recommended production line (`v0.20.x`) from an in-development `v0.21.x` line (`v0.21.0-alpha.1`) heading toward a stable 1.0 API. It says 0.x minor releases may be breaking. Master documents useful v0.21 changes including deterministic time and receive-side data-channel backpressure, but those claims must not be attributed to v0.20 without release-specific verification. The README points to runnable data-channel and ICE-restart examples, but it does not establish the plan's full TURN transport/certificate, browser matrix, standardized stats, target packaging, or lifecycle requirements.

The documented public interface is a Rust async API; the README does not advertise a C ABI. Therefore a “Rust/C ABI boundary” is conditional on the actual RELAY embedding boundary rather than an intrinsic webrtc-rs requirement.

**Validation result:** retain webrtc-rs, but the probe must choose the release line before capability scoring and must not mix master/v0.21-alpha documentation with v0.20 evidence. Runtime integration is a confirmed evaluation axis. A C ABI should be evaluated only if a non-Rust host boundary requires one.

### 4. libnice project site

The official site defines libnice as a GLib implementation of ICE, not a full WebRTC peer-connection or data-channel stack. It claims interoperability with the WebRTC library used by major browsers and lists ICE RFC 8445, TURN relay client RFC 5766, STUN RFC 5389, Trickle ICE RFC 8838, and IPv4/IPv6 support. It labels ICE-TCP support under RFC 6544 as partial (active and passive candidates only). The release list reaches 0.1.23.

This supports the plan's treatment of libnice as a libdatachannel backend/dependency rather than a fourth candidate. It also reinforces why the exact backend must be captured: choosing libnice adds GLib and its own release/dependency/behavior surface. Critically, ICE-TCP candidates and contacting a TURN server over TCP/TLS are different mechanisms; a source claim for one must not be used as proof of the other. The official overview does not document `turns:` certificate validation or prove the plan's forced TURN/TCP/TLS cases.

**Validation result:** no candidate-list correction is needed. Keep libnice nested under the selected libdatachannel configuration. Preserve separate empirical TURN/UDP, TURN/TCP, TURN/TLS, blocked-UDP, IPv6, and TLS-failure assertions; do not infer them wholesale from standards listed on the libnice site.

## Explicit potential corrections to the master plan

1. In T2 and the reproducibility manifest, explicitly require the chosen libdatachannel **ICE backend** (libjuice or libnice) and **TLS backend** (GnuTLS, Mbed TLS, or OpenSSL) to be pinned and scored as part of the candidate configuration. The general “feature flags and transitive dependencies” language implies this, but naming these axes prevents results from being incorrectly generalized across materially different builds.
2. Replace the provisional Shiguredo label, once the spike starts, with **Shiguredo WebRTC-Build (libwebrtc binary/header distribution)** and pin both its release/tag and recorded upstream libwebrtc commit. Do not describe it as an independent Shiguredo WebRTC implementation.
3. Make the target-matrix caveat concrete: Shiguredo WebRTC-Build currently lists macOS arm64 but explicitly discontinued macOS x86_64. Either treat macOS x86_64 as unsupported for this candidate under the existing “as supported” qualifier, build and own a separately validated x86_64 artifact, or reject it if x86_64 is a product hard requirement.
4. Do not infer the distributed libwebrtc artifact's full licensing from WebRTC-Build's Apache-2.0 repository license; retain the per-binary transitive inventory/legal review already required by the plan.
5. Add a T4 precondition to pin the webrtc-rs **release line** before capability mapping. Upstream calls v0.20 the current production recommendation while v0.21 is an alpha on the path to 1.0; master-only v0.21 behavior (including the documented backpressure change) must not be credited to a v0.20 probe.
6. Change T4's unconditional “Rust/C ABI boundary” wording to **host-language/ABI boundary, if any**. The documented API is native Rust/async; a C ABI is relevant only if RELAY's selected native host cannot consume Rust directly. Keep runtime/executor implications unconditional.
7. In result schemas and capability reports, distinguish **ICE-TCP candidates** from **TURN server transport over TCP** and **TURN over TLS**. libnice documents only partial ICE-TCP support on its overview; this must neither pass nor fail the distinct TURN/TCP and `turns:` gates without probe evidence.
8. If the libdatachannel probe chooses libnice, include GLib and the pinned libnice release in the packaging, runtime, and transitive-license inventory; do not score “libdatachannel” independently of that backend choice.

## Decisions reflected in the plan

- Retain libdatachannel as a real embeddable native candidate.
- Retain empirical browser/TURN/TLS/recovery testing; upstream compatibility statements are eligibility evidence, not gate proof.
- Retain the exact pinned-build dependency/license inventory because backend selection changes the shipped dependency set.
- Treat Shiguredo WebRTC-Build as an eligible embeddable libwebrtc distribution, not as a Sora client and not as a separate protocol stack.
- Preserve T3's early fit/license rejection path because the distribution README alone does not establish adapter ergonomics, stable ABI, or full artifact licensing.
- Preserve “as supported” platform accounting and record macOS x86_64 as a known gap rather than silently expecting a supplied binary.
- Retain webrtc-rs as a Rust-native candidate and retain explicit runtime/executor and lifecycle scoring.
- Treat v0.20 and v0.21-alpha as distinct candidate configurations; do not combine their evidence.
- Do not assume a C ABI is required or supplied until the host-language boundary is fixed.
- Keep empirical backpressure tests even where upstream documents improved behavior.
- Keep libnice as an optional libdatachannel ICE backend, not a standalone WebRTC candidate.
- Preserve distinct TURN/TCP, TURN/TLS, ICE-TCP, and certificate-failure evidence fields; no primary source consulted proves them equivalent.

## Validation proof

- Evidence file created before reading the plan or consulting external sources, as required.
- Source 1 was recorded immediately after consultation at 2026-08-15 22:13 UTC.
- Source 1 facts are traceable to the project-maintained README; upstream claims are not treated as test results.
- Source 2 was recorded immediately after consultation at 2026-08-15 22:13 UTC.
- Source 3 was recorded immediately after consultation at 2026-08-15 22:14 UTC.
- Source 4 was recorded immediately after consultation at 2026-08-15 22:15 UTC.
- External consultation stopped at four substantive primary sources. A failed README URL returned only a 404 page and supplied no evidence.
- No plan or code files were modified.
