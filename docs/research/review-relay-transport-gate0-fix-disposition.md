# Relay Transport Gate 0 Fix Disposition

**Scope:** Independent read-only re-review of every C1–C2, H1–H4, and M1–M3 finding in `review-relay-transport-gate0.md` against the current `relay-transport` implementation, its 18-test suite, original/fix evidence, and frozen fixtures. No source file was edited.

## Verdict

**PASS for the T1a fix set.** Every reported C/H/M finding is correctly disposed for the implemented provider-neutral negotiation slice.

**Residual findings in that reviewed slice: 0 critical, 0 high, 0 medium.**

This is **not** a full Gate-0 pass. T1a is accepted as a bounded negotiation-contract slice; Gate 0 remains open until T1b supplies the deliberately deferred send/data-channel backpressure, complete validated ICE/TURN/TLS configuration and capabilities, statistics, real callback overflow and fatal injection, drop/timeout teardown, provider adapters, and live interoperability evidence.

Evidence was cross-checked among the original `docs/research/review-relay-transport-gate0.md`, the implementer disposition `docs/research/review-relay-transport-gate0-fixes.md`, the revised `docs/research/relay-transport-gate0.md`, the selected interface contract and T0 fixture rubric, the current crate source/tests/manifests, and the checked-in binary corpus. The implementer disposition was treated as a claim to verify, not as proof.

## Finding-by-finding disposition

| Finding | Result | Independent evidence |
|---|---|---|
| C1 — rejected ownership | **PASS / fixed.** | `PeerDriver::submit` returns `Result<(), SubmitError>` and `SubmitError::into_parts` returns the owned `Command` (`crates/relay-transport/src/lib.rs:396-427, 573-588`). Every pre-admission branch moves the original command into `SubmitError`; admission/high-water mutation happens only afterward (`919-955`). The integration test retries the returned value and observes operation 2's terminal (`tests/fake_contract.rs:421-477`). An external temporary-crate adversarial test additionally compared the retained SDP allocation pointer before submission and after `QueueFull`, then retried the same operation ID successfully. |
| C2 — retained allocation/configured caps | **PASS / fixed for T1a-owned values.** | SDP, candidate subfields, and end-marker subfields are normalized through boxed strings after absolute aggregate checks (`lib.rs:63-75, 106-127, 158-166, 178-205`). Config validation rejects zero, excessive, and event capacities below the five-event service minimum (`504-542`). Caller descriptions/candidates/end markers are checked before high-water mutation or queue insertion (`640-660, 919-955`); provider-created fake output fails its accepted operation with a correlated stable error (`748-800`). Unit allocation evidence is at `980-1025`; public tests cover configured SDP/candidate return and all end-marker fields (`tests/fake_contract.rs:535-620, 1108-1142`). External probes verified exact allocation identity, reusable rejected operation IDs, every zero/upper configuration rejection, and all exact maxima. |
| H1 — lifecycle/fatal vocabulary | **PASS / seam fixed.** | The portable enum now includes `Connecting`, `Disconnected`, and terminal `Failed`, in addition to the T1a fake path (`lib.rs:313-338`). Stable provider failure appears as `TransportError::ProviderFailure`, and `Event::FatalError` documents `Failed`-first, shutdown-only-after-fatal behavior (`342-390, 473-484`). Real-provider transition/fatal evidence is correctly identified as T1b, not claimed by the fake. |
| H2 — poll/waker contract | **PASS / fixed.** | `PeerDriver::poll_event` specifies sole latest-waker replacement, wake-on-possible-progress, re-poll, no busy polling, and permanent `Ready(None)` semantics (`lib.rs:577-588`). The fake replaces/stores the latest waker and takes/wakes it on accepted submission (`947-954, 958-972`). A counting-waker test proves the stale waker remains at 0, latest is woken exactly once, progress is observable, and terminal `None` is permanent (`tests/fake_contract.rs:735-781`). |
| H3 — same-epoch conflict/glare/restart | **PASS / fixed.** | Role/kind checks precede epoch application. Answerer epochs reject older input, reset only on a newer epoch, retain the exact description, complete exact replay, and emit `ConflictingDescription` for a different value at the same epoch (`lib.rs:803-856`). Offerer `CreateOffer` requires a strictly newer epoch (`714-724`) and remote offer glare is an `InvalidState` role/kind failure. Tests cover equal replay/conflict on remote and local descriptions, offerer glare/wrong answerer kind, both newer-epoch directions, and stale descriptions/candidates/end markers (`tests/fake_contract.rs:783-1106`). |
| H4 — frozen-fixture map | **PASS / fixed for transport-relevant payloads.** | The test uses `include_bytes!` for all eight description and four candidate/end carriers, decodes each actual binary with `prost::Message`, checks V1, and maps SDP/kind plus every candidate/end field (`tests/fake_contract.rs:8-39, 100-212, 246-306`). Baseline and restart flows drive both roles. Independent raw protobuf decoding confirmed those exact 12 carriers; the other three frozen files are resume/peer-left signaling fixtures and correctly remain outside the transport value. Envelope routing/replay identities are not misused as negotiation epochs. |
| M1 — end-marker shape | **PASS / fixed.** | `EndOfCandidates` carries epoch, optional mid, optional m-line index, and optional username fragment under the aggregate bound (`lib.rs:97-156`); command/event variants carry this value (`283-289, 463-467`). Binary-map checks preserve all carrier fields (`tests/fake_contract.rs:168-212, 295-305`), and pre-admission rejection returns every field intact (`1108-1142`). The canonical variant itself supplies empty `candidate` and `end_of_candidates=true` on adapter remapping. |
| M2 — operation-ID exhaustion | **PASS / fixed.** | `u64::MAX` is documented/reserved and rejected for non-shutdown before high-water mutation (`lib.rs:25-31, 919-950`). The contract test accepts `MAX-1`, rejects non-shutdown `MAX`, then accepts shutdown `MAX` and observes both exact terminals plus exact shutdown suffix (`tests/fake_contract.rs:1144-1205`). An external adversarial probe also showed that rejecting non-shutdown `MAX` leaves `MAX` available for shutdown. |
| M3 — weak/vacuous contract evidence | **PASS / fixed for implemented T1a invariants.** | Queue-full retry drains through the exact terminal (`421-477`); the maximum progress batch runs at `event_capacity == 5` and leaves no residue (`622-664`); caller-cap rejection proves no admission/no terminal (`554-607, 1108-1142`); shutdown asserts the exact `ShuttingDown -> OperationCompleted -> Shutdown -> ShutdownComplete -> None` sequence while preserving prior FIFO terminals (`666-733`). The suite contains exactly 18 integration tests plus one allocation unit test. Deferred dropped-driver/real-overflow/fatal/timeout/capability/send cases are T1b blockers, not silently claimed T1a evidence. |

## Adversarial checks

A throwaway crate under `/tmp` (not a repository source edit) ran four additional tests against the public API:

1. filled the one-slot command queue, proved `QueueFull` returned the same SDP allocation pointer, then retried the unconsumed operation ID;
2. rejected an over-configured candidate, proved allocation identity and operation-ID reuse after pre-admission rejection;
3. exercised zero, above-maximum, and exact-maximum boundaries for command/event/SDP/candidate configuration;
4. rejected non-shutdown `OperationId(u64::MAX)` and then accepted shutdown with that reserved ID.

All four passed.

Code inspection plus the focused suite additionally verified latest-waker replacement/wake behavior; complete portable state/fatal vocabulary; exact replay versus equal-epoch conflict; role/kind glare; both newer generations; every stale description/candidate/end marker; field-complete end markers; exact capacity-five batching; and exact terminal shutdown.

## Independent frozen-binary decode

All 15 SHA-256 checks passed. A schema-directed raw protobuf decode (independent of the Rust test helper) found:

- eight description carriers with V1.1 envelopes and the expected `Offer`/`Answer`, target peer, opaque SDP, baseline/restart session versions, and ICE username fragments;
- browser/native trickle carriers with non-empty candidate, `sdp_mid="data"`, m-line `0`, and `browser-base-v1` / `native-base-v1` username fragment;
- browser/native completion carriers with absent/empty candidate, the same mid/index/username fields, and `end_of_candidates=true`;
- the remaining three carriers are `PeerUpdate(LEFT)`, resume request, and resume accepted, confirming why they do not map into the transport negotiation value.

The public test's eight-plus-four map therefore reads the real frozen binaries and covers every transport-relevant payload field without reinterpretation. The protocol-level corpus remains responsible for all-15 byte-identical replay.

## Dependencies

`crates/relay-transport/Cargo.toml:10` has an empty normal dependency table. `cargo tree --locked -p relay-transport --edges normal` printed only the root package. The only direct dev dependencies are exact path/version `relay-protocol = 0.1.0` and workspace-pinned `prost` (`Cargo.toml:15-17`), exactly matching `cargo tree --edges dev`. No protocol/schema/generated source was changed.

## Validation

| Command | Result |
|---|---|
| `(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)` | **PASS** — all 15 |
| `cargo tree --locked -p relay-transport --edges normal` | **PASS** — empty normal tree |
| `cargo tree --locked -p relay-transport --edges dev` | **PASS** — only direct `prost` and exact path `relay-protocol` |
| `cargo fmt -p relay-transport -- --check` | **PASS** |
| `cargo check --locked -p relay-transport --all-targets --all-features` | **PASS** |
| `cargo check --locked --release -p relay-transport --all-targets --all-features` | **PASS** |
| `cargo test --locked -p relay-transport --all-targets --all-features` | **PASS** — 1 unit + 18 integration |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | **PASS** — 1 unit + 18 integration |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | **PASS** |
| `cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings` | **PASS** |
| `cargo deny check` | **PASS** advisories/bans/licenses/sources; existing unmatched-allowance warnings for BSD-2-Clause, BSD-3-Clause, ISC |
| `cargo fmt --all -- --check` | **PASS** on final recheck. An earlier transient failure was confined to concurrently edited, out-of-scope `crates/relay-audio/tests/virtual_hours.rs`; the transport-scoped check passed throughout. |

## Gate boundary

The original report correctly warned not to admit a real provider against T1a alone. The fixes close its nine concrete findings **within T1a** without pretending to supply T1b. Accordingly:

- **T1a fix disposition:** accepted / PASS, residual C/H/M = **0/0/0**.
- **T1b and full Gate 0:** still open; no provider-selection or real-transport approval follows from this report.
