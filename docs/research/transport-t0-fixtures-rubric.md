# Transport T0: V1 signaling fixtures and acceptance rubric

**Status:** Implemented; validation evidence below  
**Decision:** No transport provider is selected or integrated  
**Fixture version:** `relay.transport.fixtures.v1`

## Scope

This Phase-2 T0 artifact freezes provider-neutral signaling examples for the
wire contract that already exists in `relay.v1`. It covers both offerer
directions, both answerer directions, trickle ICE, canonical
end-of-candidates, signaling resume, and browser/native ICE-restart exchanges.
It also freezes an exact environment-manifest template and the machine-readable
hard-gate/weighted scorecard used by later probes.

Out of scope: provider selection or integration, live network calls, candidate
ranking/selection, schema or generated-code changes, dependency upgrades, and
production adapter work. Fixture SDP, candidates, fingerprints, and bearer
values are deterministic inert test data. The SDP is a standards-shaped opaque
carrier sample, not a certificate-bound description that can establish a real
connection.

## Primary decision sources (four local sources)

No upstream provider source was consulted because T0 must not evaluate or
select a provider. These are the only primary decision sources used:

1. [`proto/relay/v1/signaling.proto`](../../proto/relay/v1/signaling.proto) —
   authoritative V1 payload and field inventory.
2. [`docs/protocols/signaling-v1.md`](../protocols/signaling-v1.md) — normative
   resume, ICE-candidate, end-of-candidates, validation, and security behavior.
3. [`docs/plans/2026-08-15-relay-transport-plan.md`](../plans/2026-08-15-relay-transport-plan.md)
   — T0 output, environment evidence, hard gates, scoring weights, and later
   mandatory transport matrix.
4. [`docs/research/protocol-golden-foundation.md`](protocol-golden-foundation.md)
   — existing deterministic cross-language fixture and regeneration policy.

Implementation fit was additionally checked against the already-generated
Rust/TypeScript consumers and their existing golden tests; those are generated
or test artifacts, not additional requirements sources.

## V1 representability decision

| Required scenario | Existing V1 carrier | Decision |
|---|---|---|
| Browser offer → native | `Envelope.offer { target_peer_id, sdp }` | Representable and frozen. SDP remains opaque to the signaling layer. |
| Native offer → browser | Same | Representable and frozen. |
| Browser/native answer | `Envelope.answer { target_peer_id, sdp }` | Representable and frozen in both directions. |
| Trickle candidate | `Envelope.ice_candidate` | Representable in both directions with non-empty `candidate`, `sdp_mid`, `sdp_mline_index`, and `username_fragment`. |
| End-of-candidates | `Envelope.ice_candidate` | Explicitly supported and frozen in both directions as empty `candidate` plus `end_of_candidates=true`. |
| Signaling resume | `Hello.resume` and `Welcome.resume_accepted` | Representable; request and accepted response are frozen with inert tokens. |
| ICE restart | Opaque `Offer`/`Answer` plus candidate `username_fragment` | Representable at the carrier level. Baseline/restart SDP pairs change the ICE username fragment and SDP session version. V1 has no typed restart transaction or generation field outside opaque SDP/candidates. |
| Transient disconnect | None | **Hard gap.** V1 cannot signal transient WebSocket, ICE, DTLS, SCTP, or peer-connection state. `PeerUpdate(LEFT)` only represents logical application-session departure and cannot be relabeled as a recoverable disconnect. |

The corpus includes `peer-left-v1.bin` to freeze the one representable
departure event and prevent a later harness from quietly treating it as the
missing transport-disconnect signal. There is intentionally no fixture named
`disconnect`.

## Frozen fixture corpus

The 15 versioned binaries live in [`tests/fixtures/transport/v1`](../../tests/fixtures/transport/v1)
and are indexed by [`SHA256SUMS`](../../tests/fixtures/transport/SHA256SUMS):

- four baseline description carriers: browser/native offer and answer;
- four trickle-completion carriers: browser/native non-empty candidate and
  browser/native end-of-candidates;
- one logical peer-left event;
- resume request and resume-accepted response;
- browser-initiated and native-initiated ICE-restart offer/answer pairs.

The generator is
[`crates/relay-protocol/examples/regenerate_transport_fixtures.rs`](../../crates/relay-protocol/examples/regenerate_transport_fixtures.rs).
It uses only the checked-in `relay-protocol` types and existing `prost`
dependency, contains no map fields or nondeterministic inputs, and writes no
generated source. Rust and TypeScript golden tests decode and byte-identically
re-encode every binary.

## Acceptance rubric frozen by T0

### Fixture and boundary acceptance

T0 is accepted only when all of the following hold:

1. `proto/relay/v1/*.proto` and both generated consumer trees remain unchanged
   after pinned Buf generation.
2. All 15 checksums pass and regeneration reproduces the same bytes.
3. Pinned Rust and TypeScript runtimes decode every fixture, assert its V1
   discriminant/semantics, and byte-identically re-encode it.
4. Candidate fixtures satisfy the V1 cross-field rule; end-of-candidates uses
   only the canonical empty-candidate form.
5. Browser- and native-initiated restart offers visibly change the opaque SDP
   ICE username fragment from their corresponding baseline offers.
6. No test makes a network request, chooses an ICE candidate, or imports a
   provider.
7. Disconnect remains an explicit representability gap rather than being
   mapped to `PeerUpdate(LEFT)`.
8. Environment and scorecard templates parse, remain provider-neutral, and
   freeze the required target set, gate identifiers, and weights.

### Later candidate hard gates

The scorecard freezes seven pass/fail gates before weighted comparison:
`adapter_fit`, `browser_interop`, `relay_security`, `recovery_lifecycle`,
`licensing`, `packaging`, and `maintenance`. A failure rejects a candidate
unless the architecture owner approved a narrow time-boxed exception before
scores were opened. A weighted total never overrides a gate.

### Later candidate weighted scoring

| Machine ID | Weight |
|---|---:|
| `browser_turn_interoperability_correctness` | 25 |
| `recovery_lifecycle_backpressure` | 20 |
| `cross_platform_build_packaging` | 15 |
| `adapter_fit_integration_complexity` | 15 |
| `security_diagnostics` | 10 |
| `maintenance_upstream_health` | 10 |
| `license_compliance_burden` | 5 |

Each rating is an integer 0–5: 0 unsupported, 1 blocker-heavy, 2 major gaps,
3 meets the requirement, 4 exceeds it with minor risk, and 5 has strong
evidence with low risk. The total is `sum((rating / 5) * weight)`, out of 100.
Every rating requires raw measurements, evidence paths, rationale, and an
explicit confidence level.

## Exact evidence templates

[`environment-manifest-v1.template.json`](../../tests/fixtures/transport/environment-manifest-v1.template.json)
requires the immutable candidate source/release, submodules, features,
transitive native dependencies, build image and toolchains, four-target matrix,
pinned browser builds/flags/certificates, coturn/TLS configuration, impairment
profiles/seeds, predeclared retry policy, UTC run bounds, and the observation
inventory demanded by the transport plan. It prohibits public STUN/TURN
services and defaults to one attempt.

[`scorecard-v1.template.json`](../../tests/fixtures/transport/scorecard-v1.template.json)
starts every gate at `not_run`, every rating at `null`, total at `null`, and
eligibility at false. This prevents an unused template from resembling passing
evidence. See the fixture README for completion rules and allowed values.

The repository has no established JSON Schema convention or validator. T0
therefore does not introduce a schema framework or dependency; the existing
Node golden structure parses the JSON and asserts the stable fields, target
matrix, gates, and 100-point weights.

## Decisions applied

1. Keep transport fixtures separate from the Phase-0 generic protocol golden;
   their lifecycle belongs to the bake-off, not schema generation.
2. Freeze both browser/native directions so later probes cannot substitute a
   single preferred offerer direction.
3. Include both end-of-candidates directions because V1 has an explicit
   canonical representation.
4. Represent ICE restart only as an opaque offer/answer credential-generation
   change. Do not add or infer a provider-specific restart field.
5. Freeze logical `PeerUpdate(LEFT)` under its true name, while recording
   transient disconnect as a hard gap.
6. Use IANA documentation addresses and conspicuously fake tokens/fingerprints;
   fixtures are offline wire carriers, not reusable live credentials.
7. Add no standalone JSON Schema because there is no project convention; test
   the exact template skeleton with the current Node test stack instead.

## Potential corrections for later phases

1. **Split “disconnect” in the transport plan.** T0 cannot create one V1
   disconnect fixture covering signaling interruption, ICE failure, remote
   close, and logical departure. T1/T5 should name those distinct lifecycle
   observations and obtain connection-state evidence from the adapter/event
   log rather than the V1 wire envelope.
2. **Do not treat opaque SDP inspection as a portable-core API.** The T0 test
   compares ICE username fragments only to prove fixture intent. A future
   adapter/provider may create fresh descriptions, but provider SDP helpers or
   a parsed SDP domain model must not leak into the portable core.
3. **Restart correlation is limited.** V1 can carry restarted descriptions and
   candidates, but it has no typed restart ID. Later harness logic must use
   ordered revisions and ICE username fragments, explicitly testing stale and
   out-of-order candidates rather than assuming a wire transaction field.
4. **Fixture replay is not browser connectivity proof.** The inert fingerprint
   and documentation IPs deliberately prevent these goldens from replacing
   T5 live interoperability evidence. They prove unchanged V1 carriage only.

No V1 schema correction is applied by T0.

## Exact validation

All commands ran from the repository root unless a subshell is shown.

```text
$ cd proto && npx --yes @bufbuild/buf@1.72.0 format --diff --exit-code
exit 0
$ cd proto && npx --yes @bufbuild/buf@1.72.0 lint
exit 0
$ cd proto && npx --yes @bufbuild/buf@1.72.0 build
exit 0
$ cd proto && npx --yes @bufbuild/buf@1.72.0 generate
exit 0
$ compare SHA-256 snapshots of crates/relay-protocol/src/generated and packages/protocol/src/generated
all 5 generated files byte-identical; no generation drift
```

```text
$ cargo run --manifest-path crates/relay-protocol/Cargo.toml \
    --example regenerate_transport_fixtures --locked
exit 0; all 15 fixture SHA-256 values unchanged
$ (cd tests/fixtures/transport && sha256sum --check SHA256SUMS)
exit 0; all 15 fixtures OK
```

```text
$ cargo fmt --all -- --check
exit 0
$ cargo clippy --locked -p relay-protocol --all-targets --all-features -- -D warnings
exit 0
$ cargo test --locked -p relay-protocol --all-targets --all-features
exit 0; existing protocol golden: 1 passed; transport fixture tests: 2 passed
```

```text
$ npx --yes pnpm@11.22.0 install --filter @relay/protocol --frozen-lockfile
exit 0; lockfile already up to date
$ npx --yes pnpm@11.22.0 --filter @relay/protocol typecheck
exit 0
$ npx --yes pnpm@11.22.0 --filter @relay/protocol test
exit 0; 5 tests passed (existing golden plus 4 transport/template tests)
```

No validation test performs a network request or candidate selection. Buf
format/lint/build/generation, Rust locked checks, web frozen-install/typecheck,
fixture regeneration, checksums, and both language golden suites all passed.
