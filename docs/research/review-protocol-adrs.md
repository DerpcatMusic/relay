# Protocol and ADR Review

## Scope

Independent review limited to `proto/` and architecture decision records ADR 0001 through 0006. The master plan was consulted only to test consistency. None of the reviewed files was edited.

## Criteria

- Protobuf schema correctness and Buf validation
- Wire/API evolution safety and compatibility
- Security and privacy properties
- Consistency with the repository master plan
- Exact, severity-ranked findings and actionable corrections

## Validation performed

From `proto/`, using the official `@bufbuild/buf` npm package:

```text
npx --yes @bufbuild/buf@1.58.0 --version  # 1.58.0
npx --yes @bufbuild/buf@1.58.0 lint       # pass
npx --yes @bufbuild/buf@1.58.0 build      # pass
npx --yes @bufbuild/buf@1.58.0 generate   # pass
```

Generated output from the validation run was removed afterward; the reviewed files remain unchanged. `buf breaking` could not provide a meaningful compatibility result because the repository has no commits or earlier schema image against which to compare. `proto/buf.yaml:10-12` does select Buf's `FILE` breaking policy, but CI still needs to supply a real prior-release or main-branch input.

## Severity-ranked findings

### High

#### H1 — Mandatory reconnect/resume state is absent from the wire schema

**References:** `proto/relay/v1/signaling.proto:32-48`; master plan `docs/plans/2026-08-15-relay-master-plan.md:1779-1815`.

The master plan makes WebSocket reconnection mandatory and specifies `session_id`, `peer_id`, `resume_token`, and `last_revision` on reconnect, followed by `current_revision`, missing events, or a full-renegotiation response. `Hello` carries none of the resume credential/revision state, while `Welcome` carries neither current revision nor recovery outcome. The envelope's general `revision` field does not identify the client's last-applied revision and cannot carry missed events. A reconnecting implementation therefore cannot implement the documented contract without an incompatible semantic invention outside the schema.

**Potential correction:** before establishing a compatibility baseline, add an explicit initial-join versus resume request (preferably a `oneof`), with a secret resume token and last-seen revision; add an explicit resume result containing current revision, replayed events or a full-renegotiation requirement. Specify replay ordering, idempotency, token rotation/expiry, and redaction.

#### H2 — The signaling security boundary is not specified even though the schema transports session-control secrets and private network data

**References:** `docs/adr/0003-webrtc-v1-wire.md:7-10,18-22`; `proto/relay/v1/signaling.proto:8-30,50-72`; master plan `1912-1918`, `2413-2459`.

ADR 0003 requires secure WebRTC media but dismisses signaling as an application concern. The schema sends full SDP, ICE candidates, session IDs and claimed peer IDs, yet neither the scoped ADRs nor schema establish TLS/WSS, authentication/join-ticket binding, authorization of target peers, or the rule that the server derives sender identity from the authenticated connection rather than trusting `Envelope.peer_id`. Media DTLS-SRTP does not by itself authenticate the Relay signaling account/session. RFC 8827's initial-signaling model sends signaling over TLS and derives its security claim from a message being received securely from the signaling server.

**Potential correction:** add a signaling-security decision/gate stating that production signaling is TLS/WSS only; bind each socket to a validated join ticket/resume token and server-side peer/session identity; reject payload identity mismatches and unauthorized targets; never place bearer credentials in ordinary envelope IDs; require replay/rate/size defenses and automated redaction of SDP, candidates and tokens.

#### H3 — The advertised capability model cannot reliably negotiate the locked V1 Opus profile

**References:** `proto/relay/v1/capabilities.proto:5-22`; `docs/adr/0004-48khz-network-clock.md:7-21`; `docs/adr/0005-opus.md:7-21`; master plan `1875-1901` and `790-807`.

`AudioCapability` uses free-form `codec`, an untyped `parameters` map, and ambiguous `sample_rates_hz`. It omits typed bitrate bounds, in-band FEC, DTX, ICE restart, TURN-TLS and maximum audio-track capability that the master plan identifies for negotiation. In particular, negotiating arbitrary `sample_rates_hz` is easy to misread as negotiation of the RTP clock, although ADR 0004 requires a 48 kHz RTP timestamp clock for every Opus mode. Two conforming-looking clients can assign different meanings/casing/units to map keys and strings, defeating deterministic negotiation and compatibility tests.

**Potential correction:** define a typed V1 Opus capability/profile (including bitrate bounds, allowed frame durations/channel counts, FEC and DTX), typed transport/signaling features, and explicit intersection/selection rules. State that the RTP clock is always 48,000 Hz and give any codec input-rate/bandwidth field a different, precise name. Reject non-Opus V1 media rather than silently accepting an arbitrary codec string.

#### H4 — `QUIC` is exposed as a V1 endpoint transport despite the WebRTC-only V1 decision and explicit deferral of a custom QUIC protocol

**References:** `proto/relay/v1/common.proto:13-31`; `docs/adr/0003-webrtc-v1-wire.md:9-16`; `docs/adr/0006-native-transport-bakeoff.md:9-16`; master plan `4001-4024`.

`TRANSPORT_PROTOCOL_QUIC` makes QUIC a named V1 wire value without defining how it participates in WebRTC-compatible RTP/RTCP, ICE, DTLS-SRTP or the native transport bakeoff. The master plan explicitly places a custom QUIC protocol outside V1, and ADR 0006 says unresolved transport choices must not appear in the wire contract. This leaks a premature transport choice into the supposedly provider/implementation-neutral schema.

**Potential correction:** remove the QUIC enum value before the first compatibility baseline (and reserve its number/name if appropriate), or document a standards-compatible V1 meaning and conformance tests. For ICE-facing endpoints, restrict the enum to transports the selected WebRTC/ICE contract actually defines.

### Medium

#### M1 — `IceCandidate` loses WebRTC candidate presence and generation information

**References:** `proto/relay/v1/signaling.proto:64-72`; master plan `1563-1575`, `1779-1815`.

The W3C `RTCIceCandidateInit` model makes `sdpMid`, `sdpMLineIndex`, and `usernameFragment` nullable. For a non-empty candidate, `addIceCandidate()` rejects when both locator fields are null. Proto3 implicit scalar presence makes an absent `sdp_mline_index` indistinguishable from the valid index zero, while the schema also lacks `username_fragment`, which is useful to associate candidates with an ICE generation during restart. The separate `end_of_candidates` boolean also needs a canonical mapping to WebRTC's empty candidate string.

**Potential correction:** use presence-aware `optional string sdp_mid` and `optional uint32 sdp_mline_index`, add optional `username_fragment`, validate the index as an unsigned-short range, require at least one locator for non-empty candidates, and define exactly one canonical end-of-candidates representation.

#### M2 — Protocol versioning has two insufficiently related version axes

**References:** `proto/relay/v1/common.proto:5-11`; `proto/relay/v1/signaling.proto:8-15,32-48`.

The package is already `relay.v1`, while every envelope carries a negotiable `{major, minor}` version before `Hello`/`Welcome` completes negotiation. The scoped material does not define whether package `v1`, `ProtocolVersion.major`, and the schema used to decode the initial envelope must match; what zero means; or which minor-version changes are legal. A peer can syntactically claim major 2 inside a `relay.v1.Envelope` with undefined behavior.

**Potential correction:** define one bootstrap framing rule and the exact relationship between package and negotiated version. Require presence/nonzero valid values, define major/minor compatibility and downgrade behavior, and state which version appears on pre-negotiation `Hello` and error responses.

#### M3 — Unbounded strings, repetitions and maps leave denial-of-service and semantic-injection policy outside the contract

**References:** `proto/relay/v1/capabilities.proto:6-8,14-20`; `proto/relay/v1/common.proto:16-20,36-37`; `proto/relay/v1/signaling.proto:10-29,50-72,108-114`.

The schemas accept arbitrary-length SDP, ICE candidate strings, IDs, error text, repeated lists and key/value maps. Comments asking producers not to put secrets in metadata do not constrain a hostile peer, and free-form `metadata`, `parameters`, and `Error.message` can become secret/log-injection channels. Buf lint/build cannot validate runtime size, vocabulary or cross-field invariants.

**Potential correction:** publish and enforce an envelope byte limit plus per-field, collection and map limits; use allowlisted typed fields where interoperability matters; sanitize error detail; reject unknown map keys; fuzz boundary sizes; and make server-side rate limits part of the signaling security gate.

#### M4 — Exported telemetry carries raw correlation identifiers despite the plan's sanitized-export boundary

**References:** `proto/relay/v1/telemetry.proto:7-24`; `proto/relay/v1/common.proto:33-39`; master plan `2441-2523`, `3379-3414`.

`TelemetryEvent` exports raw `session_id`, `peer_id`, `trace_id`, and `span_id`. The master plan distinguishes rich local diagnostics from optional sanitized telemetry export, but the schema does not define pseudonymization, retention, consent, sampling, or identifier scope. These stable cross-event identifiers can link users/sessions even when media content is excluded.

**Potential correction:** keep raw identifiers in local diagnostics only, or define short-lived/export-specific pseudonyms and retention/consent requirements. Document which IDs may cross the analytics boundary and prohibit reuse of public share/session secrets as telemetry IDs.

### Low

#### L1 — The broad reservation ranges are safe but unnecessarily awkward for additive V1 growth

**References:** `proto/relay/v1/common.proto:10,21,30,39`; `proto/relay/v1/capabilities.proto:10,22`; `proto/relay/v1/signaling.proto:28-29` and similar message/enum reservations; `proto/relay/v1/telemetry.proto:23-24` and similar reservations.

Reserving removed identifiers is good evolution practice, but these fresh schemas reserve nearly every low tag from the outset without documenting why. Additive fields and enum values must jump to 16 or beyond, and future maintainers may incorrectly assume those numbers correspond to deleted historical fields.

**Potential correction:** retain deliberate payload tag bands where useful, but reserve only identifiers with a documented compatibility reason; add a short tag-allocation convention. Never reuse any number after release.

## Positive observations

- All protobuf files pass Buf lint and build, and both configured generators succeed.
- `proto/buf.yaml:10-12` chooses the strict `FILE` breaking category rather than a weaker package-only policy.
- Payloads use a `oneof`, enums consistently reserve zero for `UNSPECIFIED`, and existing comments explicitly prohibit secrets in IDs/metadata/telemetry.
- ADRs 0001, 0002, 0004, 0005 and 0006 broadly preserve the master plan's monorepo, portable Rust core, 48 kHz clock, Opus V1 and acceptance-gated transport decisions.
- ADR 0003 correctly requires WebRTC-compatible secure RTP/RTCP and keeps providers out of the protocol; its gap is the adjacent signaling-security contract, not the media-security choice.

## Primary sources consulted (3)

1. Protocol Buffers, **Proto3 Language Guide** — scalar default/implicit-presence behavior, reserved identifiers, oneofs and compatibility: https://protobuf.dev/programming-guides/proto3/
2. W3C, **WebRTC: Real-Time Communication in Browsers** — `RTCIceCandidateInit` nullable fields and `addIceCandidate()` validation: https://www.w3.org/TR/webrtc/
3. IETF, **RFC 8827: WebRTC Security Architecture** — TLS signaling model, media security and ICE/IP-location privacy: https://www.rfc-editor.org/rfc/rfc8827.html

## Recommended correction order

1. Specify signaling authentication/TLS/identity binding and the reconnect/resume exchange.
2. Correct the V1 capability and ICE-candidate models before creating a compatibility baseline.
3. Remove or justify the QUIC wire value.
4. Add normative bounds, validation, and telemetry-privacy rules.
5. Commit a canonical schema baseline and run `npx @bufbuild/buf breaking --against <main-or-release-input>` in CI.
