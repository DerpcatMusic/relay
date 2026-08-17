# RELAY Phase 2 Native Transport Bake-off Plan

**Date:** 2026-08-15  
**Status:** In progress — T0 fixtures/rubric complete; T1 Gate-0 interface/fake active; no transport selection yet  
**Decision scope:** Native WebRTC transport/provider only

## Execution checkpoint (2026-08-16)

- **T0 complete:** 15 SHA-frozen V1 offer/answer/candidate/end/resume/peer-left/ICE-restart fixtures, Rust/TypeScript round-trip tests, environment manifest, and scorecard rubric are recorded in `docs/research/transport-t0-fixtures-rubric.md`.
- V1 intentionally cannot encode transient ICE/DTLS/SCTP/peer-connection disconnect state; `PeerUpdate(LEFT)` means logical departure only. ICE restart has no typed transaction id and is correlated by ordered revision plus opaque SDP ICE generation in the harness.
- Three interface shapes were compared and synthesized in `docs/design/transport-interface-synthesis.md`. T1 implements the selected object-safe factory plus single-owner command/event `PeerDriver` with explicit operation ids and negotiation epochs.
- **Gate 0 remains open** until the fake contract suite is implemented, independently reviewed, and shown to preserve the V1 fixtures with no provider types in the portable core.

## Objective

Run a bounded, evidence-producing bake-off of three native WebRTC candidates and select (or reject) a provider behind a RELAY-owned adapter. The bake-off must prove browser interoperability and adverse-network recovery without allowing candidate-specific types or signaling assumptions into the portable core or the V1 wire contract.

## Decision boundary

This phase evaluates only the native transport/provider seam. It does **not** select a plugin shell, redesign the portable core, change the V1 signaling/wire protocol, or ship production media features. A candidate advances only through the adapter interface below; direct integration into product code is out of scope.

## Candidates

1. **libdatachannel** — native C++ WebRTC data-channel/media transport library.
2. **Shiguredo native WebRTC stack/SDK candidate** — use the currently supported Shiguredo native client artifact identified during source validation; pin the exact repository, release, and supported targets before probe work begins.
3. **webrtc-rs** — Rust-native WebRTC implementation.

The Shiguredo candidate is deliberately conditional until the primary-source validation identifies which maintained native artifact actually fits RELAY's peer-to-browser, embeddable-library requirement. A hosted-service-only or opinionated Sora-client artifact must not be treated as a drop-in peer-connection library.

## Adapter interface gate (Gate 0)

Before candidate-specific code, one agent writes a provider-neutral probe contract and fixture tests. The contract is the only interface the harness may call.

```text
NativeTransportProvider
  capabilities() -> CapabilityReport
  create_peer(config: PeerConfig, events: EventSink) -> Peer

Peer
  set_remote_description(description)
  add_remote_ice_candidate(candidate)
  create_offer() / create_answer()
  set_local_description(description)
  open_data_channel(label, options) -> Channel
  close(reason)
  stats_snapshot() -> TransportStats

EventSink
  on_local_description(description)
  on_local_ice_candidate(candidate)
  on_connection_state(state)
  on_ice_state(state)
  on_data_channel(channel)
  on_error(typed_error)

Channel
  send(bytes) -> explicit backpressure/result
  buffered_amount()
  close()
  events: open, message, buffered-low, close, error
```

Required semantics:

- Candidate-owned objects, threads, callbacks, errors, SDP helpers, and signaling models do not cross the adapter.
- The adapter exposes explicit ownership, shutdown, callback-thread, and backpressure semantics.
- Trickle ICE, ICE restart, STUN/TURN credentials, TURN transport selection, certificate/TLS validation policy, and stats are provider-neutral inputs/outputs.
- The probe contract uses RELAY V1 signaling messages as opaque offer/answer/candidate carriers; it may not add provider-only wire fields.
- The harness has contract tests with a deterministic fake provider before any real candidate is admitted.
- Any capability absent from a candidate is reported, never silently emulated in the portable core.

**Gate 0 exit:** architecture owner approves the interface, fake-provider contract tests, lifecycle state machine, and a written mapping showing no provider types enter the portable core or V1 wire schema.

## Work breakdown: one-agent tasks

Every task has one directly responsible agent, a bounded input/output, and a review gate. Agents do not concurrently edit the same probe or report.

### T0 — Freeze protocol fixtures and acceptance rubric

**Owner:** Transport evaluation lead  
**Input:** Current V1 wire contract and Phase 1 architecture decisions  
**Output:** Versioned browser/native offer, answer, trickle-candidate, disconnect, and ICE-restart fixtures; scoring sheet; exact test environment manifest.  
**Exit:** Architecture owner confirms the fixtures do not alter V1.

### T1 — Define adapter contract and deterministic fake

**Owner:** Adapter owner  
**Input:** T0 fixtures  
**Output:** Interface specification, lifecycle diagram, thread/ownership rules, fake provider, contract-test inventory.  
**Exit:** Gate 0 passes before T2–T4 begin.

### T2 — libdatachannel probe

**Owner:** libdatachannel probe agent  
**Input:** Approved adapter and harness protocol  
**Output:** Thin disposable adapter, reproducible build manifest, capability/gap report, raw harness results, packaging/license inventory.  
**Exit:** Builds from a pinned upstream revision and completes the mandatory suite or records a reproducible blocker.

### T3 — Shiguredo probe

**Owner:** Shiguredo probe agent  
**Input:** Approved adapter plus validation of the exact maintained artifact  
**Output:** Same evidence bundle as T2, plus a fit assessment separating embeddable peer-connection capability from Sora-specific service/client behavior.  
**Exit:** Artifact and license gate pass; otherwise record an early rejection without building a product integration.

### T4 — webrtc-rs probe

**Owner:** webrtc-rs probe agent  
**Input:** Approved adapter and harness protocol  
**Output:** Same evidence bundle as T2, including Rust/C ABI boundary and runtime/executor implications for native hosts.  
**Exit:** Same as T2.

### T5 — Browser interoperability harness

**Owner:** Harness owner  
**Input:** T0 fixtures and T1 contract  
**Output:** Automated Chromium and Firefox test peer, signaling fixture driver, impairment controls, timestamped event log, packet capture hooks, and machine-readable results. Safari is a required manually run target on supported macOS hardware unless reliable automation is available.  
**Exit:** The fake provider proves the harness detects expected pass/fail outcomes; each candidate runs against identical browser builds and infrastructure.

### T6 — Independent evidence normalization and scoring

**Owner:** Evaluation reviewer (not a probe owner)  
**Input:** T2–T5 raw evidence  
**Output:** Normalized scorecard, hard-gate results, uncertainty register, recommendation, and sensitivity analysis.  
**Exit:** Architecture and release owners sign the evidence rather than relying on probe-owner impressions.

### T7 — Decision record

**Owner:** Architecture owner  
**Input:** T6 report  
**Output:** ADR selecting a candidate, extending the bake-off, or rejecting all three; includes pinning/update policy and rollback conditions.  
**Exit:** Decision explicitly preserves the adapter boundary and V1 wire contract.

## Reproducible harness

Pin and record:

- Candidate repository URL, immutable revision/release, submodules, feature flags, toolchain, build image, and transitive native dependencies.
- OS/architecture matrix: Windows x86_64, macOS arm64/x86_64 as supported, and Linux x86_64 CI reference.
- Chromium, Firefox, and Safari versions; browser flags and certificates.
- Coturn version/configuration; STUN/TURN URI, realm, credential mode, relay port range, and TLS certificate chain.
- Network impairment profiles and random seeds.
- Test start/end timestamps, state transitions, selected ICE pair, candidate types/protocol, DTLS/SCTP state, messages/bytes, reconnect time, errors, process RSS/CPU, binary size, and crash/sanitizer output.

Use isolated local/CI infrastructure rather than public STUN/TURN services. Collect a JSON result and human-readable log for every test. Retry only according to a predeclared policy; report first-attempt success and retry success separately.

## Mandatory test matrix

### Baseline and browser interoperability

For every candidate against pinned Chromium and Firefox, plus Safari on macOS:

1. Native offer → browser answer, trickle ICE, reliable ordered data channel, bidirectional binary and UTF-8 payloads.
2. Browser offer → native answer with the same data-channel checks.
3. Non-trickle/end-of-candidates handling.
4. Payload boundaries: empty, small, 64 KiB-class, and negotiated maximum-safe messages; verify explicit rejection/backpressure rather than truncation.
5. Repeated create/connect/send/close cycles and simultaneous close.
6. SDP/candidate fixture replay through the unchanged V1 signaling envelope.

### TURN, TLS, and topology

Run forced-relay tests so success cannot fall back to a direct path. Assert the selected pair is relay/relay (or otherwise demonstrably TURN-routed).

- TURN/UDP.
- TURN/TCP.
- TURN over TLS (`turns:`) with a valid hostname and trusted certificate.
- Invalid/expired/untrusted certificate: connection must fail closed with a diagnosable error; no insecure bypass in the shipping configuration.
- Wrong credentials and stale/rotated credentials.
- IPv4-only, IPv6 where supported, symmetric-NAT-like mapping/filtering, blocked UDP, and constrained port range.
- Credential redaction in logs and disposal/refresh behavior for time-limited TURN credentials.

Record whether each provider supports the required TURN transports natively and consistently across target platforms; do not award credit for unshippable local patches.

### Disconnect, reconnect, and ICE restart

Use explicit fault injection rather than only closing the peer:

- Short packet loss/interruption below the declared disconnect threshold.
- Network interface/path change.
- TURN server interruption and restoration.
- Signaling WebSocket interruption while the established peer remains alive.
- Remote browser crash/close and native process shutdown.
- ICE restart initiated from browser and native sides using the existing V1 negotiation messages.
- Repeated restart attempts with stale and out-of-order candidates.

For each case assert state transitions, bounded timeout, absence of duplicate channels/messages, deterministic cleanup, and whether recovery occurs without recreating the application session. “Reconnect” must be labeled precisely as ICE restart, new peer connection, or signaling reconnection; the harness must not collapse them into one result.

### Reliability, resource, and diagnostics

- 30-minute soak plus a repeated-connect stress run.
- Loss/latency/jitter profiles with deterministic seeds.
- Backpressure under a slow receiver and bounded memory behavior.
- Clean cancellation at every lifecycle state.
- Thread/leak/sanitizer checks where the platform supports them.
- Actionable errors and sufficient standardized stats to diagnose the selected path and failures.

## Gates and scoring

### Hard gates (pass/fail before weighted scoring)

1. **Adapter fit:** implements the approved semantics without leaking provider types or changing V1.
2. **Browser interop:** mandatory offerer/answerer and bidirectional data-channel cases pass on Chromium and Firefox; Safari result is known and no unexplained protocol incompatibility remains.
3. **Relay/security:** forced TURN/UDP and at least one TCP/TLS fallback path pass; TLS validation fails closed.
4. **Recovery/lifecycle:** required teardown and ICE-restart behavior is deterministic, with no critical leak/crash.
5. **Licensing:** license and all shipped transitive/native dependencies are compatible with RELAY distribution; notices/source obligations are documented and approved.
6. **Packaging:** reproducible pinned builds and a credible packaging path exist for required targets; no runtime dependency on an undeclared hosted service.
7. **Maintenance:** upstream activity, release/update process, security-reporting path, and ownership risk are documented.

A hard-gate failure rejects the candidate unless the architecture owner records a narrowly scoped, time-boxed exception before scores are opened.

### Weighted score (100 points after gates)

| Dimension | Weight | Evidence |
|---|---:|---|
| Browser/TURN interoperability and correctness | 25 | Mandatory matrix pass rate, protocol traces |
| Recovery, lifecycle, and backpressure semantics | 20 | Fault-injection results, soak/stress evidence |
| Cross-platform build and packaging | 15 | Clean pinned builds, artifact/dependency inventory |
| Adapter fit and integration complexity | 15 | LOC only as context; semantic shims, unsafe/FFI surface, runtime/thread impact |
| Security and diagnostics | 10 | TLS behavior, credential handling, stats/errors, security posture |
| Maintenance and upstream health | 10 | Releases, responsiveness, bus factor indicators, update burden |
| License/compliance burden | 5 | Approved license report and obligations |

Score each item 0–5 using written anchors: 0 unsupported, 1 blocker-heavy, 2 major gaps, 3 meets requirement, 4 exceeds with minor risk, 5 strong evidence and low risk. Multiply normalized ratings by weights. Report raw measurements and confidence beside every rating. A total score does not override a hard gate.

## Licensing and packaging gate procedure

For each exact pinned build:

- Identify the top-level license with SPDX expression and verify repository license files.
- Generate a transitive dependency and linked-binary inventory, including optional codec/media, TLS, ICE, SCTP, and platform libraries actually enabled.
- Classify static/dynamic linking, attribution, source-offer/reciprocity, patent, trademark, and redistribution obligations; obtain human legal approval rather than treating this plan as legal advice.
- Confirm whether optional features can be disabled to reduce obligations without invalidating the probe.
- Produce clean build steps and distributable artifact layout for every required OS/architecture.
- Record binary size, runtime DLL/framework/shared-library requirements, code-sign/notarization implications, Rust/C++ runtime requirements, and symbol/debug strategy.
- Verify upstream artifacts or source builds can be pinned and reproduced; mutable branches and unversioned downloads fail.
- Document CVE/advisory monitoring, upgrade cadence, and who owns emergency patching.

## Ownership

| Area | Accountable owner | Responsibility |
|---|---|---|
| Decision and boundary | Architecture owner | Adapter/V1 invariants, exceptions, final ADR |
| Evaluation | Transport evaluation lead | Schedule, environment parity, scoring rubric |
| Adapter contract | Adapter owner | Contract, fake, lifecycle/thread semantics |
| Candidate probes | One named owner per candidate | Candidate-only disposable adapter and evidence |
| Harness | Harness owner | Browser automation, TURN/TLS lab, impairment, result schema |
| Security | Security reviewer | TLS/credential/failure-mode review |
| Licensing | Legal/compliance owner | License determination and redistribution approval |
| Packaging | Release engineering owner | Target matrix, reproducibility, signing/notarization impact |
| Evidence review | Independent evaluation reviewer | Normalize results and challenge unsupported scores |

No ownership row may be satisfied by “the team.” Names are assigned before work starts. Probe owners may not self-approve gates or final scores.

## Non-goals

- Selecting or validating Truce or any plugin-shell technology.
- Replacing the portable core or letting WebRTC abstractions become the domain model.
- Changing RELAY V1 signaling/wire messages to accommodate a provider.
- Building production UI, discovery, identity, rooms, persistence, telemetry service, or deployment control plane.
- Evaluating media codecs, capture/render pipelines, SFU scale, simulcast, or production Sora service integration unless separately approved.
- Benchmarking public Internet services whose configuration is not controlled.
- Optimizing a candidate before it passes correctness and packaging gates.
- Shipping any probe adapter; probes are disposable evidence and a production implementation requires a separate plan.

## Deliverables and decision rule

The phase ends with: approved adapter specification; pinned harness and fixtures; three evidence bundles (including early-rejection reports); normalized scorecard; license and packaging reviews; uncertainty register; and an ADR.

Select the highest-scoring candidate only if it passes every hard gate and leads by enough to remain preferred under reasonable scoring-weight sensitivity. Otherwise run a narrowly specified follow-up or reject all candidates. Never use schedule sunk cost, probe code volume, or an average score to waive browser, relay/TLS, licensing, packaging, or boundary failures.
