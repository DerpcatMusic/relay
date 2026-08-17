# Phase-2 `NativeTransportProvider`: Minimal Command/Event Interface

**Status:** Gate 0 design draft; no implementation and no V1 wire-schema changes.

## Decision

The portable Connect engine owns exactly one provider handle and drives it through **two public operations**: submit a typed command and poll the provider-neutral event pump. Construction is a provider factory concern and shutdown is an explicit command. No provider/runtime/thread/callback, SDP-parser, or candidate-library type crosses the seam.

```rust
pub trait NativeTransportProvider: Send + 'static {
    fn command(&mut self, command: TransportCommand) -> Result<(), SubmitError>;
    fn poll(&mut self, cx: &mut core::task::Context<'_>)
        -> core::task::Poll<Result<TransportEvent, TransportFailure>>;
}
```

Every accepted command carrying an `OperationId` produces exactly one terminal `CommandCompleted` or `CommandFailed` event. All other progress is expressed as ordered events. Commands are accepted only by the single owning driver thread/task; provider callbacks and runtime work remain behind the implementation boundary.

## Command and event sketch

```rust
#[non_exhaustive]
pub enum TransportCommand {
    CreatePeer { op: OperationId, config: PeerConfig },
    CreateOffer { op: OperationId, options: OfferOptions },
    CreateAnswer { op: OperationId },
    SetLocalDescription { op: OperationId, description: SessionDescription },
    SetRemoteDescription { op: OperationId, description: SessionDescription },
    AddRemoteCandidate { op: OperationId, candidate: IceCandidate },
    EndRemoteCandidates { op: OperationId, media: Option<MediaSectionId> },
    RestartIce { op: OperationId },
    OpenDataChannel { op: OperationId, channel: ChannelId, label: ChannelLabel },
    Send { op: OperationId, channel: ChannelId, payload: DataPayload },
    RequestStats { op: OperationId, request: StatsRequest },
    Close { op: OperationId },
}

#[non_exhaustive]
pub enum TransportEvent {
    CommandCompleted { op: OperationId, result: CommandResult },
    CommandFailed { op: OperationId, error: TransportError },
    StateChanged { state: TransportState },
    LocalCandidate { candidate: IceCandidate },
    LocalCandidatesComplete { media: Option<MediaSectionId> },
    DataChannelState { channel: ChannelId, state: DataChannelState },
    DataReceived { channel: ChannelId, payload: DataPayload },
    SendCapacity { channel: ChannelId, available: ByteCount },
    BufferedAmountLow { channel: ChannelId, buffered: ByteCount },
    Stats { op: OperationId, snapshot: StatsSnapshot },
    Closed { reason: CloseReason },
}
```

Offer/answer results are opaque V1 signaling carriers:

```rust
pub enum CommandResult {
    Unit,
    PeerCreated { capabilities: NegotiatedCapabilities },
    DescriptionCreated { description: SessionDescription },
    ChannelOpened { channel: ChannelId },
    SendAccepted { channel: ChannelId, bytes: ByteCount },
    StatsRequested,
    CloseAccepted,
}

pub struct SessionDescription {
    pub kind: DescriptionKind,
    pub signaling: V1SessionDescriptionBytes,
}
pub struct IceCandidate {
    pub signaling: V1IceCandidateBytes,
    pub media: Option<MediaSectionId>,
}
```

The byte carriers are validated, bounded owned byte/string newtypes whose internal encoding remains exactly the existing V1 wire contract. The transport interface neither parses nor rewrites their schema.

## Configuration sketch

`PeerConfig` contains portable value types only: declared capability requirements; ICE server URIs and redacted credentials; transport preference (`Udp`, `Tcp`, or provider-neutral ordered preference); TURN TLS server-name, certificate-policy/trust inputs, and explicit validation mode; bounded queue, message, signaling-carrier, and buffered-amount limits; and deterministic clock/randomness hooks represented by portable traits owned by the harness/driver. Provider capability discovery is returned during peer creation (including unsupported requirements) rather than leaked as provider enums.

Reliable ordered data channels are the only Gate-0 channel mode. `Send` is never an unbounded enqueue: submission can fail synchronously with `SubmitError::QueueFull`; completion can report `WouldBlock { available }`; and the driver retries only after `SendCapacity`/`BufferedAmountLow`. A successful `SendAccepted` means the provider accepted exactly the reported bytes into its bounded send buffer, not that the peer received them.

## Lifecycle and ownership

The boxed provider is constructed off the portable seam by a factory selected by composition root. Once returned, it is uniquely owned and driven from one thread/task. `&mut self` makes command/poll serialization explicit. `Close` is idempotent; after it is accepted, no new work is accepted, already accepted operations terminate, one final `Closed` event is emitted, and subsequent polls remain terminated. `Drop` is a bounded emergency release only and must not wait for network/runtime threads; normal shutdown must drain through `Closed`. Implementations own, join, or detach internal threads according to a documented finite shutdown bound and may not invoke engine callbacks.

## Deterministic fake

The fake implements the same trait with a scripted command/event transcript, virtual time, injected entropy, configurable capacities, and fault points. It checks exact command ordering and operation correlation, never sleeps or spawns threads, and advances only when polled or when the harness advances its virtual dependencies. This makes the interface itself the browser-interoperability test surface.

## Initial invariants and errors

* Operation IDs are unique until their terminal event; duplicates are rejected.
* Event order is causal and stable; a terminal event never precedes its command acceptance.
* Local trickle candidates follow successful local-description establishment; completion occurs at most once per ICE generation/media scope.
* Remote candidates are generation-scoped; candidates after end-of-candidates fail unless an ICE restart begins a new generation.
* ICE restart creates a distinct generation and does not silently discard unrelated accepted operations.
* Data-channel `Open` precedes send/receive; channel IDs and labels are bounded; mode is always reliable and ordered.
* Queue and byte limits are explicit. No command implies hidden unbounded buffering.
* Errors are provider-neutral, typed by phase/category/retryability, retain safe diagnostic text/codes, and never expose provider error objects.
* Stats are normalized snapshots with optional/unknown fields and monotonic counters, not provider reports.
* Secret-bearing configuration and diagnostics must redact credentials.

## Usage sketch

```rust
let mut transport = factory.create(create_request)?;
transport.command(TransportCommand::CreatePeer { op: op(), config })?;
loop {
    match core::future::poll_fn(|cx| transport.poll(cx)).await? {
        TransportEvent::CommandCompleted { op, result } => engine.complete(op, result)?,
        TransportEvent::LocalCandidate { candidate } => signaling.send_v1(candidate)?,
        TransportEvent::SendCapacity { channel, available } => engine.resume(channel, available),
        TransportEvent::Closed { reason } => break reason,
        event => engine.on_transport_event(event)?,
    }
}
```

## Trade-off

A command/event algebra is more verbose than purpose-specific async methods, but radically reduces the public seam, makes ordering/backpressure/shutdown observable, avoids choosing an async runtime, and gives the fake complete deterministic control. The cost is a state machine and correlation IDs in the engine, plus versioning discipline for non-exhaustive commands/events.
