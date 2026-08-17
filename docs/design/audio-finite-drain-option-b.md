# Finite Audio Pipeline (Option B)

## Status

Design proposal. This option adds a separate, deep **offline** `FiniteAudioPipeline` module for complete finite-media capture and playback. It deliberately does not add end-of-input, draining, or finishing states to the live `TxWorker` or `PlaybackWorker`, and it does not change any audio callback contract.

## Decision summary

A caller submits one complete, caller-owned interleaved PCM slice together with an explicit operation configuration and caller-provided workspace. The pipeline synchronously performs a bounded transaction:

- capture: optional sample-rate conversion, exact packet framing, real Opus encoding at 5/10/20 ms, and an exact trim/accounting report;
- playback: bounded packet decode, optional sample-rate conversion, delay removal, and exact trimming into caller-provided rendered-PCM storage.

The module reuses the same public encoder, decoder, resampler, packet-duration, and format primitives used by live audio. It owns only finite-operation orchestration and accounting. All capacities are planned before execution; steady execution performs no allocation and publishes no live/scheduled clock state.

## Non-goals

- Changing `TxWorker`, `PlaybackWorker`, or callback state machines.
- Teaching a realtime worker to recognize EOF.
- Flushing finite content by injecting silence into a live queue.
- Reusing or advancing the scheduled/live transport clock.
- Hiding truncation, padding, PLC, or output-capacity exhaustion.

## Proposed public shape (illustrative)

```rust
pub struct FiniteAudioPipeline;

impl FiniteAudioPipeline {
    pub fn plan_capture(config: &FiniteCaptureConfig, input_frames: usize)
        -> Result<FiniteCapturePlan, FinitePlanError>;

    pub fn capture<'a>(
        plan: &FiniteCapturePlan,
        input: FinitePcm<'a>,
        workspace: FiniteWorkspace<'a>,
        packet_storage: &'a mut [u8],
        packet_index: &'a mut [FinitePacket],
    ) -> Result<FiniteCaptureOutput<'a>, FiniteRunError>;

    pub fn plan_playback(
        config: &FinitePlaybackConfig,
        packets: &[FinitePacketRef<'_>],
    ) -> Result<FinitePlaybackPlan, FinitePlanError>;

    pub fn playback<'a>(
        plan: &FinitePlaybackPlan,
        packets: &[FinitePacketRef<'_>],
        workspace: FiniteWorkspace<'a>,
        rendered_interleaved: &'a mut [f32],
    ) -> Result<FinitePlaybackOutput<'a>, FiniteRunError>;
}
```

The final signatures must be adapted to the repository's existing public codec/resampler types; the semantic boundary is the important part: one finite input, one preflight plan, one synchronous bounded execution, and a returned exact report.

## Core invariants

1. Success means every logical input frame was accepted and all algorithmic tail needed to reconstruct it was drained.
2. Padding required by Opus packet framing is encoded and reported, never mistaken for source content.
3. Playback removes declared leading delay and terminal padding exactly once.
4. No finite operation observes, advances, or resets the scheduled transport clock.
5. Capacity failure is detected during planning where derivable, otherwise execution is transactional with an explicit error and no falsely successful report.
6. A report distinguishes source frames, generated/drained frames, packet padding, decoded frames, resampler delay/tail, trimmed frames, and returned frames.
7. The live shutdown design remains unchanged: its approved stop/retire semantics are not a substitute for finite EOF, and this module does not call into worker shutdown paths.

## Open repository-integration questions

The detailed design below will pin these concepts to the actual codec, resampler, packet, clock, and error APIs in this repository, including ownership and visibility constraints.


## Concrete Option B API: a separate finite/offline pipeline

Option B is deliberately **not** a new mode on the live callback pipeline. It is a
separate, caller-driven module for inputs whose end is already known. The public
surface owns no hidden heap-backed scratch state and does not expose codec or
resampler implementation details.

```rust
/// Immutable construction-time limits. Validation may allocate; processing does not.
pub struct FiniteAudioConfig {
    pub input_sample_rate_hz: u32,
    pub output_sample_rate_hz: u32,
    pub channels: NonZeroU16,
    pub opus_frame_duration: OpusFrameDuration, // 5, 10, or 20 ms
    pub max_input_frames_per_push: NonZeroU32,
    pub decode_policy: FiniteDecodePolicy,
}

/// Caller-owned memory, sized once from a validated configuration.
/// Fields are private so the implementation may change its SRC/filter/codec layout.
pub struct FiniteAudioWorkspace {
    resampler: ResamplerWorkspace,
    packetizer: PacketizerWorkspace,
    encoder: OpusEncoderWorkspace,
    decoder: Option<OpusDecoderWorkspace>,
    playback: PlaybackWorkspace,
}

/// Exact sizes and alignment requirements; contains no pointers into a workspace.
pub struct FiniteAudioWorkspacePlan {
    pub input_samples: SampleRegion,
    pub resampler_samples: SampleRegion,
    pub packet_samples: SampleRegion,
    pub encoded_bytes: ByteRegion,
    pub decoded_samples: SampleRegion,
    pub playback_samples: SampleRegion,
    pub total_bytes: usize,
    pub alignment: usize,
}

pub struct SampleRegion { pub samples: usize, pub alignment: usize }
pub struct ByteRegion { pub bytes: usize, pub alignment: usize }

impl FiniteAudioWorkspacePlan {
    pub fn for_config(config: &FiniteAudioConfig)
        -> Result<Self, FiniteAudioConfigError>;

    /// Binds caller-owned storage. Failure leaves `storage` reusable by the caller.
    pub fn bind<'a>(
        &self,
        storage: &'a mut [MaybeUninit<u8>],
    ) -> Result<FiniteAudioWorkspaceRef<'a>, FiniteAudioWorkspaceError>;
}

pub struct FiniteAudioPipeline<'a> {
    workspace: FiniteAudioWorkspaceRef<'a>,
    state: FiniteState,
    accounting: FiniteAccounting,
    // Private: SRC, packetizer, codec, and playback algorithms.
}

impl<'a> FiniteAudioPipeline<'a> {
    pub fn new(
        config: FiniteAudioConfig,
        workspace: FiniteAudioWorkspaceRef<'a>,
    ) -> Result<Self, FiniteAudioConfigError>;

    /// Consumes all accepted input or reports the precise accepted prefix.
    pub fn push(
        &mut self,
        interleaved: &[f32],
        sink: &mut impl FinitePacketSink,
    ) -> Result<PushReport, FiniteAudioError>;

    /// Declares the true end of input and emits all finite tail packets.
    pub fn finish(
        &mut self,
        sink: &mut impl FinitePacketSink,
    ) -> Result<FiniteFinish, FiniteAudioError>;

    pub fn accounting(&self) -> FiniteAccounting;
}

pub trait FinitePacketSink {
    fn write_packet(&mut self, packet: FinitePacket<'_>)
        -> Result<(), FiniteSinkError>;
}

pub struct FinitePacket<'a> {
    pub bytes: &'a [u8],
    pub sequence: u64,
    pub media_start_48k: u64,
    pub valid_media_frames_48k: u32,
    pub encoded_frames_48k: u32,
    pub end_padding_48k: u32,
}
```

Typical ownership and use are explicit:

```rust
let plan = FiniteAudioWorkspacePlan::for_config(&config)?;
let mut storage = vec![MaybeUninit::<u8>::uninit(); plan.total_bytes]; // off RT
let workspace = plan.bind(&mut storage)?;
let mut finite = FiniteAudioPipeline::new(config, workspace)?;

for block in source.blocks() {
    finite.push(block, &mut packet_file)?;
}
let result = finite.finish(&mut packet_file)?;
assert_eq!(result.accounting.accepted_input_frames, source.frame_count());
```

The illustrative `Vec` belongs to the caller and is optional: a fixed array, arena,
shared-memory mapping, or suitably aligned slab may be used instead. `push` and
`finish` perform no allocation, locking, thread creation, file I/O, or wall-clock
queries. `FiniteAudioWorkspaceRef`, codec handles, filter phases, delay lines, and
packet assembly cursors remain private; callers cannot couple themselves to the
chosen SRC or Opus wrapper.


## Offline decode and playback surface

The matching read side is deliberately finite and caller-driven. It does not own a
playback ring or device callback:

```rust
pub struct FiniteDecodeInput<'a> {
    pub packet: &'a [u8],
    pub sequence: u64,
    pub media_start_48k: u64,
    pub valid_media_frames_48k: u32,
    pub encoded_frames_48k: u32,
    pub end_padding_48k: u32,
}

pub struct FiniteRenderReport {
    pub packets_decoded: u64,
    pub valid_media_frames_48k: u64,
    pub omitted_padding_frames_48k: u64,
    pub output_frames: u64,
    pub leading_trim_frames: usize,
    pub trailing_trim_frames: usize,
}

impl<'w> FiniteAudioPipeline<'w> {
    pub fn decode_packet(
        &mut self,
        input: FiniteDecodeInput<'_>,
        output: &mut impl FinitePcmSink,
    ) -> Result<(), FiniteAudioError>;

    pub fn finish_decode(
        &mut self,
        output: &mut impl FinitePcmSink,
    ) -> Result<FiniteRenderReport, FiniteAudioError>;
}
```

The decoder validates consecutive extended sequence/media positions and requires the
final packet's `valid + end_padding == encoded`. It decodes each complete Opus frame,
omits only manifest-declared padding, and feeds source-valid 48 kHz PCM into a private
finite fixed-ratio converter. `finish_decode` pumps and trims that converter's filter
tail. It never guesses padding by looking for zero samples. For the no-SRC 48 kHz case
it copies exactly the valid prefix. The output count is the checked rational mapping
selected by the finite rounding policy in the plan.

This is not adaptive playout. No packet-arrival timestamp, wall clock, delivery jitter,
or `NetworkTime` is accepted. If a caller wants a scheduled-clock diagnostic, it may
supply a separate sequence of `(extended_media_position, scheduled_local_frames)` to
a read-only verifier that uses the production drift estimator; those observations do
not alter offline sample conversion or determine when the finite tail ends.

## Lifecycle, error, and accounting rules

Both directions use `Ready -> Active -> Finished` with sticky `Faulted`. `finish` is
idempotent only after a successful finish: it returns the same immutable report and
emits no bytes. A push after finish is `AlreadyFinished`. Validation and insufficient
sink/workspace capacity fail before consuming input. A codec or sink error after a
committed packet reports exact committed source frames and packets; retry begins only
at the first uncommitted item. No error is represented as silence or a successful
short result.

`FiniteAccounting` uses checked `u64` frame counters and names every term:

```text
accepted source frames
 -> fixed-SRC generated / leading trim / trailing trim
 -> valid media frames + explicit Opus padding
 -> decoded valid media frames + omitted padding
 -> playback-SRC generated / leading trim / trailing trim
 -> emitted destination frames
```

For every successful run, valid TX media frames equal valid RX media frames. The
encoded duration may exceed valid media only by the declared final padding. Sequence
and RTP timestamp reduction occurs only after checked extended addition; wrap counts
remain observable. `MissingAtDeadline` does not appear in this offline contract:
missing input is a structural finite-file error rather than asserted network loss.

## Required tests

The option is accepted only with deterministic tests for all 5/10/20 ms Opus frames,
all supported capture and destination rates, aligned and one-frame-short source ends,
zero length, and both exact-required and zero-pad policies. Tests must prove:

* source prefix preservation through fixed-SRC trim accounting;
* final `valid + padding == encoded` and no interpretation of trailing source zeroes;
* full packet, sequence, timestamp and wrap accounting;
* finite decode output count and leading/trailing trim at 44.1, 48, 96 and 192 kHz;
* capacity rejection with zero progress, sticky codec/sink failures, finish replay,
  and rejection of malformed manifests or discontinuities;
* no heap allocation during `push`, `finish`, `decode_packet`, or `finish_decode` after
  construction; and
* differential comparison with a whole-buffer offline oracle within the documented
  lossy-codec tolerance, without claiming sample identity.

Each duration requires a non-integral capture-rate case so capture SRC delay/padding is
actually exercised. The test report separates codec frames, valid media frames, and
physical destination frames; it never calls an `InbandFecOrPlc` attempt confirmed FEC.

## Complexity, memory, and allocation

Runtime is linear in accepted samples plus codec and filter work:
`O(N * channels * taps)`. The bound workspace contains codec state, fixed filter delay,
one source block, one Opus PCM frame, one maximum packet, one decoded frame, and one
output quantum. It is independent of total stream duration so long as the sink accepts
incremental output. Construction and the caller's optional arena allocation occur off
the realtime thread. Processing performs no allocation, lock, wait, logging, I/O, or
thread creation; sink implementations are separately responsible for their behavior.

## Trade-offs and explicit non-selection

Option B is a deep, useful offline module and a strong oracle for finite accounting. It
also keeps finite-only types out of `TxWorker`, `RxWorker`, `PlaybackWorker`, and the
callback ring. That isolation is its main advantage.

It does **not** by itself satisfy the approved live shutdown gate. It cannot finish an
already-running `TxWorker`, resolve its committed queues, drain the production
one-frame-lookahead RX state, flush the live adaptive playback converter, retain a
ring-blocked tail, publish a terminal fence, or acknowledge that the callback consumed
the last frame. Switching from a live worker to this offline object would duplicate
codec/SRC state and would not be phase-continuous. Therefore Option B may be implemented
later as a file renderer or differential oracle, but it must not be described as the
finite end operation for the current live pipeline.
