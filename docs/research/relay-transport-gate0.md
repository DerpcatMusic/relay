# `relay-transport` Gate-0 T1a negotiation slice

**Status:** revised and locally validated; full Gate 0 remains open pending T1b  
**Scope:** provider-neutral bounded negotiation only; no provider selected

## Design

`relay-transport` exposes one deep, object-safe boundary:

- `NativeTransportProvider` constructs a peer only from `ValidatedPeerConfig`.
- `PeerDriver::submit` transfers a whole command into a bounded queue or returns
  the exact rejected command inside `SubmitError`. `PeerDriver::poll_event` uses
  only `core::task::Context`, so the portable API does not select an async runtime.
- The driver is single-owner (`&mut self`). Native callbacks, threads, runtimes,
  SDP helpers, and provider objects cannot cross the seam.
- `FakeNativeTransportProvider` implements the same seam without network access
  and advances deterministically only when submitted or polled.

The T1a commands are `CreateOffer`, `CreateAnswer`,
`SetLocalDescription`, `SetRemoteDescription`, `AddRemoteCandidate`,
`EndRemoteCandidates`, and `Shutdown`. Events are correlated terminal operation
results, local description/candidate/end progress, explicit state changes, and
`ShutdownComplete`.

## Bounds and validation

`PeerConfig::validate` rejects zero, unserviceable, or excessive capacities
before provider allocation. Command and event capacities are capped at 4096.
SDP is absolutely capped at 64 KiB and candidate/end-marker text at 4 KiB;
each validated configuration may lower those limits. Constructors normalize
retained strings to their text length. Caller-owned values above a configured
cap are rejected before admission and returned intact with no terminal event.
A provider-created output that exceeds a configured cap fails its already
accepted operation with a correlated `OperationFailed`. The fake requires event
capacity of at least five, the largest ordered progress batch from one T1a
command.

Operation IDs must be strictly increasing. A scalar high-water mark therefore
provides bounded duplicate/out-of-order detection instead of retaining an
unbounded set of historical IDs. `u64::MAX` is reserved for orderly shutdown.
`QueueFull`, `DuplicateOperation`, capacity errors, epoch errors, invalid-state
errors, and shutdown use stable public enum variants and messages.

## Lifecycle and invariants

The T1a fake's successful negotiation path is:

```text
New -> Negotiating -> Connected -> ShuttingDown -> Shutdown
          ^               |
          +---------------+  (newer offer epoch)
```

The portable seam also names `Connecting`, recoverable `Disconnected`, and
terminal provider `Failed` states. Real-provider transition, fatal-failure, and
recovery evidence remains T1b work rather than an implemented fake behavior.

The T1a implementation enforces these invariants:

1. Every successfully submitted operation produces exactly one and only one
   `OperationCompleted` or `OperationFailed` event.
2. Progress events precede the correlated terminal event.
3. An answerer installs the matching remote offer before `CreateAnswer`.
4. A remote candidate or end marker is accepted by the pump but fails with
   `StaleEpoch` unless a remote description for that exact epoch is installed.
5. A newer answerer-side remote offer or offerer-side `CreateOffer` replaces the
   active epoch; older description/candidate/end input cannot be applied.
6. Accepting `Shutdown` closes submission immediately but drains commands that
   were accepted earlier. `ShuttingDown` precedes its operation terminal, then
   `Shutdown` and the final `ShutdownComplete` marker follow. Thereafter polling
   permanently returns `Ready(None)` and submission returns
   `TransportError::Shutdown`; no later event is possible.
7. No queue, string, runtime handle, provider object, or wire-schema extension
   is implicit or unbounded.

Public-seam tests decode the eight SHA-frozen baseline/restart offer/answer
binaries and four trickle/end binaries through `prost::Message`. They map every
transport-relevant V1 payload field into bounded descriptions, candidates,
end markers, and commands, then drive both roles and newer epochs. The fake's
locally generated candidate and end-marker fields are compared exactly with the
native fixtures. Its deterministic generated SDP is opaque fake output and is
not claimed to be byte-identical to provider- or browser-created SDP. Outer V1
envelope routing/replay identity remains outside the transport value, and an
envelope revision is not reinterpreted as a negotiation epoch.

## Contract-test evidence

The public-seam suite now contains 18 integration tests plus one internal
allocation test. The H4 tests decode the checked-in V1 binaries themselves;
they do not duplicate fixture SDP or candidate strings. The remaining M3
strengthening makes these paths non-vacuous:

- a `QueueFull` rejection returns the exact command, whose retry drains through
  its exact operation terminal;
- the largest five-event provider batch is exercised with
  `event_capacity == 5` and leaves no overflow residue;
- configured caller-input cap failures return the original values before
  admission and produce no terminal;
- shutdown ends in the exact ordered suffix `ShuttingDown`, shutdown
  `OperationCompleted`, `Shutdown`, `ShutdownComplete`, then permanent `None`.

Both offerer and answerer baseline paths use frozen descriptions, trickle, and
end markers. Both newer-epoch paths use the corresponding frozen ICE-restart
offer/answer pairs and reject baseline-epoch descriptions, candidates, and end
markers after the restart.

## Limitations and pending T1b work

T1a is intentionally not a usable WebRTC transport and makes no provider
selection. The following remain explicitly pending for T1b or later gates:

- data-channel creation, payload transfer, inbound bounds, capacity events, and
  all-or-none send backpressure;
- STUN/TURN configuration, TURN transport/TLS policy, credentials, relay
  security, provider capabilities, and statistics;
- real callback/event overflow adaptation and injectable provider-fatal paths;
- dropped-driver ownership, teardown timeout behavior, browser
  interoperability, and real native-provider adapters;
- any explicit provider restart-control command beyond the current portable
  newer-epoch description flow.

The presence of portable connection/failure vocabulary is not evidence for
those real-provider behaviors. The negotiation epoch is an internal portable
correlation tag. V1 still carries restart generations only in opaque SDP and
ICE username fragments; this slice does not add a wire field, infer an epoch
from replay revision, or reinterpret `PeerUpdate(LEFT)` as transport
disconnection. Full Gate 0 remains open pending that T1b evidence.

## Validation

All commands ran from the repository root with the checked-in lockfile:

```text
(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)
cargo tree --locked -p relay-transport --edges normal
cargo tree --locked -p relay-transport --edges dev
cargo fmt --all -- --check
cargo check --locked -p relay-transport --all-targets --all-features
cargo check --locked --release -p relay-transport --all-targets --all-features
cargo test --locked -p relay-transport --all-targets --all-features
cargo test --locked --release -p relay-transport --all-targets --all-features
cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings
cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings
cargo deny check
```

Results: all 15 fixture checksums, dev-only dependency-tree inspection, format,
debug/release checks, debug/release tests, and debug/release strict Clippy all
passed. Each test profile passed 1 unit test and 18 integration tests. The
normal dependency tree remains empty; only the dev tree contains `prost` and
`relay-protocol`. `cargo deny check` passed advisories, bans, licenses, and
sources; it retained the three existing `license-not-encountered` warnings for
BSD-2-Clause, BSD-3-Clause, and ISC allow-list entries.
