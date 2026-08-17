# Transport V1 fixtures

This directory freezes Phase-2 T0 inputs without selecting or integrating a
transport provider. All `.bin` files are deterministic `relay.v1.Envelope`
wire fixtures produced from checked-in generated Rust types. Addresses are
IANA documentation ranges, and every credential/token is an inert test value.
No fixture may require a network connection.

## Frozen V1 scenarios

| Fixture | Frozen meaning |
|---|---|
| `v1/browser-offer-v1.bin` | Browser-to-native baseline opaque SDP offer |
| `v1/native-answer-v1.bin` | Native-to-browser baseline opaque SDP answer |
| `v1/native-offer-v1.bin` | Native-to-browser baseline opaque SDP offer |
| `v1/browser-answer-v1.bin` | Browser-to-native baseline opaque SDP answer |
| `v1/{browser,native}-trickle-candidate-v1.bin` | Non-empty trickle candidate with mid, m-line index, and ICE username fragment |
| `v1/{browser,native}-end-of-candidates-v1.bin` | Canonical V1 end-of-candidates: empty candidate plus `end_of_candidates=true` |
| `v1/peer-left-v1.bin` | Logical application-session departure only; **not** a transient transport-disconnect signal |
| `v1/resume-request-v1.bin` | Signaling resume request with the last applied revision |
| `v1/resume-accepted-v1.bin` | Accepted signaling resume with a rotated inert token |
| `v1/{browser,native}-ice-restart-offer-v1.bin` | Opaque offer whose SDP carries a new ICE username fragment and session version |
| `v1/{browser,native}-ice-restart-answer-v1.bin` | Opaque answer for the corresponding restart generation |

V1 has no payload for transient signaling or peer-connection state. Therefore
there is intentionally no fixture labeled `disconnect`: `PeerUpdate(LEFT)`
means that a peer left the application session and must not be reinterpreted as
a recoverable network interruption. See the T0 research note for the hard gap.

ICE restart is representable only through existing opaque `Offer`/`Answer`
SDP plus `IceCandidate.username_fragment`; V1 does not expose a typed restart
operation or generation ID. The baseline and restart fixtures make that
credential-generation change observable without adding a wire field.

## Regeneration and integrity

Regenerate with the existing locked Rust consumer, then refresh and verify the
checked-in SHA-256 list:

```sh
cargo run --manifest-path crates/relay-protocol/Cargo.toml   --example regenerate_transport_fixtures --locked
(cd tests/fixtures/transport && sha256sum v1/*.bin > SHA256SUMS)
(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)
```

The files contain no Protobuf maps. Rust and TypeScript tests decode every
binary and require byte-identical re-encoding under the pinned runtimes.
`SHA256SUMS` freezes the exact corpus independent of either runtime.

## Evidence templates

- `environment-manifest-v1.template.json` records the exact candidate pin,
  build/toolchain, target, browser, coturn/TLS, impairment, retry, and run
  environment required by the transport plan.
- `scorecard-v1.template.json` freezes all seven hard gates and the seven
  weighted dimensions (100 total points).

Every key in both templates is required. Empty strings and `null` values mean
**incomplete evidence**, never “not applicable.” Arrays required by an executed
case must contain the observed values; legitimately absent feature flags or
native dependencies may remain empty. Copy a template for each candidate/run;
do not edit the templates in place.

Variable-length environment entries use these exact object shapes:
`submodules` is `{path, immutableRevision}`; `transitiveNativeDependencies` is
`{name, version, source, linkage, enabledFeatures, licenseSpdx}`;
`impairmentProfiles` is `{id, latencyMs, jitterMs, lossPercent,
bandwidthKbps}`; and `randomSeeds` contains non-negative integers. Evidence
arrays contain repository-relative artifact paths. `rawMeasurements` entries
are `{name, value, unit, context}`; `value` is a JSON number, string, or boolean.

Hard-gate `status` is one of `not_run`, `pass`, or `fail`. A non-null
`exception` is `{approvedBy, approvedAtUtc, scope, expiresAtUtc}` and must be
architecture-owner approved before scores are opened. Ratings are integers
0–5: 0 unsupported, 1 blocker-heavy, 2 major gaps, 3 meets requirements,
4 exceeds with minor risk, 5 strong evidence/low risk. Confidence is
`not_assessed`, `low`, `medium`, or `high`. The weighted total is
`sum((rating / 5) * weight)` and cannot override a failed hard gate.

This repository has no established JSON Schema convention or validator, so T0
does not introduce standalone JSON schemas or a validation dependency. The
existing Node golden test parses the templates and freezes their identifiers,
gate set, target matrix, and weights.
