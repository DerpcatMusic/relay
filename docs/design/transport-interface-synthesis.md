# Native transport Gate-0 interface synthesis

**Status:** selected design for implementation after T0 fixture freeze  
**Inputs:** minimal command/event, capability-trait, and explicit-state-machine designs

## Comparison

The capability-trait design is flexible, but exposes four sub-interfaces,
associated types, provider-specific validated configuration, and borrowing/order
rules to every caller. It is a shallow seam: candidate variation reappears in
the Connect engine and makes object-safe provider selection awkward.

The consuming-typestate direction prevents invalid calls at compile time, but
WebRTC has asynchronous, remote-driven, partially concurrent transitions. A
large typestate family would either explode or be bypassed by an untyped event
path. Its valuable insight is instead to make negotiation epochs and the
runtime transition table explicit.

The minimal command/event pump has the deepest interface: one ownership model
and one contract-test surface hide callback adaptation, candidate runtimes,
threads, futures, SDP helpers, queues, and shutdown. On its own it needs the
state-machine design's epochs, exact completion rule, and lifecycle invariants.

## Selected interface shape

```rust
pub trait NativeTransportProvider: Send + Sync + 'static {
    fn capabilities(&self) -> ProviderCapabilities;
    fn create_peer(
        &self,
        config: ValidatedPeerConfig,
    ) -> Result<Box<dyn PeerDriver>, TransportError>;
}

pub trait PeerDriver: Send + 'static {
    fn submit(&mut self, command: Command) -> Result<CommandAccepted, SubmitError>;
    fn poll_event(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<Event, TransportError>>;
}
```

`ValidatedPeerConfig` is constructed by a RELAY-owned validator before provider
construction. Candidate capability mismatches return a stable error rather
than provider extension bytes. `core::task::Context` is runtime-neutral and
prevents busy polling; adapters may use any implementation internally but only
wake the owning driver. The deterministic fake uses the same seam with a test
waker and virtual clock.

Every operation-bearing accepted command yields exactly one terminal
`OperationCompleted` or `OperationFailed` event. Unsolicited progress is an
ordered event. The single owner never invokes one peer concurrently. The
provider's bounded callback/event adaptation, worker threads, and native object
lifetime are hidden.

## Required value vocabulary

Commands cover create offer/answer, set local/remote description, add/end
remote candidates, ICE restart, open/close the one reliable ordered data
channel, all-or-none bounded send, stats request, and shutdown. Events cover
operation completion/failure, local descriptions/candidates/end marker, stable
signaling/ICE/connection states, channel open/message/buffered-low/close,
stats, fatal error, and shutdown complete.

Descriptions and candidates are bounded RELAY-owned values corresponding
exactly to existing V1 `Offer`, `Answer`, and `IceCandidate` fields. They are
not full protobuf envelopes and contain no provider type. The signaling adapter
alone maps them into V1 envelopes. `NegotiationEpoch` tags descriptions and
candidates; stale restart-era input is rejected deterministically.

Configuration contains only validated portable values: ICE servers, redacted
credentials, UDP/TCP/TLS transport preference, TURN TLS server name and trust
policy, queue/message/buffer limits, certificate policy, timeouts, and required
capabilities. Shipping configuration has no insecure TLS bypass.

## Backpressure and ownership

- `submit(Send)` is atomic: accepted transfers the complete bounded payload;
  `QueueFull`/`WouldBlock` transfers none.
- An accepted send means queued by the provider, never remotely delivered.
- Retry occurs only after `SendCapacity`/`BufferedAmountLow`.
- Command and event queues have construction-time bounds and explicit overflow
  errors; descriptions, candidates, stats, and messages have byte caps.
- `Shutdown` is idempotent. `ShutdownComplete` is terminal; no later event is
  legal. Native endpoints are dropped on the owning control thread only after
  completion or a reported hard-timeout teardown path.

## Runtime transition contract

The implementation validates a RELAY-owned transition table:
`New -> Negotiating -> Connecting -> Connected -> Restarting/Connecting ->
Closing -> Closed`. Remote candidates require the matching installed remote
description epoch. Simultaneous/repeated close is idempotent. Stale operation
ids, descriptions, candidates, and events are rejected or classified; they are
never silently applied.

## Gate-0 acceptance

Before a real candidate is admitted, a deterministic fake must prove:

1. offerer and answerer paths, trickle/end marker, and unchanged V1 fixture map;
2. both-sided ICE restart and stale/out-of-order epoch rejection;
3. exact-one terminal operation completion and ordered event delivery;
4. bounded send/event backpressure without truncation or ownership ambiguity;
5. simultaneous close, dropped driver, fatal error, timeout, and clean shutdown;
6. forced provider capability gaps are surfaced;
7. no candidate/runtime/thread/callback/helper type appears in public Rust docs
   or the protocol schema.

This synthesis selects the deep command/event seam. It incorporates explicit
state/epoch invariants, and rejects the layered capability-trait surface for the
portable engine.
