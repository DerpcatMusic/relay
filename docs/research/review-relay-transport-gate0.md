# `relay-transport` Gate-0 Independent Review

## Scope

Reviewed only:

- `crates/relay-transport` public API, fake, and public-seam tests;
- `docs/research/relay-transport-gate0.md`;
- the selected contract and acceptance sections of `docs/design/transport-interface-synthesis.md`;
- the T0 transport fixtures/rubric and their evidence templates;
- `docs/plans/2026-08-15-relay-transport-plan.md`.

No source file was edited. This report is the only review artifact added.

## Verdict

**Gate 0 remains open.** The implemented seam is deep, object-safe, single-owner, and runtime-neutral, and the deterministic negotiation-only fake has sound FIFO batching and terminal shutdown behavior. It is nevertheless only the documented **T1a** slice. Two claimed safety/ownership properties are false at the Rust boundary, and the selected full Gate-0 acceptance surface is explicitly deferred to T1b.

Finding count: **2 critical, 4 high, 3 medium**.

## Critical findings

### C1 — `QueueFull` cannot satisfy the advertised “transfers nothing” ownership contract

`PeerDriver::submit` consumes `Command` and returns only `Result<(), TransportError>` (`crates/relay-transport/src/lib.rs:422-430`). On any rejection, including `QueueFull`, the moved command is dropped; ownership is not returned to the caller. The implementation detects a full queue before pushing (`crates/relay-transport/src/lib.rs:715-738`), but that proves only that the *driver* retained nothing, not that the caller can retry the same owned payload.

The test hides the loss by submitting `second.clone()` and retaining the original (`crates/relay-transport/tests/fake_contract.rs:209-222`). This is not the documented whole-command/no-transfer behavior (`docs/research/relay-transport-gate0.md:10-13`) and can force an extra allocation for a large SDP/candidate. A rejected-command error must return the `Command` (or admission must reserve before an ownership commit) for all-or-none transfer to be real.

### C2 — text length limits do not bound owned allocation, and configured caps are enforced only after queue admission

`SessionDescription::new` first converts into `String`, then checks only `len()` and retains the original allocation (`crates/relay-transport/src/lib.rs:62-74`). `IceCandidate::new` likewise retains the supplied `String` values after checking only aggregate text length (`crates/relay-transport/src/lib.rs:105-129`). A short or empty `String` can have arbitrarily large capacity; moving it into either type preserves that allocation. `impl Into<String>` can also allocate an arbitrarily large temporary before the post-conversion length check. Therefore the queue-slot cap does **not** bound queue-owned bytes.

The per-peer configured limits are not checked by `submit`; the command is accepted and queued at `crates/relay-transport/src/lib.rs:715-738`, while caller SDP/candidate limits are checked only during later processing (`crates/relay-transport/src/lib.rs:612-620`, `684-693`). This is intentional in the report (`docs/research/relay-transport-gate0.md:27-33`) but contradicts its stronger “no ... string ... implicit or unbounded” invariant (`docs/research/relay-transport-gate0.md:66-67`). Validate a borrowed view against both absolute and configured limits, then normalize into a bounded allocation before admission.

## High findings

### H1 — the current state/event vocabulary freezes the T0 wire gap into the adapter seam

The only public states are `New`, `Negotiating`, `Connected`, `ShuttingDown`, and `Shutdown` (`crates/relay-transport/src/lib.rs:237-250`). There is no portable `Connecting`, `Disconnected`, `Failed`, or fatal-provider event. T0 says only that **V1 wire envelopes** cannot carry transient disconnect and directs later work to obtain connection-state evidence from the adapter/event log (`docs/research/transport-t0-fixtures-rubric.md:164-181`). Omitting those states from the adapter makes it impossible to distinguish connection attempt, recoverable disconnection, and terminal failure without changing the seam or leaking provider concepts.

This also falls short of the selected vocabulary and transition contract (`docs/design/transport-interface-synthesis.md:57-75`, `89-96`). T1a correctly admits that capabilities, fatal failures, timeout teardown, and real overflow adaptation remain for T1b (`docs/research/relay-transport-gate0.md:101-113`), so the T1a artifact must not be treated as full Gate-0 exit evidence. The Phase-2 plan itself still marks the gate open (`docs/plans/2026-08-15-relay-transport-plan.md:7-12`, `63-72`).

### H2 — the poll/waker obligation is neither specified nor tested

The trait documents ordered polling and terminal `Ready(None)`, but never requires that the registered waker be woken when a `Pending` poll can make progress (`crates/relay-transport/src/lib.rs:417-430`). The fake does store and wake one waker on `submit` (`crates/relay-transport/src/lib.rs:730-737`), yet every test uses `Waker::noop()` and immediately polls synchronously (`crates/relay-transport/tests/fake_contract.rs:19-25`, `245-247`, `338-346`). Thus the suite cannot fail if wake delivery is removed or implemented incorrectly.

This is material because the selected design says `Context` prevents busy polling and real callbacks wake the owner (`docs/design/transport-interface-synthesis.md:44-49`). The contract needs the standard re-poll guarantee, replacement-waker behavior, and a counting-waker test that proves one `Pending` poll is subsequently awakened.

### H3 — equal-epoch conflicting answerer offers are silently accepted

For answerers, only `epoch < active` is stale (`crates/relay-transport/src/lib.rs:636-643`); state is reset only when `epoch != active` (`644-650`). A different remote offer carrying the **same** epoch therefore completes successfully, records the same `(epoch, Offer)` tuple, and may leave a connected peer in `Connected` (`656-662`). Because the fake retains no description identity beyond that tuple, it cannot distinguish an idempotent replay from a conflicting same-generation offer. This defeats the epoch's purpose as the negotiation-generation identity and leaves repeated offer/glare behavior ambiguous.

The suite covers only one stale candidate (`crates/relay-transport/tests/fake_contract.rs:250-285`). It does not lock down same-epoch replay versus conflict, offerer glare, newer answerer/offerer epochs, stale descriptions, or stale end markers, despite the selected acceptance requiring both-sided restart and stale/out-of-order rejection (`docs/design/transport-interface-synthesis.md:98-106`).

### H4 — the fake suite does not actually replay or map the frozen T0 fixture corpus

The public-seam tests define hand-written SDP/candidate strings (`crates/relay-transport/tests/fake_contract.rs:8-11`) and assert selected substrings/fields (`37-68`); they never read any of the 15 SHA-frozen binaries. The answerer generation test accepts any `LocalCandidate` without asserting its T0 fields (`169-182`). Consequently fixture changes and fake changes can drift independently while all eight tests remain green.

T0 freezes exact binary carriers and byte-identical replay (`docs/research/transport-t0-fixtures-rubric.md:60-77`, `83-99`), while the Gate-0 acceptance calls for an unchanged V1 fixture map (`docs/design/transport-interface-synthesis.md:98-109`). At minimum, a fixture-driven adapter mapping test must decode the checked-in offer/answer/candidate/end carriers and prove every portable field maps without reinterpretation.

## Medium findings

### M1 — end-of-candidates does not preserve the frozen V1 candidate field shape

`IceCandidate` carries candidate text, `sdp_mid`, m-line index, and username fragment (`crates/relay-transport/src/lib.rs:95-167`), but `Command::EndRemoteCandidates` and `Event::LocalCandidatesEnded` carry only an epoch (`crates/relay-transport/src/lib.rs:207-213`, `324-328`). T0 freezes end-of-candidates in the existing `Envelope.ice_candidate` carrier rather than a new wire type (`docs/research/transport-t0-fixtures-rubric.md:44-53`, `83-94`). The current seam therefore cannot preserve any mid, m-line, or username-fragment values present on that canonical empty-candidate carrier; an adapter must reconstruct or discard them rather than map them exactly. Document a canonical derivation or carry the bounded end-marker fields.

### M2 — accepting `OperationId(u64::MAX)` prevents any later orderly shutdown

The high-water rule accepts any first/increasing `u64` and rejects every ID `<= highest` (`crates/relay-transport/src/lib.rs:25-30`, `719-725`). Once a non-shutdown operation with `u64::MAX` is accepted, no valid ID remains for `Shutdown`, and drop-time teardown is not specified in T1a. Reserve a terminal ID, reject exhaustion before acceptance, or define rollover/session reset semantics.

### M3 — several claimed invariants are implemented but weakly/non-vacuously tested

The implementation itself serializes command batches by draining existing events before processing the next command (`crates/relay-transport/src/lib.rs:741-755`), so current fake progress precedes each terminal and queued pre-shutdown work drains FIFO. However:

- the queue-full retry test never drains the retried command to its terminal (`crates/relay-transport/tests/fake_contract.rs:199-223`);
- the shutdown test checks terminal IDs and the final marker, but not the required `ShuttingDown -> shutdown terminal -> Shutdown -> ShutdownComplete` ordering (`319-369`);
- no test runs the largest five-event batch at `event_capacity == 5`;
- configured-size testing exercises provider-generated SDP, not an over-configured caller SDP/candidate accepted into the queue (`287-304`);
- no dropped-driver, event-overflow, fatal-error, timeout, or capability-gap test exists; those are selected Gate-0 cases (`docs/design/transport-interface-synthesis.md:98-109`) and are acknowledged T1b work.

## Properties verified in the current T1a slice

- **Deep/object-safe/runtime-neutral seam:** both traits are object-safe; the public driver uses only `&mut self`, `Context`, and `Poll`, with no executor/provider/callback type exposed (`crates/relay-transport/src/lib.rs:406-431`).
- **Logical queue slots:** command and fake event queues have validated slot ceilings; the current fake emits at most five events for one processed batch (`crates/relay-transport/src/lib.rs:376-390`, `465-487`, `569-610`). This does not cure C2's owned-allocation issue or define real callback overflow.
- **Accepted-operation terminals:** every current fake command path reaches exactly one `complete` or `fail`; processing is FIFO and a later command is not processed while earlier progress remains (`crates/relay-transport/src/lib.rs:496-535`, `741-752`).
- **Terminal shutdown:** admission closes when shutdown is accepted, earlier queued commands remain FIFO, `ShutdownComplete` is last, and polling then permanently returns `Ready(None)` (`crates/relay-transport/src/lib.rs:705-710`, `715-738`, `741-755`).
- **Determinism:** the fake has no network, clock, RNG, runtime, or provider dependency. Its output is fixed and poll-driven.

## T1a versus T1b boundary

T1a is a useful negotiation-contract draft, not a usable transport and not the complete selected Gate-0 interface. T1b (or an explicit re-approval narrowing Gate 0) must cover data channels and atomic sends, send/inbound byte backpressure, capacity notifications, STUN/TURN/TLS configuration, capabilities, stats, connection/disconnect/failure states, fatal errors, teardown timeout/drop behavior, real callback/event overflow, typed restart behavior, and provider adapters (`docs/research/relay-transport-gate0.md:101-118`). No real candidate should be admitted against the T1a surface alone.

## Validation run

All commands ran from the repository root:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo check --locked -p relay-transport --all-targets --all-features` | pass |
| `cargo check --locked --release -p relay-transport --all-targets --all-features` | pass |
| `cargo test --locked -p relay-transport --all-targets --all-features` | pass; 8 integration tests |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | pass; 8 integration tests |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | pass |
| `cargo deny check` | pass for advisories, bans, licenses, and sources; three existing `license-not-encountered` warnings for BSD-2-Clause, BSD-3-Clause, and ISC |
