# Phase-2 `NativeTransportProvider` capability-trait interface (Gate 0)

> Design draft. This document defines the provider seam and its conformance-test surface; it does not implement a provider and does not change the V1 wire schema.

## Goals and boundary

The portable Connect engine and deterministic browser-interoperability harness depend only on a small, runtime-neutral Rust API. A provider owns its native peer, worker/runtime, callback adaptation, SDP parsing/formatting, and candidate-library representations. None of those types cross the seam. V1 signaling payloads remain opaque carriers: the engine may route and correlate them, but only the provider interprets their bytes/text according to the existing schema.

The API must expose capabilities before peer construction, validate STUN/TURN inputs including TURN transport and TLS policy, support complete negotiation (offer/answer, local/remote descriptions, trickle and explicit end-of-candidates, ICE restart), and provide one reliable ordered data channel with explicit bounded backpressure and buffered-low notification. All observable behavior is represented as provider-neutral commands, events, snapshots, errors, and stats.

## Proposed public shape

```rust
// In the portable crate. No tokio, native WebRTC, URL/candidate, callback, or SDP types.

use core::fmt::Debug;
use std::{borrow::Cow, time::Duration};

pub trait NativeTransportProvider: Send + Sync + 'static {
    type Peer: NativePeer;
    type Config: ProviderConfig;

    fn capabilities(&self) -> ProviderCapabilities;
    fn validate_config(&self, config: &Self::Config)
        -> Result<ValidatedPeerConfig<Self::Config>, TransportError>;
    fn new_peer(
        &self,
        config: ValidatedPeerConfig<Self::Config>,
        events: EventSink,
    ) -> Result<Self::Peer, TransportError>;
}

/// Provider-specific extension data is permitted only as inert, owned bytes under
/// an explicit provider namespace. Portable code neither downcasts nor executes it.
pub trait ProviderConfig: Clone + Send + Sync + Debug + 'static {
    fn common(&self) -> &PeerConfig;
    fn extension(&self) -> Option<&ProviderExtension> { None }
}

pub struct ValidatedPeerConfig<C> { /* private: C + validating provider identity */ }

pub trait NativePeer: Send + 'static {
    type Negotiation: NegotiationOps;
    type Ice: IceOps;
    type Data: ReliableOrderedDataOps;
    type Observe: ObserveOps;

    fn negotiation(&mut self) -> &mut Self::Negotiation;
    fn ice(&mut self) -> &mut Self::Ice;
    fn data(&mut self) -> &mut Self::Data;
    fn observe(&mut self) -> &mut Self::Observe;

    /// Begins idempotent bounded shutdown. No success event may be followed by
    /// a non-terminal event; all provider-owned workers are joined by deadline.
    fn shutdown(&mut self, deadline: Deadline) -> Result<ShutdownReport, TransportError>;
}

pub trait NegotiationOps {
    fn create_offer(&mut self, id: OperationId, options: OfferOptions)
        -> Result<(), TransportError>;
    fn create_answer(&mut self, id: OperationId, options: AnswerOptions)
        -> Result<(), TransportError>;
    fn set_local_description(&mut self, id: OperationId, description: V1Description)
        -> Result<(), TransportError>;
    fn set_remote_description(&mut self, id: OperationId, description: V1Description)
        -> Result<(), TransportError>;
}

pub trait IceOps {
    fn add_remote_candidate(&mut self, id: OperationId, candidate: V1Candidate)
        -> Result<(), TransportError>;
    fn end_remote_candidates(&mut self, id: OperationId, scope: CandidateScope)
        -> Result<(), TransportError>;
    fn restart_ice(&mut self, id: OperationId, options: IceRestartOptions)
        -> Result<(), TransportError>;
}

pub trait ReliableOrderedDataOps {
    /// Requests/open-negotiates the single Gate-0 reliable ordered channel.
    fn open(&mut self, id: OperationId, config: ReliableOrderedChannelConfig)
        -> Result<(), TransportError>;
    /// Atomic admission: `Accepted` means provider owns all bytes; `Backpressured`
    /// means it owns none. Never blocks waiting for capacity.
    fn try_send(&mut self, message: OutboundMessage)
        -> Result<SendAdmission, TransportError>;
    fn set_buffered_low_threshold(&mut self, bytes: u64)
        -> Result<(), TransportError>;
    fn buffered_amount(&self) -> Result<u64, TransportError>;
}

pub trait ObserveOps {
    fn state(&self) -> Result<PeerStateSnapshot, TransportError>;
    fn stats(&mut self, id: OperationId, selector: StatsSelector)
        -> Result<(), TransportError>;
}

/// An owned, bounded, nonblocking event ingress created by portable core.
/// It is a concrete portable type rather than a provider callback trait.
pub struct EventSink { /* private bounded queue handle */ }
impl EventSink {
    pub fn try_emit(&self, event: TransportEvent) -> Result<(), EventSinkError>;
}
```

Commands that can complete asynchronously take a caller-chosen `OperationId`; acceptance only means the operation was admitted. Completion/failure arrives exactly once as an event carrying that ID. Synchronous errors mean the operation was not admitted and no completion event will follow.

```rust
#[non_exhaustive]
pub struct ProviderCapabilities {
    pub signaling: SignalingCapabilities,
    pub ice: IceCapabilities,
    pub data: DataCapabilities,
    pub limits: ProviderLimits,
    pub shutdown: ShutdownCapabilities,
}

pub struct PeerConfig {
    pub ice_servers: Vec<IceServer>,
    pub ice_policy: IceTransportPolicy,
    pub operation_timeout: Duration,
    pub event_queue_capacity: usize,
    pub max_outbound_buffered_bytes: u64,
}

pub struct IceServer {
    pub urls: Vec<IceServerUrl>,
    pub credentials: Option<IceCredentials>,
}
pub struct IceServerUrl {
    pub scheme: IceScheme,                 // Stun | Stuns | Turn | Turns
    pub host: String,
    pub port: Option<u16>,
    pub transport: IceServerTransport,     // Udp | Tcp | Tls
    pub tls: Option<TurnTlsPolicy>,
}
pub struct TurnTlsPolicy {
    pub server_name: String,
    pub verification: CertificateVerification, // SystemRoots | PinnedSpki{sha256}
}
```

Construction validation rejects invalid combinations rather than silently weakening them: credentials on unsupported schemes, TURN without credentials, TLS policy on non-TLS transports, `turns` without TLS, TLS without a valid server name, unsupported transport/scheme/provider combinations, duplicate/conflicting URLs, or configured limits above advertised maxima. Secrets are redacted from `Debug`, errors, events, and stats.

The V1 carriers are deliberately semantic-free at this boundary:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Description {
    pub kind: DescriptionKind, // Offer | Answer
    pub carrier: OpaqueV1Carrier,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Candidate {
    pub carrier: OpaqueV1Carrier,
    pub scope: CandidateScope,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueV1Carrier(Box<[u8]>); // constructors enforce only V1 envelope size/encoding limits
```

Do not introduce typed SDP/candidate accessors here. Conversion between the unchanged wire envelope and `OpaqueV1Carrier` belongs in the existing signaling adapter; interpretation belongs inside the provider. Explicit `DescriptionKind` and `CandidateScope` are routing metadata already known by the protocol, not parsed SDP.

```rust
#[non_exhaustive]
pub enum TransportEvent {
    OperationCompleted { id: OperationId, output: OperationOutput },
    OperationFailed { id: OperationId, error: TransportError },
    LocalCandidate { candidate: V1Candidate },
    LocalCandidatesEnded { scope: CandidateScope },
    StateChanged(PeerStateSnapshot),
    DataChannelOpened { channel: ChannelId },
    MessageReceived { channel: ChannelId, message: InboundMessage },
    BufferedAmountLow { channel: ChannelId, buffered_amount: u64 },
    DataChannelClosed { channel: ChannelId, reason: CloseReason },
    Fatal(TransportError),
    ShutdownComplete(ShutdownReport),
}

#[non_exhaustive]
pub enum OperationOutput {
    Offer(V1Description),
    Answer(V1Description),
    LocalDescriptionSet,
    RemoteDescriptionSet,
    RemoteCandidateAdded,
    RemoteCandidatesEnded,
    IceRestartStarted { generation: IceGeneration },
    Stats(TransportStats),
}
```

State, errors, and stats use stable provider-neutral enums/records with `#[non_exhaustive]` and optional namespaced diagnostics as bounded redacted bytes/strings. Diagnostics must never be needed for control flow.

## Object safety and runtime implications

Associated types maximize compile-time provider flexibility: each provider can use optimized concrete facet adapters without heap allocation, downcasting, or exposing its runtime. The full `NativeTransportProvider` is intentionally not a convenient heterogeneous trait object because `Peer` and `Config` are associated and validation binds them. Production wiring should be generic (`Connect<P: NativeTransportProvider>`), which monomorphizes the seam and makes accidental provider mixing impossible.

Where runtime provider selection is required, add a narrow erasure adapter in a non-portable composition crate:

```rust
pub trait ErasedTransportProvider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;
    fn validate_and_new_peer(
        &self,
        config: ErasedPeerConfig,
        events: EventSink,
    ) -> Result<Box<dyn ErasedNativePeer>, TransportError>;
}
pub trait ErasedNativePeer: Send { /* same commands, no associated return types */ }
```

Erasure may box calls and loses provider-specific typed configuration; it must translate extensions by namespace and cannot expose `Any`. It is not the canonical conformance surface. Neither API implies async/await or an executor: methods are immediate command admission, events are drained by portable core, and each provider owns any runtime/thread it needs. Providers that require same-thread affinity must hide it behind a `Send` command proxy; the native object stays on the provider-owned worker. A provider unable to do so is not Gate-0 conformant.

## Deterministic usage

```rust
fn run_case<P: NativeTransportProvider>(provider: P, cfg: P::Config) {
    let caps = provider.capabilities();
    assert!(caps.data.reliable_ordered);

    let (events, sink) = bounded_transport_events(64);
    let validated = provider.validate_config(&cfg).unwrap();
    let mut peer = provider.new_peer(validated, sink).unwrap();

    peer.negotiation().create_offer(OperationId(1), OfferOptions::default()).unwrap();
    let offer = expect_offer(&events, OperationId(1));
    peer.negotiation().set_local_description(OperationId(2), offer).unwrap();

    peer.ice().add_remote_candidate(OperationId(3), remote_candidate()).unwrap();
    peer.ice().end_remote_candidates(OperationId(4), CandidateScope::Session).unwrap();

    peer.data().open(OperationId(5), ReliableOrderedChannelConfig::gate0()).unwrap();
    expect_open(&events);
    match peer.data().try_send(OutboundMessage::binary(b"ping".to_vec())).unwrap() {
        SendAdmission::Accepted { buffered_amount } => assert!(buffered_amount > 0),
        SendAdmission::Backpressured { buffered_amount, limit } => {
            peer.data().set_buffered_low_threshold(limit / 2).unwrap();
            expect_buffered_low(&events);
        }
    }

    peer.observe().stats(OperationId(6), StatsSelector::Peer).unwrap();
    let report = peer.shutdown(Deadline::after(Duration::from_secs(2))).unwrap();
    assert!(report.workers_joined);
}
```

The deterministic fake implements the same traits and owns a virtual clock plus a scripted command/event scheduler. It never sleeps, spawns a thread, or consults wall time. The harness advances it explicitly through a test-only controller retained beside (not obtainable from) the `NativePeer`; test control types therefore do not pollute the production seam. Scripts assert command ordering, inject admission backpressure/event-queue saturation, and schedule completion/state/candidate events deterministically.

## Core invariants

1. Capability claims are immutable for the provider instance and every accepted config is a subset of them.
2. A validated config is provider-bound, single-construction input, and cannot be forged by public fields.
3. Each admitted `OperationId` is unique while outstanding and produces exactly one completion or failure, except terminal shutdown reports cancellation explicitly.
4. Description sequencing is checked: answer requires a remote offer; local/remote description transitions cannot silently reorder. Glare behavior is advertised and deterministic.
5. Candidates are scoped to an ICE generation. After end-of-candidates, another candidate for that generation is rejected. Restart creates a new generation and does not relabel queued old-generation events.
6. `try_send` is nonblocking and all-or-nothing. `Backpressured` retains caller ownership. `Accepted` transfers ownership until delivery/close; buffered amount is monotonic between drain notifications modulo newly accepted sends.
7. Buffered-low is edge-triggered on crossing from above to at/below threshold, re-armed only after rising above it, and never substitutes for polling/admission truth.
8. Event order is FIFO per peer. Terminal/fatal state suppresses subsequent non-terminal events. Queue overflow is never silent: lossy stats may be coalesced if advertised; control/data events either use reserved capacity or force a visible fatal shutdown.
9. Provider callbacks never execute portable engine code. They only translate into owned events and perform nonblocking enqueue.
10. `shutdown` is idempotent, deadline-bounded, rejects new work once begun, resolves/cancels outstanding operations, closes channels, stops callbacks, and joins all owned workers. `Drop` is a last-resort nonblocking safety net, not successful shutdown.
11. All payload/config/event allocations are bounded by advertised and validated limits. Errors/stats never contain credentials, raw provider pointers, runtime handles, or unbounded native diagnostics.

## Error taxonomy

`TransportError` has stable categories: `UnsupportedCapability`, `InvalidConfiguration`, `InvalidState`, `InvalidSequence`, `DuplicateOperation`, `LimitExceeded`, `BackpressureProtocol`, `SignalingRejected`, `IceFailure`, `DataChannelFailure`, `Timeout`, `Cancelled`, `EventQueueSaturated`, `ProviderUnavailable`, `ShutdownIncomplete`, and `Internal`. It carries `ErrorContext { operation, retryability, provider_code: Option<RedactedDiagnostic> }`. Native codes are diagnostic only; retry policy derives from the stable category/retryability. Configuration errors identify a redacted field path and capability mismatch.

## Hidden complexity and tradeoffs

* Negotiation is a state machine, not four independent calls. Operation IDs and asynchronous completion prevent native callback timing from leaking while retaining deterministic tests.
* ICE restart creates generation and stale-candidate problems; generation must be explicit in scopes/events even if V1 wire fields remain unchanged. The signaling adapter derives correlation from existing V1 context; no schema extension is implied.
* `EventSink` queue saturation is a correctness issue. A single bounded queue needs reserved control capacity or a provider-local coalescing layer; received messages cannot be dropped invisibly.
* Native buffered-amount callbacks differ in edge semantics. Providers normalize to the crossing rule and may poll internally, within advertised precision/latency.
* Layered traits are deeper and more flexible than one monolithic interface, and let a fake replace facets, but associated types complicate trait objects and DI containers. Generic production wiring is the deliberate default; explicit erasure is a tax paid only for runtime selection.
* Owned carriers/messages make thread transfer and lifetime rules obvious but copy at some FFI boundaries. Providers may internally use pools; borrowed native buffers may not escape.
* Requiring `Send` for the peer proxy may add a mailbox hop for thread-affine stacks. This is preferable to infecting portable core with an executor or affinity type.
* Provider-specific configuration extensions preserve capability flexibility but risk semantic fragmentation. They are namespaced, inert, capability-advertised, size-bounded, and forbidden for behavior required by Gate 0.
* Returning a synchronous shutdown report is simple but a provider may need asynchronous teardown. The proxy may block only up to the explicit deadline; a future evolution could split `begin_shutdown`/terminal event without changing ownership rules.

## Conformance surface

Every provider, including the fake, runs the same black-box contract suite: capability/config matrices; offer/answer sequencing; local/remote failure propagation; trickle and end markers by generation; ICE restart with stale events; TURN UDP/TCP/TLS validation; reliable ordered message preservation; atomic send backpressure and buffered-low edges; queue saturation; normalized state/error/stats snapshots; duplicate IDs; timeouts/cancellation; and bounded idempotent shutdown with worker/callback leak checks. No test may downcast the provider or inspect native SDP/candidates/runtime state.
