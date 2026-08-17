# `relay-transport` Gate-0 review fixes

**Status:** implementation and focused validation complete; independent re-review pending  
**Scope:** dispositions for `review-relay-transport-gate0.md`; this does not close Gate 0 or select a provider

## Dispositions

| Finding | Disposition | Evidence |
|---|---|---|
| C1 — rejected ownership | **Fixed.** `submit` returns `SubmitError` containing the exact rejected `Command`; `into_parts` returns ownership for retry without a clone. | `command_queue_full_transfers_nothing_and_allows_retry` retries the returned value and drains through its exact `OperationCompleted(2)` terminal. |
| C2 — retained allocation/configured caps | **Fixed for T1a-owned values.** Constructors normalize retained strings to length-sized allocations. Caller-supplied SDP, candidate, and end-marker commands are checked against configured caps before admission and returned intact, with no terminal. Provider-created output can still produce a correlated post-admission capacity failure. | Unit allocation tests plus `configured_input_caps_return_commands_without_admission_or_terminal_events` and `configured_text_caps_fail_accepted_operations_with_stable_errors`. |
| H1 — portable lifecycle vocabulary | **Fixed at the seam.** `Connecting`, `Disconnected`, and `Failed` plus stable fatal-provider reporting are present without changing V1. Real provider transition evidence remains T1b. | Public `PeerState`, `TransportError::ProviderFailure`, and `Event::FatalError` contract. |
| H2 — waker obligation | **Fixed.** The latest-poll waker replacement/re-poll guarantee is documented and tested with counting wakers. | `pending_replaces_the_registered_waker_and_submit_wakes_only_the_latest`. |
| H3 — same-epoch conflicts | **Fixed.** Exact same-epoch replay is idempotent; a different retained description fails with `ConflictingDescription`. Role/kind and glare paths are explicit. | `same_epoch_description_replay_is_idempotent_but_conflicts_are_stable` and `remote_description_kind_follows_role_and_offerer_rejects_glare`. |
| H4 — fixture replay/map | **Fixed for the T1a public seam.** Tests decode the checked-in SHA-frozen V1 binaries with `prost::Message`, construct bounded transport values, and compare every transport-relevant payload field without hand-written SDP/candidate substitutes. Baseline and newer-epoch flows drive both roles. Outer envelope routing/replay identity remains the signaling adapter's responsibility and is deliberately not folded into `NegotiationEpoch`. | `frozen_v1_payloads_map_every_transport_field_into_bounded_commands`, both baseline tests, and both newer-epoch restart tests. |
| M1 — end-marker shape | **Fixed.** `EndOfCandidates` preserves `sdp_mid`, m-line index, and username fragment from the canonical empty-candidate V1 carrier under the same aggregate bound. | Fixture map plus `end_marker_is_bounded_and_preadmission_return_preserves_every_field`. |
| M2 — operation-ID exhaustion | **Fixed.** `u64::MAX` is reserved for shutdown and rejected for non-shutdown work before admission. | `maximum_operation_id_is_reserved_for_orderly_shutdown`. |
| M3 — weak/non-vacuous tests | **Fixed for implemented T1a invariants.** The returned queue-full command drains through its exact terminal; the maximum five-event batch runs at `event_capacity == 5`; caller-cap rejection proves no admission/no terminal; shutdown asserts the exact `ShuttingDown -> OperationCompleted(shutdown) -> Shutdown -> ShutdownComplete -> None` suffix. | Focused contract suite (18 tests). Dropped-driver, real overflow/fatal injection, timeout, capability-gap, and send backpressure remain T1b because T1a does not implement those behaviors. |

## Fixture-map boundary

The fixture test dependency is dev-only and exact-path scoped:

```toml
[dev-dependencies]
relay-protocol = { version = "=0.1.0", path = "../relay-protocol" }
prost.workspace = true
```

No production dependency, V1 schema, or generated protocol source changed. The mapping is intentionally payload-only:

- `Offer.sdp` / `Answer.sdp` map byte-for-byte as UTF-8 text into bounded `SessionDescription` with the matching kind;
- non-terminal `IceCandidate` maps candidate, optional mid, checked `u32 -> u16` m-line index, and optional username fragment into bounded `IceCandidate`;
- canonical `candidate == "" && end_of_candidates` maps all optional carrier fields into bounded `EndOfCandidates`;
- the locally assigned negotiation epoch is not inferred from `Envelope.revision`.

The eight description fixtures (baseline plus both ICE-restart directions) and four trickle/end fixtures are decoded directly from `tests/fixtures/transport/v1`. This proves unchanged carrier mapping, not live browser connectivity or provider SDP equivalence.

## Gate boundary after fixes

These fixes strengthen the provider-neutral negotiation slice; they do **not** satisfy the full selected Gate-0 matrix. T1b still needs data-channel/send semantics and byte backpressure, validated ICE/TURN/TLS configuration and capabilities, stats, real provider/callback overflow adaptation, injectable fatal failure, drop/teardown timeout behavior, and candidate adapters/live interoperability evidence. Gate 0 therefore remains open.

## Exact validation

All commands ran from the repository root with the checked-in lockfile:

| Command | Result |
|---|---|
| `(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)` | pass; all 15 frozen fixtures unchanged |
| `cargo tree --locked -p relay-transport --edges normal` | pass; no normal dependency |
| `cargo tree --locked -p relay-transport --edges dev` | pass; exact `relay-protocol` path and pinned workspace `prost` only |
| `cargo fmt --all -- --check` | pass |
| `cargo check --locked -p relay-transport --all-targets --all-features` | pass |
| `cargo check --locked --release -p relay-transport --all-targets --all-features` | pass |
| `cargo test --locked -p relay-transport --all-targets --all-features` | pass; 1 unit + 18 integration tests |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | pass; 1 unit + 18 integration tests |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | pass |
| `cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings` | pass |
| `cargo deny check` | pass for advisories, bans, licenses, and sources |

`cargo deny check` retained the three existing `license-not-encountered`
warnings for BSD-2-Clause, BSD-3-Clause, and ISC allow-list entries.

**Independent re-review: pending.** This implementation report does not mark
Gate 0 closed.
