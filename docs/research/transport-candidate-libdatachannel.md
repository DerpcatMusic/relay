# libdatachannel — RELAY Phase-2 native WebRTC bake-off research

**Retrieval date:** 2026-08-16 UTC  
**Research status:** Complete for paper evidence; no candidate was built or run.  
**Decision status:** Evidence dossier only. This document neither selects nor rejects a bake-off winner.  
**Scope:** The upstream `paullouisageneau/libdatachannel` native library, evaluated from official repository, release, source, license, and build-manifest evidence only.

## Evidence rules

- **Documented** means an upstream statement or a behavior directly visible in the pinned public API/build/source.
- **Inference / unproven** means plausible adapter or runtime behavior that still needs the authorized bake-off probe.
- All source links below use the immutable release commit or an exact dependency gitlink. The maintenance observation uses an immutable post-release commit. No issue threads, blogs, package indexes, or other secondary sources are used.

## Artifact, revision, and maintenance status

| Item | Exact evidence | Assessment |
|---|---|---|
| Candidate | `paullouisageneau/libdatachannel` | The upstream describes it as a standalone C++ implementation of WebRTC Data Channels/Media Transport/WebSockets with C bindings and a browser-shaped API ([README](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L11-L20)). |
| Release | `v0.24.5`, annotated tag object `61204eb447916d259eea6e90f4a50c73d2d062e8`, peeled commit `443f6934d9007eb7076ab7825ba330f355fcbead` | Published 2026-06-12. The official release records a WebSocket TLS fix and libjuice 1.7.2 update ([release](https://github.com/paullouisageneau/libdatachannel/releases/tag/v0.24.5)); CMake declares version 0.24.5 ([manifest](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L1-L5)). |
| Source archive observed | GitHub-generated legacy tarball for commit `443f…`, 52,654,106 bytes, SHA-256 `f161ba30a6e77a5f58dad58f36d26d6e1d74ac7784ef25ef27950e346339d3cf` | Retrieval observation, not an upstream signed release artifact. The release had no attached binary assets at retrieval; commit identity, not this generated-archive hash, is the authoritative pin. |
| Post-release maintenance | `master` at `51085b8de4e6185dc019e3705c88b87933d7c3f6`, committed 2026-08-07 | This is newer than the pinned release and adds Mbed TLS 4.1 work ([commit](https://github.com/paullouisageneau/libdatachannel/commit/51085b8de4e6185dc019e3705c88b87933d7c3f6)). **Inference:** a release two months before retrieval plus later merged work indicates active maintenance; upstream publishes no support-duration/SLA policy in the examined release material. |

The release is source-only for this evaluation. Do not silently substitute `master` for the pinned release in a probe.

## License, notices, and redistribution implications

### Candidate license

The pinned release is **Mozilla Public License 2.0**; upstream states MPL-2.0 has applied since 0.18 ([README](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L27-L29), [LICENSE](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/LICENSE)). The repository tree has no top-level `NOTICE` file.

For distribution, the license text requires covered source and modifications to remain under MPL-2.0, executable recipients to be told how to obtain that source, and existing license notices not to be removed; it permits a Larger Work under terms of the distributor's choice while retaining the covered-software requirements ([MPL §§3.1–3.4](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/LICENSE#L157-L204)). This is a compliance inventory, not legal advice.

### Exact bundled gitlinks at `v0.24.5`

| Dependency | Gitlink | Role in this candidate | License evidence / notice implication |
|---|---|---|---|
| libjuice | `3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6` | Default ICE backend | MPL-2.0 ([license](https://github.com/paullouisageneau/libjuice/blob/3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6/LICENSE)); covered-source obligations apply independently. |
| usrsctp fork | `fec583d54493f879d2ae44a743423bf8a04371ab` | SCTP data channels | BSD-style license with source/binary notice conditions ([license](https://github.com/paullouisageneau/usrsctp/blob/fec583d54493f879d2ae44a743423bf8a04371ab/LICENSE.md)). |
| plog | `94899e0b926ac1b0f4750bfbd495167b4a6ae9ef` | Logging | MIT; retain copyright and permission notice ([license](https://github.com/SergiusTheBest/plog/blob/94899e0b926ac1b0f4750bfbd495167b4a6ae9ef/LICENSE)). |
| libsrtp | `24b3bf8f19b6f5ab4cd2bcceb4f4064efca86fd5` | Media only; avoidable with `NO_MEDIA=ON` | BSD-style notice conditions ([license](https://github.com/cisco/libsrtp/blob/24b3bf8f19b6f5ab4cd2bcceb4f4064efca86fd5/LICENSE)). |
| nlohmann/json | `55f93686c01528224f448c19128836e7df245f72` | Examples only; avoidable with `NO_EXAMPLES=ON` | MIT at the gitlink ([license](https://github.com/nlohmann/json/blob/55f93686c01528224f448c19128836e7df245f72/LICENSE.MIT)). |

The release tree fixes the gitlink commits above; [`.gitmodules`](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/.gitmodules) fixes their canonical upstream URLs. OpenSSL (default), GnuTLS, Mbed TLS, or libnice are found as system libraries and are **not version-pinned** by this release manifest. Their actual binary versions, licenses, notices, and transitive libraries must therefore be captured from the chosen build image; upstream source alone cannot close that compliance inventory.

## Supported targets, toolchains, dependencies, runtime, and thread model

### Targets and toolchains

**Documented:** upstream claims GNU/Linux, Android, FreeBSD, macOS, iOS, and Windows support ([README](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L11-L20)). Its build guide gives POSIX/Make, Xcode, MinGW cross-compile, and MSVC/NMake procedures and exposes CMake shared/static targets ([BUILDING](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/BUILDING.md)). The library target requires C++17. CMake's baseline is 3.13, but the bundled media path requires CMake 3.21; a data-channel-only `NO_MEDIA=ON` build avoids that media-specific requirement ([CMake](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L1-L35), [targets](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L260-L290)).

**CI evidence at the release:** the official workflows build/test Linux and macOS with OpenSSL, GnuTLS, and Mbed TLS; Windows/MSVC with OpenSSL; a Linux libnice configuration; and no-media/no-WebSocket variants ([OpenSSL workflow](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/.github/workflows/build-openssl.yml), [other workflows](https://github.com/paullouisageneau/libdatachannel/tree/443f6934d9007eb7076ab7825ba330f355fcbead/.github/workflows)).

**Unproven for RELAY:** the tag's workflows do not demonstrate Android, iOS, FreeBSD, macOS arm64 as a separately declared matrix entry, cross-compilation from RELAY's Rust workspace, or the four required bake-off targets (Windows x86_64, macOS arm64/x86_64, Linux x86_64). Those remain build probes.

### Dependency choices and binary shape

- Security backend: exactly one of OpenSSL (default), GnuTLS, or Mbed TLS 3 at this tag; ICE backend: libjuice (default) or system libnice; SCTP: usrsctp. Media adds libsrtp. WebSocket support is independent and removable ([build options](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L9-L28), [dependency wiring](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L299-L484)).
- `BUILD_SHARED_LIBS` defaults on. CMake also defines an explicit static target, installs headers/library/PDB where applicable, and exports `LibDataChannel::LibDataChannel` and `LibDataChannel::LibDataChannelStatic` packages ([targets/install](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L260-L297), [install](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/CMakeLists.txt#L491-L552)). Shared-library SOVERSION is `0.24` except Apple, where it is `0`.
- **Inference:** `NO_MEDIA=ON`, `NO_WEBSOCKET=ON`, `NO_EXAMPLES=ON` is the smallest relevant RELAY profile. Static linkage can simplify deployment but incorporates more third-party object code/notices; shared linkage introduces platform loading and deployment concerns. Neither size nor symbol/ABI stability is proven until measured.
- The public C API reduces the adapter's exposure to a C++ ABI, but the binary still contains C++ and native crypto/ICE/SCTP dependencies. Upstream does not ship a first-party Rust binding in this repository. A RELAY adapter therefore needs owned FFI safety/lifetime/thread-affinity work or a separately evaluated binding.

### Runtime and callback threads

- The library owns a global worker pool. Its default is `max(hardware_concurrency, 2)`, and `SetThreadPoolSize` can choose the count; per-object processors preserve task order while dispatching work onto that pool ([global API](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/global.hpp#L30-L56), [initialization](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/init.cpp#L113-L154), [processor](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/processor.hpp#L22-L72)).
- With default libjuice, libdatachannel selects libjuice's shared poll mode (or mux mode when requested), and libjuice may create resolver work; with libnice, libdatachannel owns a GLib main-loop thread. WebSocket support adds a poll-service thread; disabling WebSockets removes that component ([ICE construction](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/icetransport.cpp#L50-L141), [libnice loop](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/icetransport.cpp#L383-L433), [poll service](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/pollservice.cpp#L29-L50)).
- Unresolved remote host candidates can cause a detached `RTC resolver` thread ([source](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L1181-L1213)).
- **Adapter consequence:** callbacks are library-thread callbacks, not Rust-async-executor callbacks. FFI callbacks must not unwind, block, free an object currently calling back, or assume a stable OS thread. Exact callback concurrency/reentrancy under close must be probed.

## Embeddable-library fit

**Documented fit:** the candidate is a library rather than an application/service SDK; it exposes both C++ and C bindings, public peer-connection/data-channel handles, CMake static/shared targets, install/export metadata, callbacks, and explicit global preload/cleanup. The C++ `PeerConnection` surface provides descriptions, candidates, state callbacks, data-channel creation/reception, selected-pair access, and limited counters ([header](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/peerconnection.hpp#L43-L133)). These are compatible in shape with a provider-owned adapter behind RELAY's portable interface.

**Unproven fit:** no upstream evidence covers RELAY's Rust ownership model, command/event epochs, cancellation deadlines, V1 end-of-candidates carrier, or process-wide coexistence with plugin hosts. The global thread pool, global SCTP settings, and global cleanup make multi-instance lifecycle isolation an explicit probe item.

## Signaling and ICE capability

### Offer/answer and trickle

- **Documented:** SDP offer/answer with JSEP-compatible session establishment and Trickle ICE are advertised ([README protocol/features](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L92-L118)). The API has `setLocalDescription`, `setRemoteDescription`, `addRemoteCandidate`, local-description/candidate callbacks, and explicit signaling/gathering states ([header](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/peerconnection.hpp#L64-L123)).
- Auto-negotiation is enabled by default: creating the first channel initiates an offer, and setting a remote offer automatically answers. `disableAutoNegotiation=true` gives the adapter explicit control ([C API docs](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md#L199-L238)). For deterministic RELAY operation-id/epoch ownership, manual negotiation is the safer **inference**, not yet a measured choice.
- `offer`, `answer`, `pranswer`, and `rollback` description types are represented, although RELAY only needs the frozen offer/answer carriers ([C API docs](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md#L214-L228)).

### End-of-candidates

- Locally, gathering completion calls `endLocalCandidates`, sets `Description::mEnded`, and changes gathering state to `Complete`; generated SDP then contains `a=end-of-candidates` ([state path](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L195-L205), [description generation](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/description.cpp#L330-L361)).
- There is no nullable/end marker in `onLocalCandidate`, and no public `addRemoteCandidate(null)`/`endRemoteCandidates` method. A full remote SDP parser recognizes `a=end-of-candidates` ([parser](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/description.cpp#L144-L165)), but the normal trickle API only accepts a concrete `Candidate`.
- **Adapter inference requiring proof:** translate local `GatheringState::Complete` to RELAY's explicit empty-candidate/end carrier. For a remote post-SDP end carrier, the adapter may have to record the generation as ended without a corresponding upstream call. Whether that no-op yields timely failure/completion on both ICE backends is unproven. The official browser example itself sends only non-empty candidates and does not demonstrate an end marker ([example](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/examples/web/script.js#L84-L93)).

### ICE restart — hard blocker in the pinned default profile

The public API has no `restartIce`, no gathering-state reset, and permits `gatherLocalCandidates` only while gathering is `New` ([header](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/peerconnection.hpp#L87-L106), [implementation](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/peerconnection.cpp#L165-L180)). `LocalDescriptionInit` can set credentials before gathering with libjuice, but libnice explicitly does not support that hook ([configuration/API](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/peerconnection.hpp#L38-L41), [backend implementation](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/icetransport.cpp#L144-L148)).

Most decisively, the exact default libjuice gitlink rejects a second remote description whose ICE credentials changed and logs **“ICE restart is not supported”** ([libjuice source](https://github.com/paullouisageneau/libjuice/blob/3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6/src/agent.c#L520-L550)); it also refuses to change local credentials after gathering began ([source](https://github.com/paullouisageneau/libjuice/blob/3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6/src/agent.c#L622-L636)).

**Conclusion:** same-`PeerConnection` browser-initiated and native-initiated ICE restart is documented unsupported for the pinned default backend and is a RELAY hard blocker unless an authorized probe proves an acceptable provider-neutral recovery using another backend or full peer-connection replacement. Replacement is not equivalent to native ICE restart and must be judged against the frozen restart fixtures, channel continuity, epochs, latency, and adapter boundary; this dossier does not approve that workaround.

## STUN/TURN transports and certificate policy

- Ice-server URLs accept `stun`, `turn`, and `turns`, optional credentials, and `transport=udp|tcp|tls`. Defaults are port 3478, port 5349 over TLS, and UDP ([C API docs](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md#L90-L120)).
- With **libjuice**, STUN and TURN/UDP are supported. TURN control over TCP/TLS is rejected; the source limits configured TURN servers to two. ICE-TCP candidate gathering is optional and active-only (`JUICE_ICE_TCP_MODE_ACTIVE`) ([source](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/icetransport.cpp#L38-L40), [ICE-TCP/TURN](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/icetransport.cpp#L134-L180)).
- With **libnice**, TURN control over UDP, TCP, or TLS is mapped to libnice. Upstream explicitly warns that TCP/TLS govern only the TURN control connection; relayed traffic remains UDP ([C API docs](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md#L117-L120), [mapping](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/icetransport.cpp#L600-L645)).
- `iceTransportPolicy=Relay` exists, and local non-relay candidates are suppressed before callback ([configuration](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/configuration.hpp#L64-L90), [filter](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L1093-L1110)).

### Certificates

For WebRTC DTLS, ECDSA is the default, RSA is selectable, the application may supply PEM certificate/key files, and SDP fingerprint checking is enabled unless `disableFingerprintVerification` is explicitly set ([configuration](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/configuration.hpp#L58-L95), [verification](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L442-L463)). Generated defaults use ECDSA P-256 and a SHA-256 SDP fingerprint ([certificate source](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/certificate.cpp#L52-L99)). RELAY must not enable the verification bypass.

For TURN/TLS, the libdatachannel `Configuration` exposes no CA bundle, hostname override, certificate pin, or verification toggle. **Unknown / security blocker:** the effective TURN/TLS trust-store and hostname-validation policy is delegated below this API to the selected libnice build and is not documented by the pinned candidate sources. It must be proven with valid-chain, wrong-host, expired, private-CA, and untrusted-chain probes before TURN/TLS can pass the security gate.

## Data channels, backpressure, and buffered-low

- **Reliable ordered default:** `unordered=false`; if neither maximum lifetime nor maximum retransmits is set, the channel is reliable ([reliability header](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/reliability.hpp#L18-L39)). The upstream protocol inventory identifies RFC 8831 SCTP data channels ([README](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L92-L104)).
- The channel exposes `bufferedAmount`, a low threshold, and a low callback. The callback fires on a strict crossing from above the threshold to at-or-below it ([channel API](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/channel.hpp#L23-L53), [implementation](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/channel.cpp#L52-L61)).
- `send` returns `true` only when sent immediately; `false` means buffered, not rejected. Oversize and closed/not-open cases throw in C++ / return C API errors ([C docs](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md#L493-L509), [SCTP send](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/sctptransport.cpp#L374-L393)).
- The SCTP send queue is constructed with limit `0`; the queue implementation defines `0` as no limit. Therefore the library does **not** provide a bounded application-facing send queue ([construction](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/sctptransport.cpp#L159-L164), [queue semantics](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/queue.hpp#L53-L103)).

**Adapter consequence:** RELAY must impose its own bounded admission queue/high-water mark and pause before unbounded native buffering; `send == false` is a progress signal, not a reason to resend the same message. Exact accounting, low-callback threading, recovery after congestion, and memory ceiling remain mandatory probes.

## Stats and observability

The C++ API exposes peer/ICE/gathering/signaling states, selected candidate pair, local/remote addresses, logging, SCTP bytes sent/received, and SCTP RTT; counters can be cleared ([peer API](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/peerconnection.hpp#L84-L132), [counter implementation](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/peerconnection.cpp#L384-L408)). The C API documents selected-pair/address access but does not expose the C++ byte/RTT methods in `rtc.h` ([C header](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/rtc.h#L210-L235)).

This is not a browser-style `RTCStatsReport`. There is no documented public candidate-pair bitrate/loss, STUN/TURN transaction metrics, DTLS/SCTP substate report, congestion-window, retransmission, per-channel byte count, or standardized stats timestamp. **Unproven:** a C++ shim could expose the limited C++ counters to Rust, but RELAY's full observation inventory still needs adapter-owned state/timing and process metrics. Missing standardized stats is an operability risk, not proof of functional failure.

## Shutdown and lifecycle

- `DataChannel::close` marks the channel closed, requests SCTP stream reset when possible, invokes the closed callback, and clears callbacks ([source](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/datachannel.cpp#L88-L115)).
- `PeerConnection::close` starts transport stop; peer/data-channel/track closure and transport destruction can be asynchronous, with teardown delegated to a process-wide teardown processor ([source](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L82-L106), [teardown](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L376-L418)). Closed state callbacks are invoked synchronously on whichever thread performs the state transition, unlike non-closed state callbacks, which are queued ([source](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/src/impl/peerconnection.cpp#L1333-L1373)).
- C++ `Cleanup()` returns a shared future; global cleanup joins worker/poll resources. The C `rtcCleanup()` wrapper blocks, destroys remaining objects, waits for callbacks, and explicitly must not be called from a callback ([global API](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc/global.hpp#L54-L56), [C docs](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md#L43-L55)).

**Unknowns:** there is no documented bounded close deadline, drain guarantee for already buffered messages, or callback-quiescence token per peer. The probe must verify adapter-owned shutdown ordering: stop admission, unregister/guard callbacks, close channel/peer, await terminal event, release handles, then await global cleanup outside callbacks.

## Browser-interoperability evidence

**Documented claim:** upstream says the stack is compatible with Firefox, Chromium, and Safari ([README](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L23-L25), [compatibility section](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md#L88-L118)). The repository includes an official browser `RTCPeerConnection` client that exchanges offers, answers, candidates, and a data channel with the native example ([web example](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/examples/web/script.js)).

**Evidence limit:** the release workflows run native unit tests only; they do not pin or automate Chromium, Firefox, Safari, coturn, or adverse networks. The example is a usage specimen, not dated/versioned interoperability results, and omits explicit end-of-candidates and restart. Browser interoperability is therefore a documented capability claim but **unproven for RELAY's acceptance matrix**.

## Packaging and binary implications

1. Source consumption is straightforward through CMake and exact gitlinks, but upstream's release page supplies no prebuilt target binaries. RELAY would own reproducible builds, SBOM/notices, signing, and target distribution.
2. The minimal data-channel profile can omit libsrtp, WebSockets, JSON examples, and their associated code/dependency surface. It cannot omit usrsctp, an ICE backend, threading, logging, and a crypto backend.
3. The default source build is not hermetic: OpenSSL/GnuTLS/Mbed TLS and optional libnice come from the environment unless separately pinned. System-library preference can also replace bundled gitlinks. The environment manifest must record the resolved CMake package paths, versions, hashes, and dynamic-library closure.
4. Static and shared outputs both exist, but actual binary size, RSS, startup cost (including certificate generation), symbols, runtime DLL/so/dylib closure, universal-macOS feasibility, and Rust link settings are unknown until the authorized four-target build.
5. MPL-2.0 source-availability and notice handling applies regardless of static versus dynamic packaging; static linkage does not erase third-party license obligations.

## Unknowns and hard blockers

| Severity | Item | Why it remains open |
|---|---|---|
| **Hard blocker at pinned default** | Same-peer ICE restart | Exact libjuice source rejects changed remote ICE credentials; public libdatachannel API cannot reset gathering. Both frozen restart directions must pass or an explicitly accepted replacement strategy must be proven. |
| **Hard blocker until security probe** | TURN/TLS certificate validation | TURN/TLS requires libnice, while libdatachannel exposes no TURN CA/hostname/pin policy. Wrong-host/untrusted/expired behavior is absent from candidate evidence. |
| **Hard blocker until adapter proof** | Bounded backpressure | Native send queue is unlimited. RELAY must prove bounded admission, no duplicates, correct low-water wakeup, and stable memory under saturation. |
| **Hard blocker until lifecycle proof** | Shutdown/callback safety across FFI | Teardown and callbacks span library-owned/global threads, with synchronous Closed callbacks and no per-peer quiescence primitive. |
| Gate evidence missing | Browser/TURN matrix | Upstream claims browser support but has no pinned-browser/coturn CI evidence. |
| Gate evidence missing | Four target builds and packaging | Only a subset is represented in upstream CI; environment/system dependencies are not pinned. |
| Functional uncertainty | Remote trickle end marker | No public end-remote-candidates call. Adapter no-op/recording behavior must be tested on both ICE backends. |
| Operability risk | Limited stats | No standardized WebRTC stats report and C API omits C++ byte/RTT accessors. |
| Unknown | TURN/TCP/TLS plus restart combination | Only libnice provides TURN TCP/TLS control, but restart behavior through the current wrapper is undocumented/unproven. |
| Unknown | Close/drain deadline and network-change recovery | No upstream guarantee establishes a bounded drain or roaming/reconnect behavior. |

## Exact reproducible probe manifest (for a later authorized run)

No commands in this section were executed for this research task. A probe is not reproducible until every blank in RELAY's checked-in `tests/fixtures/transport/environment-manifest-v1.template.json` is replaced and the manifest status identifies an actual run. Use one immutable manifest per backend/build profile; never merge observations from different binaries.

### Immutable source checkout

```text
repository = https://github.com/paullouisageneau/libdatachannel.git
release = v0.24.5
tag_object = 61204eb447916d259eea6e90f4a50c73d2d062e8
commit = 443f6934d9007eb7076ab7825ba330f355fcbead
submodules =
  deps/libjuice  3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6
  deps/usrsctp   fec583d54493f879d2ae44a743423bf8a04371ab
  deps/plog      94899e0b926ac1b0f4750bfbd495167b4a6ae9ef
  deps/libsrtp   24b3bf8f19b6f5ab4cd2bcceb4f4064efca86fd5
  deps/json      55f93686c01528224f448c19128836e7df245f72
```

Later-run checkout assertions:

```sh
git clone https://github.com/paullouisageneau/libdatachannel.git libdatachannel
cd libdatachannel
git checkout --detach 443f6934d9007eb7076ab7825ba330f355fcbead
git submodule update --init --recursive
test "$(git rev-parse HEAD)" = 443f6934d9007eb7076ab7825ba330f355fcbead
git submodule status --recursive
# Fail unless the five direct gitlinks exactly match the table above and none is prefixed '-', '+' or 'U'.
```

### Build profiles to probe separately

**LDC-JUICE-MIN** — relevant minimal/default ICE profile:

```sh
cmake -S . -B build/juice-min -G Ninja \
  -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DBUILD_SHARED_LIBS=OFF -DBUILD_SHARED_DEPS_LIBS=OFF \
  -DUSE_NICE=OFF -DUSE_GNUTLS=OFF -DUSE_MBEDTLS=OFF \
  -DPREFER_SYSTEM_LIB=OFF \
  -DNO_MEDIA=ON -DNO_WEBSOCKET=ON -DNO_EXAMPLES=ON -DNO_TESTS=ON
cmake --build build/juice-min --target datachannel
```

**LDC-NICE-MIN** — required to evaluate TURN control over TCP/TLS:

```sh
cmake -S . -B build/nice-min -G Ninja \
  -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DBUILD_SHARED_LIBS=OFF -DBUILD_SHARED_DEPS_LIBS=OFF \
  -DUSE_NICE=ON -DUSE_GNUTLS=OFF -DUSE_MBEDTLS=OFF \
  -DPREFER_SYSTEM_LIB=OFF \
  -DNO_MEDIA=ON -DNO_WEBSOCKET=ON -DNO_EXAMPLES=ON -DNO_TESTS=ON
cmake --build build/nice-min --target datachannel
```

Before either configure step, pin and record image digest, OS/architecture, CMake, Ninja, compiler/linker, SDK, Rust/Cargo, OpenSSL, libnice/GLib (NICE profile), C/C++ runtimes, and the complete linked-library hashes. `find_package(OpenSSL REQUIRED)` and `find_package(LibNice REQUIRED)` make those environment pins mandatory; upstream does not provide them.

### Required target/browser/network matrix

- Targets, each from its own manifest: Windows x86_64; macOS arm64; macOS x86_64; Linux x86_64 (CI reference).
- Browsers: pinned automated Chromium and Firefox on supported desktop targets; pinned Safari/manual on macOS. Record version, driver, flags, executable signature/hash, and certificate SHA-256 exactly as the RELAY template requires.
- Isolated coturn only; no public STUN/TURN service. Pin coturn image/binary digest and config hash. Test STUN/host, TURN/UDP on both profiles, and TURN/TCP plus TURN/TLS on NICE only. For TLS use a recorded private CA and separately exercise valid host, wrong host, expired leaf, and untrusted CA.
- Predeclared one-attempt default, fixed impairment seeds/profiles, synchronized UTC start/end, and the complete RELAY required-observation list.

### Functional probe cases and pass evidence

1. Both offerer directions and both answerer directions using `disableAutoNegotiation=true`; preserve opaque SDP and correlate RELAY operation id plus negotiation epoch.
2. Trickle candidates in both directions; local `Complete` must emit exactly one RELAY end carrier per generation. Remote end must be accepted exactly once without inventing a candidate, and ICE must reach a terminal success/failure within the declared timeout.
3. Reliable ordered data channel: negotiated default reliability; monotonic sequence plus payload SHA-256; prove no loss, duplicate, or reorder in baseline and impaired profiles.
4. Browser-initiated and native-initiated restart from the frozen baseline/restart credential generations. Record whether the same native peer/channel survives. Treat peer replacement separately and fail continuity unless the predeclared rubric explicitly accepts it.
5. Saturate send until buffering occurs; enforce an adapter high-water bound, verify `false` is not resent, low callback wakes once per threshold crossing, all admitted sequence numbers arrive once, and process RSS stays below the declared ceiling.
6. Selected pair/type/protocol, all state transitions, SCTP bytes/RTT, errors, process CPU/RSS, binary size and dynamic closure. Explicitly mark unavailable DTLS/SCTP stats rather than synthesizing them.
7. Shutdown from idle, connected, congested, remote-close, and callback-adjacent states: bounded deadline, no callback after adapter destruction, no deadlock/UAF, and clean sanitizer/crash output. Await global cleanup off callback threads.
8. Run the declared loss/jitter/reorder/roaming/reconnect profiles and record first-attempt results separately.

### Required artifacts

- Completed `environment-manifest-v1` and `scorecard-v1` per profile/target.
- Configure/build logs; `CMakeCache.txt`; compiler/linker command database; source/submodule status; dependency/SBOM and notices; exact binary and dynamic-library hashes/sizes.
- Machine-readable event timeline, selected-pair snapshots, message ledger, buffered-amount/RSS time series, restart generations, coturn logs, browser logs, packet capture where policy permits, sanitizer/crash output, and shutdown timings.
- Raw evidence must show failures as failures. In particular, source evidence predicts LDC-JUICE-MIN ICE-restart failure; the probe must not relabel peer replacement as a same-peer restart.

## Primary-source index

1. [Pinned repository tree](https://github.com/paullouisageneau/libdatachannel/tree/443f6934d9007eb7076ab7825ba330f355fcbead)
2. [Official v0.24.5 release](https://github.com/paullouisageneau/libdatachannel/releases/tag/v0.24.5)
3. [README / compatibility](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/README.md)
4. [C API documentation](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/DOC.md)
5. [Build guide and CMake manifest](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/BUILDING.md)
6. [MPL-2.0 license](https://github.com/paullouisageneau/libdatachannel/blob/443f6934d9007eb7076ab7825ba330f355fcbead/LICENSE)
7. [Public C++ headers](https://github.com/paullouisageneau/libdatachannel/tree/443f6934d9007eb7076ab7825ba330f355fcbead/include/rtc)
8. [Exact libjuice ICE-restart behavior](https://github.com/paullouisageneau/libjuice/blob/3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6/src/agent.c#L520-L550)
9. [Official release CI workflows](https://github.com/paullouisageneau/libdatachannel/tree/443f6934d9007eb7076ab7825ba330f355fcbead/.github/workflows)
10. [Post-release maintenance commit](https://github.com/paullouisageneau/libdatachannel/commit/51085b8de4e6185dc019e3705c88b87933d7c3f6)
