# Option C: A Typed Finite-Stream Drain Protocol

## Status and objective

**Status:** design only; no implementation is proposed in this change.

Relay needs one end-of-stream vocabulary across capture resampling, Opus transmit,
Opus receive/FEC, receive resampling, and playback publication. Today those stages
have different kinds of buffered truth: a partial capture packet, valid samples in
an Opus frame padded to a legal duration, convolution history, a packet held while
waiting to decide FEC, and decoded audio waiting for ring capacity. Treating any one
of these as "the end" either truncates audio or makes shutdown timing-dependent.

This option models a finite stream as a typed state machine and makes draining a
bounded, caller-driven operation. It deliberately does **not** make the audio
callback drain, allocate, block, lock, destroy codecs, or wait for a producer. The
callback continues to consume already-published playback blocks and observes a
separate terminal marker only after every preceding frame is visible.

The central rule is:

> EndOfStream is an input fact; Drained is a stage-produced proof that no more
> meaningful output can be produced for that stream epoch.

A reset or disconnect is not a graceful EndOfStream. It is an abort that invalidates
an epoch and drops its pending state.

## Public protocol sketch

The common type is an envelope, not an untyped universal marker. Every seam chooses
its own terminal metadata, so capture frames, RTP sequence positions, and device
frames cannot be mixed accidentally.

```rust
/// Monotonically changes whenever storage or state is reused for another stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamEpoch(NonZeroU64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndOfStream<T> {
    pub epoch: StreamEpoch,
    pub terminal: T,
    pub reason: EndReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndReason { Completed, PeerCompleted }

pub struct CaptureEnd {
    /// Total meaningful capture-domain frames, including `final_chunk`.
    pub total_frames: u64,
}

pub struct MediaEnd {
    pub terminal: MediaTerminal,
}

pub enum MediaTerminal {
    Empty,
    Packet {
        sequence: ExtendedSequence,
        /// Meaningful 48 kHz frames in this negotiated-duration packet.
        valid_frames: NonZeroU16,
    },
}

pub struct PlaybackEnd {
    /// Exact meaningful 48 kHz extent accepted from RX.
    pub media_frames: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainPoll<P, W> {
    /// At least one cursor advanced or one output transaction was accepted.
    Progress(P),
    /// No state changed. The caller must satisfy the typed wait condition.
    Pending(W),
    /// Terminal proof for this stage, including its exact accounting report.
    Drained(Drained<P>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Drained<R> {
    pub epoch: StreamEpoch,
    pub report: R,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainBudget {
    pub max_resampler_calls: NonZeroUsize,
    pub max_codec_calls: NonZeroUsize,
    pub max_publications: NonZeroUsize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FinishError<E> {
    WrongEpoch { active: StreamEpoch, supplied: StreamEpoch },
    ConflictingEnd,
    InputAfterEnd,
    NotEnding,
    WorkspaceTooSmall { required: usize, supplied: usize },
    TerminalExtentOutOfWindow,
    Stage(E),
    Faulted,
}
```

The dependency-safe home for `StreamEpoch`, generic `EndOfStream<T>`,
`DrainPoll<P, W>`, `Drained<R>`, `DrainBudget`, and generic lifecycle errors is a
tiny `#![no_std]`, dependency-free leaf crate (`relay-finite-stream`, name subject to
repository convention). `relay-resample` and `relay-audio` can both depend on it
without a cycle. Audio terminal payloads and wait reasons remain in `relay-audio`:

```rust
pub enum OutputPending { OutputCapacity }

pub enum RxDrainPending {
    OutputCapacity,
    PlayoutDeadline { sequence: ExtendedSequence },
}
```

Concrete modules implement the common shape with domain-specific methods; Relay
should **not** add a public object-safe `FiniteStage` trait. Such a trait would erase
useful terminal/wait types and workspace bounds without providing a second adapter
at a real seam. Resampler reports/errors remain in `relay-resample` and are wrapped
by Tx/playback.

`begin_end` is idempotent for a byte-for-byte/equality-identical marker in the same
epoch. A conflicting duplicate is a sticky protocol error. `drain_step` after
success returns the same `Drained` proof without touching a sink. Normal input after
accepted EOS is rejected. `Pending` guarantees no progress, which is important:
callers may sleep on ring/network capacity or a playout deadline without polling.

Workspaces are sized from the authoritative converters/codecs at construction and
allocated by the caller. A concrete worker binds one mutable workspace for the
whole ending state (by an owning session wrapper or an internal opaque binding ID),
so swapping a blank workspace between retries cannot resurrect or duplicate pending
content. Illustrative concrete signatures appear below.

`StreamEpoch` is a local generation/capability, not an untrusted wire integer. A
transport end record carries the authenticated session/SSRC identity plus terminal
metadata; the ingress adapter maps that identity to the active local epoch before it
can construct `EndOfStream<MediaEnd>`. Stale peer records therefore cannot select a
newly reused worker merely by guessing its generation.

## State model

Each stage has explicit `Open(epoch)`, `Ending(epoch, phase)`, `Drained(epoch,
report)`, and `Faulted(epoch, error)` states. These are runtime states because Relay
stores long-lived workers behind queues and resets them; a pure consuming typestate
would make that orchestration substantially harder. An optional borrowing
`Ending<'worker, 'workspace>` facade can improve direct/offline use, but the worker
state remains authoritative.

The orchestrator owns stage ordering. It closes a downstream stage only after the
upstream stage proves `Drained`; output made while computationally flushing an
upstream filter is still ordinary downstream input. Backpressure suspends a phase
in `Ending`, never changes logical lengths, and never lets a downstream EOS overtake
pending data. A composite driver is a cursor over this DAG, not a loop hidden inside
one module.

Abort is a different transition. `abort(prepared_reset)` invalidates the old epoch,
clears logical pending lengths, and opens the prepared newer epoch; it does not emit
old tail or an old terminal marker. Codec reconstruction can fail, so fallible work
is prepared before commit where possible and failure leaves the old worker
`Faulted`, never half-reset. Large reclamation and endpoint destruction remain off
the callback. For the current untagged sample ring, abort either waits for the ring
to become empty or replaces the worker/renderer pair after the device callback has
stopped; it may not mix old queued samples into a new epoch.

## Valid-frame truth

Every potentially padded value distinguishes physical duration from logical extent.
For `PcmSpan`, `interleaved.len() == physical_frames * channels` and consumers may
read only the first `valid_frames * channels` samples:

```rust
pub struct PcmSpan<'a> {
    pub interleaved: &'a [f32],
    pub channels: NonZeroU16,
    pub physical_frames: u16,
    pub valid_frames: u16,
}

pub struct PacketAudioExtent {
    /// Duration returned by Opus for this packet; normally the negotiated fixed
    /// `FrameDuration` in Relay V1.
    pub decoded_frames: u16,
    /// Meaningful PCM prefix. Less than decoded only on the terminal packet.
    pub valid_frames: u16,
}
```

Relay's current `MediaPacket` contains only SSRC, sequence, RTP timestamp, payload
type, and Opus bytes. Opus itself cannot encode "only the first N decoded frames are
valid," and sample values cannot reveal it because real audio may end in zeros.
Option C therefore requires `PacketAudioExtent` in an integrity-bound RTP header
extension or equivalent ordered media envelope. `MediaEnd` repeats the final
sequence/valid length as an end-to-end consistency check; it cannot be the sole
carrier because it may arrive after the final packet's playout deadline. A transport
without this negotiated metadata must reject exact finite mode before accepting
input (or explicitly define all padding as content).

TX pads a final partial media frame to the **negotiated fixed** Opus
`FrameDuration`, not an opportunistically smaller duration. RX validates
`0 < valid_frames <= decoded_frames == negotiated_frames`, associates the extent
with that exact extended sequence, and returns a logical `PcmSpan`. Ordinary and
FEC-recovered nonterminal positions are full duration. Metadata on packet N always
describes packet N's normal decode, never PCM recovered for N-1 from N's FEC. If the
terminal packet is lost, PLC is trimmed to its announced valid prefix only when the
trusted per-packet/end metadata is available.

Padding and virtual resampler flush zeros are two different facts. Opus padding is
physical PCM sent through the codec and excluded by `valid_frames`; flush zeros are
an internal computation used to expose delayed resampler output and never enter the
logical input count.

## Stage-specific behavior

### Fixed/adaptive resamplers and the partial capture chunk

The current `WorkerResampler` deliberately accepts only
`requirements().input_frames_next` and has no drain method; the current
`FiniteFixedRatioConverter` instead consumes a whole finite slice, pumps Rubato,
and reports leading/trailing trim. Option C extracts that finite machinery into a
resumable ending state usable by both `FixedRatioConverter` and
`AdaptiveClockConverter`, without making live `process_interleaved` accept ambiguous
short chunks.

```rust
pub enum ResamplerEndPolicy {
    /// The epoch was opened drainable: suppress declared leading delay from its
    /// first output and return the exact finite timeline mapping at EOS.
    ExactMappedExtent,
    /// Preserve causal streaming latency and flush delayed response at EOS. The
    /// report exposes the leading latency/extra physical duration explicitly.
    PreserveCausalLatency,
    /// Abandon retained history; this is reported data loss, never "drained".
    Truncate,
}

pub struct ResamplerDrainReport {
    pub meaningful_input_frames: u64,
    pub virtual_flush_input_frames: usize,
    pub raw_output_frames: u64,
    pub leading_delay_frames: usize,
    pub leading_trim_frames: usize,
    pub included_filter_tail_frames: usize,
    pub trailing_trim_frames: usize,
    pub returned_output_frames: u64,
}

impl FixedRatioConverter {
    pub fn begin_end(
        &mut self,
        end: EndOfStream<CaptureEnd>,
        final_input: &[f32], // 0..input_frames_next, whole interleaved frames
        ws: &mut FixedDrainWorkspace,
    ) -> Result<(), FinishError<ResampleError>>;

    pub fn drain_step(
        &mut self,
        ws: &mut FixedDrainWorkspace,
        output: &mut impl PcmSink,
        budget: DrainBudget,
    ) -> Result<DrainPoll<ResamplerDrainReport, OutputPending>, FinishError<ResampleError>>;
}
```

`begin_end` validates the entire final slice and total extent before advancing the
backend: `CaptureEnd.total_frames` must equal previously accepted frames plus this
whole-frame suffix, with checked arithmetic. It then copies the suffix into the
bound caller workspace; success means the caller may release the capture buffer. A
capacity/validation error consumes neither EOS nor samples. This directly handles
the 1..chunk-1 capture frames that the current `CaptureInput::Chunk` cannot
represent. Zero frames is valid.

Exact alignment is an **epoch-start contract**, not something EOS can repair.
A drainable epoch suppresses/withholds the backend's first `output_delay()` raw
frames before any downstream publication and records the phase of every accepted
call. At EOS it feeds the backend's supported partial input form and a statically
bounded number of virtual-zero calls until the exact mapped endpoint is available,
then trims only raw suffix/padding. For a fixed ratio the returned logical count is
`ceil(total_input * output_rate / input_rate)`. Calling `ExactMappedExtent` on a
legacy epoch that already published startup delay returns a precondition error.

`PreserveCausalLatency` supports such conventional live epochs without pretending
retroactive trim: it flushes the delayed response, reports the leading latency and
extra physical duration, and lets the higher-level format carry or trim that range.
It is inappropriate for scheduled Relay playback unless the presentation timeline
was opened with that latency offset. Thus FIR history is explicit computational
support, not accidentally lost or silently declared extra media. `Truncate` returns
an `AbortedReport`, not `Drained`.

Fixed ratio phase/count arithmetic uses checked integer rational arithmetic. For the
adaptive converter, every normal call records the accepted input/output phase. EOS
snapshots the current smoothed ratio and freezes both controller input and smoothing;
the terminal schedule is therefore finite and reproducible rather than waiting for
future clock observations. The exact mapped endpoint is the snapshotted cumulative
phase result, not a fresh `ceil(total * last_ratio)` that rewrites earlier history.
Construction derives worst-case flush calls, raw output, and workspace from Rubato's
`output_delay`, max input/output, filter support, and the admitted correction range.
A backend/configuration without a finite authoritative bound cannot implement exact
drain.

### Opus transmitter

A Tx stream that may end exactly is opened in drainable, delay-compensated mode
before its first capture chunk; EOS cannot upgrade today's causal/truncating live
converter after startup samples were packetized. TX first drains the resampler's
ordinary valid output into the existing 48 kHz accumulator. After the resampler
proves drained it emits complete negotiated frames;
if PCM remains, it zero-fills exactly one negotiated Opus frame, encodes once, and
attaches the original logical prefix as `PacketAudioExtent`. It then offers
`EndOfStream<MediaEnd>`. Packet and marker commits advance sequence/state only when
the sink accepts them.

```rust
impl TxWorker {
    pub fn begin_end(
        &mut self,
        end: EndOfStream<CaptureEnd>,
        final_capture: &[f32],
        ws: &mut TxDrainWorkspace,
    ) -> Result<(), TxFinishError>;

    pub fn drain_step(
        &mut self,
        ws: &mut TxDrainWorkspace,
        batch: &mut PacketBatch,
        end_slot: &mut Option<EndOfStream<MediaEnd>>,
        budget: DrainBudget,
    ) -> Result<DrainPoll<FiniteTxReport, OutputPending>, TxFinishError>;
}
```

`PacketBatch` must be empty on entry, matching the existing Tx contract. A full
batch returns `Pending(OutputCapacity)` only if no publication fit; if some packets
were appended it returns `Progress`. `end_slot` is separate because the current
`MediaPacket` has no control variant. The transport adapter must accept the packet
batch before the end marker; if packet and control channels can reorder, the final
extended sequence in `MediaEnd` still supplies the RX fence.

An encode success followed by batch/network backpressure is retained as one bounded
pending packet in `TxDrainWorkspace`; retries publish byte-identical payload and
metadata and never encode it twice. The workspace needs one capture chunk, one
converter quantum/tail window, one negotiated PCM frame, and `MAX_PACKET_BYTES`—not
storage proportional to stream duration. An empty stream emits only
`MediaEnd { terminal: MediaTerminal::Empty }`. Exact finite mode is rejected during
negotiation when valid
length metadata cannot be carried.

### Opus receiver and FEC

The current `RxWorker` deliberately has one packet of lookahead: `tick()` stages a
position and resolves the preceding one; `drain()` resolves the final staged
position without inventing another deadline. EOS must preserve that model. A peer
end announces the last permissible sequence, but it does **not** assert that every
preceding UDP packet has already arrived. RX continues accepting valid packets and
advances only when the caller supplies the existing playout-deadline fact.

```rust
pub enum RxDrainInput {
    Poll,
    /// Same semantic input as today's `RxWorker::tick`: a scheduler decision,
    /// not wall time or packet-arrival time.
    Deadline(ExtendedSequence),
}

impl RxWorker {
    pub fn begin_end(
        &mut self,
        end: EndOfStream<MediaEnd>,
        ws: &mut RxDrainWorkspace,
    ) -> Result<(), RxFinishError>;

    pub fn drain_step(
        &mut self,
        event: RxDrainInput,
        ws: &mut RxDrainWorkspace,
        decoded: &mut impl PcmSink,
        budget: DrainBudget,
    ) -> Result<DrainPoll<RxDrainReport, RxDrainPending>, RxFinishError>;
}
```

Before each unresolved position's deadline the result is
`Pending(PlayoutDeadline { sequence })`; no early PLC is produced merely because an
end marker won a network race. At a permitted deadline RX decodes a present packet,
uses packet N+1 for N's in-band FEC where available, or applies the configured
bounded PLC policy. After the final position's deadline, the existing staged
lookahead is resolved with `None`; RX never waits for or invents a packet after the
announced final sequence. `MediaTerminal::Empty` is valid only before any position
was
accepted/emitted.

`MediaEnd` is validated against the active SSRC/epoch, negotiated duration, current
extended-sequence window, and a configured maximum terminal gap. This prevents a
forged far-future end from scheduling billions of PLC calls. An end behind already
rendered media is a protocol error unless it is an exact duplicate of the retained
terminal record. Reorder entries beyond the final sequence are returned/rejected.

The decoded span for the last packet (normal decode or final PLC) is shortened to
its trusted `valid_frames` before receive resampling. A missing nonterminal position
is full negotiated duration. Packet N's extent never trims FEC/PLC produced for
N-1. Because there is no N+1 after the terminal packet, that last packet itself
cannot be recovered using in-band FEC; its loss uses the configured final PLC/silence
policy and the trusted terminal valid extent.

### Receive resampler and playback publication

Receive resampling uses the same ending contract as capture conversion, but
`ExactMappedExtent` is mandatory for scheduled playback. Consequently an
EOS-capable playback epoch is declared before its first frame and withholds the
adaptive converter's leading delay from ring publication; a legacy already-running
epoch cannot be upgraded losslessly at its end. The converter sees logical decoded
prefixes, not Opus padding. On EOS, adaptive clock input and smoothing freeze at the
last accepted scheduled observation; the worker computationally flushes to its
snapshotted device-frame endpoint and reports raw delay/trim. No synthetic clock
observation is generated for virtual zeros.

The current playback ring stores scalar samples, and replacing it with an enum that
inlines a maximum audio block would waste space and change normal callback copying.
Instead deepen the existing ring with an in-order terminal fence:

```rust
impl AudioProducer {
    /// Nonblocking. The fence captures this producer's monotonic committed-sample
    /// cursor after all prior writes.
    pub fn finish(&mut self, epoch: StreamEpoch) -> FinishWriteOutcome;
}

pub enum FinishWriteOutcome {
    Published { fence_samples: u64 },
    Full,
    ConsumerDisconnected,
    AlreadyPublished { fence_samples: u64 },
}

pub enum ReadState {
    Complete,
    Underrun,
    EndReached { epoch: StreamEpoch },
    Disconnected, // endpoint destruction/abort, not graceful EOS
}
```

Internally this is the existing fixed sample SPSC plus a small fixed SPSC fence
queue. Each fence contains `(epoch, total_committed_samples)`. Publishing a fence is
release-ordered after sample writes. The callback maintains `total_read_samples`;
it may observe a fence early but retains one inline pending fence and reports
`EndReached` only after consuming through that cursor. Acquire/release ordering then
makes `EndReached` proof that every prior audio sample was observable in order. The
ring module hides cursor wrap and the two-queue ordering from playback callers.

After `finish`, that producer rejects new audio/epochs until the callback's end
acknowledgement is observed and the pair is explicitly reopened (or replaced). This
single-outstanding-epoch rule prevents a callback race in which it polls the fence,
misses a concurrently published marker, then reads across the marker into a newer
stream from the independent sample ring. Supporting overlapped epochs would instead
require an in-band descriptor ring that can cap each read at a fence; Option C does
not pay that cost.

During normal live operation `PlaybackWorker::process_frame` may retain today's
all-or-drop overload policy. Once EOS is accepted, however, converted terminal
output remains in the bound drain workspace until `AudioProducer::write` accepts it;
`DroppedFull` is forbidden because it would falsely complete a finite stream. A full
sample ring or fence queue returns `Pending(OutputCapacity)` without spinning. The
worker is `Drained` when the fence is published. That is distinct from rendered:
the callback publishes a small atomic `last_rendered_end_epoch` only when it returns
`EndReached`.

The callback still only zero-fills, copies from the ring, compares bounded counters,
and performs lock-free cursor/acknowledgement operations. It never calls a codec,
resampler, `drain_step`, allocation, lock, log, syscall, drop of a large owner, or
unbounded loop. Underrun before the fence is silence, not terminal acknowledgement.
Endpoint `Disconnected` remains abort/lifetime information and must never be
reinterpreted as graceful `EndReached`.

## Usage and orchestration

```rust
// Worker/control thread. `final_capture` may be empty or shorter than the normal
// fixed capture chunk; success copies it into `tx_ws`.
tx.begin_end(
    EndOfStream {
        epoch,
        terminal: CaptureEnd { total_frames },
        reason: EndReason::Completed,
    },
    final_capture,
    &mut tx_ws,
)?;

match tx.drain_step(
    &mut tx_ws,
    &mut empty_packet_batch,
    &mut empty_end_slot,
    budget,
)? {
    DrainPoll::Progress(report) => schedule_worker_again(report),
    DrainPoll::Pending(OutputPending::OutputCapacity) => wait_for_transport_capacity(),
    DrainPoll::Drained(done) => transport_now_owns_final_marker(done),
    DrainPoll::Pending(_) => unreachable!(),
}

// Receive worker: the scheduler, not RX, decides when a missing position expired.
rx.begin_end(peer_end, &mut rx_ws)?;
match rx.drain_step(RxDrainInput::Poll, &mut rx_ws, &mut pcm_sink, budget)? {
    DrainPoll::Pending(RxDrainPending::PlayoutDeadline { sequence }) =>
        arm_existing_playout_deadline(sequence),
    other => handle_rx_progress(other),
}
// At that deadline, retry with RxDrainInput::Deadline(sequence).
```

The production coordinator composes RX, receive resampling, and playback publication
behind one deep worker interface. It retains at most one output quantum at each seam
and keeps a stage cursor: resolve RX; feed all accepted PCM to SRC; after RX is
`Drained`, close SRC; after SRC is `Drained`, publish the playback fence. A later
stage returning backpressure prevents its upstream pending cursor from advancing.
No caller manually injects zeros or computes filter trim.

Every invocation has `DrainBudget`; reaching it after progress returns `Progress`
and schedules another worker turn. `Progress` never means "run until finished."
Worker `Drained` means all output/fence values were accepted by their immediate
bounded sink. Callback completion is queried separately with
`last_rendered_end_epoch.load(Acquire)`; publication and rendering are deliberately
not collapsed into one state.

## Reset, disconnect, idempotence, and errors

* **Graceful local stop:** capture submits EOS together with its final short chunk,
  then schedules bounded steps until Tx output owns `MediaEnd`.
* **Graceful peer stop:** RX accepts a typed `MediaEnd`, continues deadline decisions
  through its final sequence, and ultimately publishes a playback fence.
* **Reset/seek/reconfigure:** aborts the old epoch, clears logical pending lengths
  and phase/FEC/codec state, and opens a strictly newer prepared epoch. It emits no
  tail or terminal marker for the old epoch.
* **Disconnect/timeout:** uses today's explicit truncating path unless a valid EOS
  was already accepted and its sinks remain usable. Optional `ConcealThenAbort`
  has a configured maximum PLC count and still returns `Aborted`, never `Drained`.
* **Backpressure:** is `Pending(OutputCapacity)`, not an error. The pending value and
  every cursor remain unchanged.

Idempotence is deliberately narrow:

| Operation | Repetition/result |
| --- | --- |
| identical EOS in `Ending` | `Ok`, no second tail/packet/fence |
| conflicting EOS for same epoch | sticky `ConflictingEnd` |
| ordinary input after EOS | `InputAfterEnd`, input unconsumed |
| `drain_step` after success | same `Drained { epoch, report }` |
| retry after `Pending` | resumes exact pending bytes/samples |
| abort to already-active newer epoch | `Ok`, no additional reset |
| stale old-epoch command/retry | `WrongEpoch`, no mutation |
| endpoint disconnect while ending | abort/fault with committed report; never drained |

Errors are split into three classes. Precondition/capacity errors found before state
advance leave the stage usable. `Pending` is normal flow control. A backend codec,
resampler, metadata, arithmetic, or ring invariant error after any committed output
is sticky until reset; each concrete error carries the exact committed report in the
same spirit as today's `TxProcessFailure`/`FiniteTxError`. Already published packets
or samples are never retracted and the caller must abort the epoch. Error retries
return the same error/accounting rather than running the failed operation again.

EOS, reset, and error commands are worker/control-plane operations. They cross a
bounded command queue; queue-full is returned to the non-RT owner. The callback sees
only the sample ring, terminal fence, and atomic acknowledgement. It never races or
mutates resampler/codec state.

## Hidden implementation invariants

* At most one final capture chunk, encoded packet, decoded frame, resampler output
  quantum, and unpublished ring transaction is retained at each seam. Bounds derive
  from `AudioPipelineConfig`, converter `FrameRequirements`, negotiated
  `FrameDuration`, reorder/end-gap limits, and `MAX_PACKET_BYTES`.
* The exact workspace bound is validated off-thread and the same logical workspace
  is bound for every retry in an epoch; drain never allocates or grows it.
* Pending content remains byte/sample-identical until accepted. Codec, sequence,
  timeline, valid-length, phase, and publication cursors advance exactly once at
  documented commit points.
* `Pending` changes no state. `Progress` advances at least one monotonic cursor.
  A finite terminal extent plus finite flush bound therefore cannot livelock when
  sinks/deadlines eventually become available.
* A downstream EOS/fence cannot be offered until its upstream is `Drained` and all
  seam output was accepted. A stage produces nothing after returning `Drained`.
* `valid_frames <= physical_frames`; logical counts never include Opus padding or
  virtual flush zeros. Terminal extent metadata is tied to epoch and exact extended
  sequence before decode.
* Fixed resampling terminates by checked rational target counts. Adaptive resampling
  terminates by a snapshotted phase endpoint and frozen ratio schedule. Neither uses
  floating-point "approximately empty" as a terminal test.
* Raw generated output decomposes exactly into leading trim + returned mapped output
  + trailing trim (plus separately reported optional filter tail). No frame belongs
  to two categories.
* A playback fence's committed-sample cursor is captured only after all prior sample
  writes; callback acknowledgement occurs only after its read cursor reaches it.
  No next-epoch sample is published through that ring pair before acknowledgement.
* Codec reset/destruction, ring endpoint replacement, and reclamation do not run in
  the playback callback.

## Test plan

State-machine/property tests generate bounded interleavings of data, EOS, exact and
conflicting duplicates, sink refusal, deadline permission, reset, disconnect, and
stale epochs. The oracle tracks logical extent and commit cursors independently of
the implementation. At every step it checks `Pending` purity, exactly-once output,
upstream/downstream order, and stable terminal/error reports.

Focused tests include:

1. Empty stream; exact capture chunk/packet; every final capture length from zero to
   `input_frames_next - 1`; and every 48 kHz packet residual from one to negotiated
   `FrameDuration - 1`. The final Opus physical duration remains negotiated while
   valid length is exact.
2. A real full-duration frame ending in zeros is **not** trimmed, while a padded
   terminal frame with nonzero/zero/noisy content is trimmed solely by extent
   metadata. Invalid zero/oversized/mismatched extents are rejected before decode.
3. Batch refusal after terminal encode and end-slot refusal after all packets: retry
   yields identical bytes, sequence, timestamp, extent, one encode call, and one end.
4. Fixed resampler impulses at first and last source frames for all supported rates,
   validating `ceil` mapped count and the raw = leading + returned + trailing
   accounting. Split at every chunk boundary and compare with one-shot
   `FiniteFixedRatioConverter` as a differential oracle.
5. Adaptive impulses/noise at min/max corrections and changing corrections before
   EOS; validate snapshotted cumulative endpoint, frozen terminal ratio, finite
   maximum flush calls, and chunk-boundary determinism.
6. EOS arriving before the final packet: RX returns `PlayoutDeadline`, accepts that
   packet until deadline, and does not prematurely PLC. Cover present final packet,
   missing N recovered from N+1 FEC, missing terminal packet (no future FEC carrier),
   terminal PLC trim, empty stream, duplicate/reordered end, and end beyond the
   configured gap/window.
7. Playback sample ring full on terminal audio and fence queue full after audio:
   repeated steps lose nothing; callback cannot report `EndReached` before its read
   cursor crosses the fence. Cover wraparound of monotonic/ring cursors and endpoint
   destruction distinct from graceful end.
8. Underrun before the fence proves silence is not terminal. Publishing versus
   callback-consuming the fence yields distinct worker/renderer observations.
9. Identical/conflicting EOS, drain-after-drained, input-after-EOS, wrong epoch,
   abort/reset from every pending phase, reset failure, workspace mismatch, stale
   queued command, and stale backpressure retry.
10. Budget tests set every limit to one and prove each call stays bounded while the
    complete result equals a large budget. Allocation counters confirm stable
    workspace/ring identities and zero allocations during begin/end/drain.
11. Callback audit/instrumentation proves no allocation, locks, logging, syscalls,
    codec/resampler calls, owner destruction, or unbounded loops. Loom (or a small
    equivalent concurrency model) verifies sample-write -> fence-publish ->
    sample-read -> end-ack acquire/release ordering; sanitizer runs cover ownership.
12. Existing live Tx disconnect truncation, ordinary Rx tick/drain, overload drop,
    and playback render tests remain unchanged, proving graceful EOS is opt-in.

Golden end-to-end tests run impulse, ramp, trailing-zero, and deterministic noise
through capture rate conversion, real Opus, reordered/lost receive, adaptive
playback conversion, and a small ring. They assert exact logical extent, report
algebra, packet sequence/timestamp/extent, FEC source identity, final fence order,
and callback acknowledgement against an offline reference within codec tolerance.

## Complexity, memory, and latency

For one call, let `R` be the budgeted resampler output frames, `C` channels, `K`
the configured bounded filter taps, `F` one negotiated Opus frame, and `U` the
budgeted publications/sequence decisions. Work is `O(R*C*K + F*C + U)` plus bounded
packet copies of at most `MAX_PACKET_BYTES`. A fence publication/comparison is
`O(1)`. The budget caps the number of such operations, so one invocation never
scales with the full stream length.

Total post-EOS CPU work is linear in already accepted partial PCM, the bounded RX
terminal sequence gap, and the statically bounded resampler flush. RX may be pending
on real playout deadlines for already announced positions, and all workers may be
pending on sink capacity; therefore wall-clock completion is intentionally not
bounded by CPU work. No stage waits for a sequence beyond that named by `MediaTerminal::Packet` or
for new clock observations to make a filter terminate.

Caller-owned drain workspace is:

```text
O(C * (one input chunk + one raw resampler quantum/tail window
       + one Opus PCM frame + one decoded/staged PCM frame)
  + MAX_PACKET_BYTES + constant cursors/reports)
```

It is independent of total stream duration. Resampler/Opus implementations may own
their already-prewarmed fixed internal filter/codec storage, as they do today. The
playback ring remains `O(playback_ring_samples)` plus a small fixed fence queue—not
`ring_capacity * maximum_audio_block`. Epochs, valid lengths, rational/phase
counters, pending flags, and callback acknowledgement are constant-size. All
capacity arithmetic is checked before streaming.

## Repository fit and migration shape

This proposal builds on current behavior rather than relabeling it:

* `relay-resample::WorkerResampler` remains the live exact-chunk interface.
  Resumable finite internals are factored from `FiniteFixedRatioConverter`; its
  one-shot report remains a useful differential oracle/convenience wrapper.
* `relay-audio::TxWorker` keeps `CaptureInput::Disconnected` as explicit truncation.
  Graceful `begin_end(final_capture)` is a separate path; `FiniteTxWorker` can later
  become a wrapper over it instead of a second packetization implementation.
* `RxWorker::tick` remains the only deadline fact, and its current one-item
  `drain()` behavior becomes the last phase of typed peer ending rather than a
  free-standing ambiguous operation.
* `PlaybackWorker::process_frame` keeps live all-or-drop overload behavior. Ending
  uses retained output and a resumable write. `PlaybackRenderer::RenderState` gains
  `EndReached`; current `Disconnected` retains endpoint-lifetime meaning.
* `MediaPacket` or its transport envelope gains integrity-bound `PacketAudioExtent`.
  This is the only required wire-format change, and exact finite negotiation gates
  it before audio starts.

The generic envelope/poll/budget vocabulary lives in the dependency-free leaf;
audio-specific terminal/wait types live in `relay-audio`. Backend partial-flush
mechanics and phase accounting stay hidden inside `relay-resample`; codec padding
stays hidden inside Tx; FEC ordering stays
hidden inside RX; sample/fence cursor ordering stays hidden inside the ring. This is
the depth benefit: callers learn one ending state model without learning Rubato,
Opus, reorder, or atomic publication mechanics.

## Trade-offs

Runtime explicit states do not make illegal transitions unrepresentable as pure
Rust typestate could. In exchange they fit Relay's resettable, queue-driven,
long-lived workers and make abort/backpressure practical. Typed terminal payloads,
epochs, sticky validation, and optional borrowing wrappers recover most safety
without forcing every scheduler to store self-referential typestate sessions.

Option C is more invasive than a separate one-shot finite pipeline: live modules gain
ending states, media needs valid-length metadata, and the sample ring gains a fence
queue. It is also more flexible: the same protocol can gracefully end a live
capture, tolerate arbitrary backpressure/deadlines, preserve RX FEC lookahead, and
provide a callback-consumed proof without buffering a whole stream.

Valid-frame metadata costs a few integrity-protected transport bits, but it is the
only general way to distinguish codec padding from real trailing zero audio.
`ExactMappedExtent` requires delay compensation from epoch start and finite terminal
computation to expose delayed resampler output; truncation is cheaper but returns
`Aborted`, making content loss visible. `PreserveCausalLatency` supports conventional
live/offline uses while explicitly reporting, rather than silently hiding, the
additional physical duration.

Finally, worker-published and callback-rendered EOS remain different facts. Combining
them would require a worker to block on the hard-RT consumer or would falsely report
a stream rendered while samples remained queued. The two-proof design costs one
fence and one atomic acknowledgement and preserves the callback's existing bounded,
allocation-free character.
