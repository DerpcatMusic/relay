# RELAY signaling protocol V1

**Status:** Phase-0 contract draft  
**Schema:** `proto/relay/v1/signaling.proto`

This document defines validation and security behavior that Protocol Buffers cannot encode by itself. The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative for V1 implementations.

## Transport and identity

- Production signaling MUST use HTTPS/WSS with certificate validation. Plaintext is allowed only on explicitly configured loopback development endpoints.
- Before accepting `Hello`, the server MUST bind the socket to a session and peer using a short-lived, single-purpose join ticket or a valid resume token.
- `Envelope.session_id` and `Envelope.peer_id` are routing assertions, not authentication. The server MUST reject them when they differ from the socket binding.
- A sender MUST be authorized for every `target_peer_id` before a message is relayed.
- Join tickets and resume tokens are bearer secrets. They MUST be short-lived or rotated, stored with least privilege, compared without leaking timing information where practical, and redacted from logs and telemetry.
- Full SDP and raw ICE candidates MUST NOT appear in ordinary logs or exported telemetry.

## Version bootstrap

The Protobuf package name fixes the wire major: `relay.v1` means `ProtocolVersion.major == 1`.

1. A pre-negotiation `Hello` envelope uses major 1 and the sender's highest implemented minor.
2. `Hello.supported_versions` lists supported `{major, minor}` pairs, most preferred first.
3. The server selects the highest mutually supported V1 minor and returns it in `Welcome.selected_version` and the envelope version.
4. Peers MUST reject major zero, a major other than 1, an empty supported-version list, or a selected version they did not advertise.
5. Minor changes are additive only. Removed or semantically incompatible behavior requires a new major/package.

## Initial join and resume

`Hello.entry` MUST contain exactly one of:

- `InitialJoin`, after authentication with a join ticket outside the envelope; or
- `ResumeRequest`, containing the previously issued secret and the last fully applied revision.

On success the server returns a rotated `Welcome.resume_token` and `current_revision`. For resume it also returns exactly one recovery result:

- `ResumeAccepted`: events strictly ordered by increasing revision, beginning at `last_seen_revision + 1`; or
- `FullRenegotiationRequired`: the replay window is unavailable or state cannot be reconstructed safely.

Clients MUST apply replay events idempotently by revision. A lower/equal revision is a duplicate; a gap aborts replay and triggers full renegotiation. Resume-token rotation and state publication MUST be atomic from the session's point of view.

## Capability intersection

V1 media is Opus with a 48,000 Hz RTP timestamp clock. The clock is not negotiated.

- `opus_frame_durations_us` values MUST come from the V1 product set `{5000, 10000, 20000}`.
- `opus_channel_counts` MUST contain supported values from `{1, 2}`; the V1 product profile selects stereo (`2`) unless a later policy explicitly permits mono media.
- `max_opus_bitrate_bps` MUST be nonzero and is an upper bound, not a requested operating bitrate.
- DTX MUST negotiate to false for the V1 product profile.
- In-band FEC, TURN-TLS, ICE restart, and maximum audio tracks are typed capabilities. Unknown behavior MUST NOT be inferred from free-form strings.
- Selection is the deterministic intersection of both peers and server route policy. No common V1 profile is a typed incompatibility error.

## ICE candidate mapping

For a non-empty candidate, at least one of `sdp_mid` or `sdp_mline_index` MUST be present. `sdp_mline_index` MUST fit an unsigned 16-bit value. `username_fragment`, when present, identifies the ICE generation.

End-of-candidates has one canonical representation: `end_of_candidates == true` with an empty `candidate`. A message MUST NOT combine a non-empty candidate with `end_of_candidates == true`.

## Input limits

Receivers MUST reject input before unbounded allocation or fan-out. Initial V1 ceilings are:

| Item | Maximum |
|---|---:|
| serialized envelope | 262,144 bytes |
| opaque ID | 128 UTF-8 bytes |
| client name/version | 128 UTF-8 bytes each |
| SDP | 131,072 UTF-8 bytes |
| ICE candidate | 4,096 UTF-8 bytes |
| error message | 512 UTF-8 bytes |
| capability list entries | 16 per repeated field |
| replay events per welcome | 256 |

Servers SHOULD choose stricter role-specific limits and rate limits where interoperability permits. Error text returned to peers MUST be sanitized and MUST NOT echo credentials, SDP, candidates, stack traces, or provider errors.

## Telemetry boundary

Local diagnostics may retain rich in-memory session statistics. Exported `TelemetryEvent` identifiers MUST be short-lived, export-specific pseudonyms; public share/session IDs, peer IDs, trace data containing user identifiers, and all bearer secrets are prohibited. Retention, sampling, and consent policy are control-plane responsibilities and must be defined before telemetry export ships.

## Required validation

Before a V1 compatibility baseline is published, tests must cover:

- unsupported/downgraded versions;
- expired, replayed, and rotated resume credentials;
- ordered replay, duplicate revisions, gaps, and full renegotiation;
- identity/target authorization mismatches;
- every input limit and cross-field ICE invariant;
- typed capability intersection with no common profile;
- automated redaction fixtures;
- `buf lint`, `buf build`, format, and breaking comparison against the committed baseline.
