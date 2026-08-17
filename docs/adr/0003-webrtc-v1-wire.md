# ADR 0003: Use WebRTC-compatible media semantics for the V1 wire contract

## Status
Accepted for V1 interoperability; signaling, infrastructure, and transport provider choices remain provisional.

## Context
V1 needs a specified, observable wire contract for low-latency audio across changing transport implementations. Inventing RTP, RTCP, feedback, and media security behavior would add risk without differentiating the product. WebRTC endpoints use RTP/RTCP with the secure RTP profile, while its security architecture requires SRTP/SRTCP and DTLS-SRTP ([RFC 8834 §§4.1–4.3](https://www.rfc-editor.org/rfc/rfc8834.html#section-4.1); [RFC 8827 §6.5](https://www.rfc-editor.org/rfc/rfc8827.html#section-6.5)).

## Decision
Base the V1 media wire contract on WebRTC-compatible secure RTP/RTCP behavior and negotiated Opus audio. Relay-owned signaling is part of the security boundary: production signaling uses TLS/WSS, binds each socket to a validated short-lived join ticket or rotated resume token, derives peer/session identity server-side, and rejects payload identity mismatches or unauthorized targets. Truce remains a provisional plugin-shell choice; transport implementations and service providers likewise remain provisional and are not the protocol definition. Opus packetization and SDP mapping must conform to its standardized RTP payload format ([RFC 7587 §§4, 6.1](https://www.rfc-editor.org/rfc/rfc7587.html#section-4)).

## Consequences
- Mature interoperability, encryption, and feedback mechanisms are available.
- Implementations must handle WebRTC negotiation and compatibility complexity.
- Provider substitution is feasible only if Relay-owned conformance fixtures remain authoritative.
- V1 cannot introduce incompatible custom packet semantics without an explicit revision.

## Validation gates
- Two independent implementations pass offer/answer and encrypted audio interoperability tests.
- Captures demonstrate standards-compatible RTP/RTCP and negotiated Opus parameters.
- Version-skew, authenticated resume/replay, credential redaction, packet-loss, and NAT traversal scenarios pass in CI or a repeatable lab.
- Signaling rejects plaintext production connections, replayed/expired tickets, identity mismatches, oversized envelopes, and unauthorized target peers.
- The conformance suite runs without requiring Truce and applies equally to every candidate transport or provider.
