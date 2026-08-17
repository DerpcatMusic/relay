# Phase-2 `NativeTransportProvider` interface and state machine

> **Status:** substantive first-pass design for RELAY Gate 0. This document is design-only; it does not change the V1 wire schema.

## Decision summary

RELAY owns a provider-neutral, portable Connect engine. Native WebRTC runtimes sit behind an object-safe `NativeTransportProvider` factory and one owned `Peer` command/event boundary. The public seam exposes only RELAY-owned values: opaque V1 signaling carriers, validated configuration inputs, stable state/error/event/stat enums, byte payloads, and bounded resource limits. No provider object, runtime/executor, thread handle, callback, candidate parser, SDP helper, or provider future crosses it.

The common lifecycle is an explicit deterministic transition machine, represented by a single `Peer` handle plus state-checked commands rather than a large family of provider-specific session objects. Every command returns synchronously with either a stable operation id/acceptance result or a provider-neutral error; completions arrive as ordered events. This makes a deterministic fake natural: it consumes commands, advances a virtual state machine, and emits scripted events without an async runtime.

```rust
pub trait NativeTransportProvider: Send + Sync + 'static {
    fn capabilities(&self) -> Capabilities;
    fn create_peer(&self, config: PeerConfig) -> Result<Box<dyn Peer>, TransportError>;
}

pub trait Peer: Send + 'static {
    fn state(&self) -> PeerSnapshot;
    fn command(&mut self, command: Command) -> Result<CommandAccepted, TransportError>;
    fn poll_event(&mut self) -> Result<Option<Event>, TransportError>;
    fn next_wake_deadline(&self) -> Option<MonotonicDeadline>;
    fn shutdown(&mut self) -> Result<ShutdownProgress, TransportError>;
}
```

`Peer` is owned and driven by exactly one Connect-engine thread. Implementations may own bounded internal worker threads, but all callbacks terminate inside the adapter and are serialized into a bounded event queue. The engine never invokes provider code concurrently on one peer.

## Core state machine

```text
New
 ├─ CreateOffer(op) ─> NegotiatingLocal ─ LocalDescriptionReady ─> HaveLocalOffer
 ├─ SetRemote(offer) ─> HaveRemoteOffer ─ CreateAnswer(op)
 └─ Shutdown ─> Closing ─ ShutdownComplete ─> Closed

HaveLocalOffer ─ SetRemote(answer) ─> Connecting
HaveRemoteOffer ─ CreateAnswer ─> NegotiatingLocal ─ LocalDescriptionReady ─> Connecting
Connecting ─> Connected ─> DataChannelOpen
Connected ─ RestartIce(op) ─> RestartingIce ─ LocalDescriptionReady ─> Connecting
(any non-Closed state) ─ fatal error / shutdown ─> Closing ─> Closed
```

Remote trickle candidates are accepted only after the corresponding remote description epoch is installed. Local candidates and the end-of-candidates marker are events tagged with the matching negotiation epoch. ICE restarts increment the epoch and make stale-epoch signaling a deterministic error rather than silently applying it.

## Initial command and event vocabulary

```rust
pub enum Command {
    CreateOffer { op: OperationId },
    CreateAnswer { op: OperationId },
    SetRemoteDescription { op: OperationId, description: RemoteDescription },
    AddRemoteCandidate { epoch: NegotiationEpoch, candidate: RemoteCandidate },
    EndRemoteCandidates { epoch: NegotiationEpoch },
    RestartIce { op: OperationId },
    OpenDataChannel { op: OperationId, config: DataChannelConfig },
    Send { channel: ChannelId, payload: Bytes },
    RequestStats { op: OperationId },
    CloseDataChannel { channel: ChannelId },
    BeginShutdown,
}

pub enum Event {
    LocalDescriptionReady { op: OperationId, epoch: NegotiationEpoch, description: LocalDescription },
    LocalCandidate { epoch: NegotiationEpoch, candidate: LocalCandidate },
    LocalCandidatesComplete { epoch: NegotiationEpoch },
    SignalingStateChanged(SignalingState),
    IceStateChanged(IceState),
    ConnectionStateChanged(ConnectionState),
    DataChannelOpened { op: OperationId, channel: ChannelId },
    DataReceived { channel: ChannelId, payload: Bytes },
    BufferedAmountLow { channel: ChannelId, buffered_amount: u64 },
    DataChannelClosed { channel: ChannelId, reason: ChannelCloseReason },
    Stats { op: OperationId, report: StatsReport },
    OperationFailed { op: OperationId, error: TransportError },
    ShutdownComplete,
}
```

The final design must sharpen wake mechanics, queue bounds, byte ownership, capability negotiation, config validation, exact description carrier types, error taxonomy, and typestate-vs-explicit-machine tradeoffs after repository inspection.
