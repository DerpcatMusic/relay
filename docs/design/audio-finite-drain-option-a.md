# Audio finite drain — option A: three explicit end operations

## Decision summary

Option A is the smallest change that can make the existing finite path truthful:

1. **Keep** `FiniteTxWorker::process_finite` as the TX end operation, but make an incomplete-frame rejection a real zero-progress rejection rather than a silent discard.
2. Add one allocation-free, terminal `AdaptiveClockConverter::finish_interleaved` operation in `relay-resample`.
3. Add one stateful `PlaybackWorker::finish_finite` operation in `relay-audio`. It consumes the final frame returned by the existing `RxWorker::drain`, invokes the converter finish once, and incrementally publishes retained output when the playback ring has space.

A sizing query accompanies the adaptive finish, but it is not a lifecycle transition. There is no `finish` method on `WorkerResampler`, no change to `TxWorker::process_capture`, and no change to `PlaybackRenderer::render`. Calling none of these finite operations preserves today's live semantics exactly.

This option is intentionally a finite/offline or lab contract. It is **not** a new meaning for live disconnect. Construction and finishing run on worker/control threads. The device callback remains a bounded ring read plus zero fill.

“Complete and delay-compensated” here has the same boundary as the current local composition design: all declared capture frames survive the fixed capture SRC's leading/tail delay, all declared decoded media frames survive the adaptive playback SRC's leading/tail delay, and every omitted/padded frame is counted. It does not claim waveform identity through lossy Opus or invent an Opus container pre-skip contract that `relay-opus` does not currently expose. If product acceptance also requires libopus encoder-lookahead/pre-skip compensation, that must be designed and gated explicitly in `relay-opus`; this option must not overclaim it from SRC trim counts.

## Why this is the minimal seam

The repository already has most of the TX answer:

- `FiniteFixedRatioConverter` resets, accepts a partial final fixed-SRC chunk, pumps Rubato with `partial_len(0)`, and reports exact leading/trailing trim.
- `FiniteTxWorker` owns a one-shot `Ready -> Completed/Faulted` lifecycle, preflights `PacketBatch`, and packetizes only full Opus frames.
- `RxWorker::drain` already resolves the one pending FEC/lookahead decision without inventing another deadline.

The missing operation is below `PlaybackWorker`: `AdaptiveClockConverter` is currently live-only and retains its sinc history. Adding a general stream/session abstraction would duplicate the existing worker states and broaden the live API. Option A instead adds terminal operations only where state is already owned.

## Public Rust surface

Names are proposed; field units are normative. Every frame count is **per channel**. Slice lengths remain scalar interleaved samples.

### 1. Existing TX operation, with strict incomplete-frame semantics

```rust
pub enum FinalFramePolicy {
    /// Encode one complete Opus frame padded with zero PCM and report how much
    /// of that packet is source-valid.
    ZeroPad,

    /// Require the delay-compensated 48 kHz source length to be an exact
    /// multiple of the negotiated Opus frame size.
    RequireComplete,
}

impl FiniteTxWorker {
    pub fn process_finite(
        &mut self,
        input: &[f32],
        policy: FinalFramePolicy,
        batch: &mut PacketBatch,
    ) -> Result<FiniteTxReport, FiniteTxError>;
}
```

`RequireComplete` replaces the current misleading `RejectIncomplete` behavior. If a remainder exists, the operation returns

```rust
TxError::IncompleteFinalOpusFrame {
    valid_media_frames: usize,
    packet_frames: usize,
}
```

with `input_frames_consumed == 0`, `packets_emitted == 0`, an unchanged timeline, an empty batch, and `FiniteState::Ready`. It does **not** convert the input, emit the complete prefix, or discard the remainder.

With `ZeroPad`, the final Opus call still receives exactly the negotiated 5/10/20 ms frame. `FiniteTxReport` remains the authoritative out-of-band finite manifest:

```rust
pub struct FiniteTxReport {
    pub resampler: FiniteProcessReport,
    pub packets_emitted: usize,
    pub final_valid_media_frames: usize,
    pub zero_padded_media_frames: usize,
    pub batch_full: bool,
    pub required_batch_capacity: usize,
}
```

For an aligned end, both final-frame fields are zero; all emitted packets are fully valid. For an unaligned padded end,

```text
final_valid_media_frames + zero_padded_media_frames == opus_packet_frames
```

and `final_valid_media_frames > 0`. A zero-length source emits no packet.

The valid count is not currently an RTP field. The finite owner must retain it beside the `PacketBatch` and supply it at RX finish. If that side metadata cannot be carried reliably, `RequireComplete` is mandatory. Guessing the count from a decoded packet or inspecting trailing silence is forbidden.

### 2. Adaptive converter sizing and terminal finish

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveFinishRequirements {
    pub channels: usize,
    /// Normal full input transaction expected for `final_input`.
    pub final_input_frames: usize,
    /// Caller output capacity that is sufficient at every admitted ratio/phase.
    pub output_workspace_frames: usize,
    /// Initial startup delay already present at the head of prior streaming output.
    pub leading_trim_frames: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveFinishReport {
    /// Source-valid frames consumed from the final full transaction.
    pub valid_input_frames: usize,
    /// Raw frames written, including final backend overshoot.
    pub generated_output_frames: usize,
    /// Valid finish frames in `output[..output_frames * channels]`.
    pub output_frames: usize,
    /// Initial raw frames to remove from the head of the complete collected stream.
    pub leading_trim_frames: usize,
    /// Raw zero-pump overshoot omitted from `output_frames`.
    pub trailing_trim_frames: usize,
    /// Always zero after this one-shot low-level success.
    pub pending_output_frames: usize,
}

impl AdaptiveClockConverter {
    /// Pure checked sizing query; no ratio, filter, or lifecycle mutation.
    pub fn finish_requirements(&self) -> Result<AdaptiveFinishRequirements, ResampleError>;

    /// Worker-only terminal operation. `final_input` is one normal full input
    /// transaction; only its first `valid_input_frames` are source-valid.
    pub fn finish_interleaved(
        &mut self,
        final_input: &[f32],
        valid_input_frames: usize,
        output: &mut [f32],
    ) -> Result<AdaptiveFinishReport, ResampleError>;
}
```

The useful finish result is already a prefix, so it needs no allocation or compaction. Across the entire adaptive stream, if `streaming_generated_frames` is the sum of earlier `ProcessReport::output_frames`, the complete useful range is:

```text
raw complete stream:
    earlier outputs || output[..generated_output_frames]

useful frame range:
    leading_trim_frames
        .. streaming_generated_frames
         + generated_output_frames
         - trailing_trim_frames
```

Equivalently, the high-level path publishes only `output_frames` from the finish workspace and reports the leading trim that occurred in previously published startup output.

`finish_interleaved` accepts `1..=final_input_frames` valid frames. A no-media stream is handled by the high-level `Empty` end below without manufacturing a decoder or SRC transaction. Passing zero with a nonempty final transaction is invalid.

### 3. Playback end after RX lookahead

```rust
pub struct FinitePlaybackInput<'a> {
    /// The last decoded frame returned by `RxWorker::drain()`.
    pub frame: &'a PcmFrame,
    /// Full packet frames, or `FiniteTxReport::final_valid_media_frames`
    /// for a zero-padded last TX packet.
    pub valid_media_frames: usize,
    pub remote_media_sample_position: ExtendedTimestamp,
    pub scheduled_local_device_frame: u64,
}

pub enum FinitePlaybackEnd<'a> {
    /// First finish call after RX lookahead was drained.
    Final(FinitePlaybackInput<'a>),
    /// A genuinely empty finite receive stream.
    Empty,
    /// Retry publication of finish output retained after ring backpressure.
    Continue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackFinishStatus {
    /// Valid finish output remains in the worker because the ring had no room.
    PendingRing,
    /// Every valid adaptive-tail frame has been published to the ring.
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackFinishReport {
    pub status: PlaybackFinishStatus,
    /// Valid final 48 kHz input accepted on this call; zero on `Continue`.
    pub input_frames_consumed: usize,
    /// Raw adaptive finish output generated on this call; zero on `Continue`.
    pub generated_output_frames: usize,
    /// Valid device frames published on this call.
    pub published_output_frames: usize,
    /// Initial adaptive delay at the head of the complete collected stream.
    pub leading_trim_frames: usize,
    /// Raw finish overshoot not published.
    pub trailing_trim_frames: usize,
    /// Valid device frames retained but not yet published.
    pub pending_output_frames: usize,
    /// Previously and newly published frames still queued for the renderer.
    pub queued_playback_frames: usize,
}

impl PlaybackWorker {
    pub fn finish_finite(
        &mut self,
        end: FinitePlaybackEnd<'_>,
    ) -> Result<PlaybackFinishReport, PlaybackFinishError>;
}
```

The enum keeps the lifecycle to one high-level method without borrowing the final `PcmFrame` across calls. The first call copies/converts everything it needs into construction-time storage. `Continue` never needs the frame again.

A successful `Final` may return `PendingRing`; that is not a drop. The worker retains the unpublished suffix and the caller lets the unchanged callback consume the ring before calling `Continue`. Each call performs at most one bounded ring write. When `Finished` is returned, adaptive state has no pending tail, but `queued_playback_frames` may still be nonzero. Shutdown still waits for the callback to drain that queue, then stops/detaches the callback and obtains host acknowledgement before dropping endpoints off-thread.

## Buffer ownership and bounds

There are two deliberately different ownership layers:

- Direct `relay-resample` callers own the `output: &mut [f32]` passed to `finish_interleaved` and allocate it from `finish_requirements` before entering the terminal operation.
- `relay-audio` owns that same kind of slice inside `PlaybackWorker` because it must retain unpublished samples across `PendingRing` returns. The memory is allocated only by `playback_pair` on its caller's worker/control thread, from the same authoritative requirement; it is fixed thereafter. The playback ring and `PacketBatch` are likewise construction-time, caller-sized bounded stores.

Thus no finish operation allocates, and no borrowed buffer is secretly retained. “Caller-owned” at the low-level boundary does not force the high-level API to keep a caller borrow alive across callback-driven backpressure.

## Required usage

```rust
// TX: worker/control thread.
let mut tx = FiniteTxWorker::new(pipeline, tx_stream, capture_frames_max)?;
let mut packets = PacketBatch::new(precomputed_packet_bound)?;
let tx_end = tx.process_finite(
    capture_interleaved,
    FinalFramePolicy::ZeroPad,
    &mut packets,
)?;
assert!(!tx_end.batch_full);

// Deliver, ingress, tick, and process every earlier RX outcome normally.
// Every earlier PlaybackPublication must be Published for a complete finite run.

// RX has one pending decision. Do not process the result with process_frame.
let last = rx.drain().ok_or(FiniteRunError::MissingFinalLookahead)?;
let valid = if tx_end.zero_padded_media_frames == 0 {
    last.frame().samples_per_channel()
} else {
    tx_end.final_valid_media_frames
};

let mut finish = playback.finish_finite(FinitePlaybackEnd::Final(
    FinitePlaybackInput {
        frame: last.frame(),
        valid_media_frames: valid,
        remote_media_sample_position: extend(last.timestamp()),
        scheduled_local_device_frame: schedule(last.sequence()),
    },
))?;

while finish.status == PlaybackFinishStatus::PendingRing {
    // The normal device callback drains the ring; no callback lifecycle change.
    finish = playback.finish_finite(FinitePlaybackEnd::Continue)?;
}

// Offline/lab collection removes exactly finish.leading_trim_frames from the
// beginning. The finish operation already withheld trailing_trim_frames.
// Then allow queued_playback_frames to render, stop+ack the callback, and drop
// worker/renderer/ring handles on a control thread.
```

For an empty finite stream, require `rx.drain() == None` and call `FinitePlaybackEnd::Empty`. Supplying `Empty` after any adaptive input, `Final` twice, `Continue` before `Final/Empty`, or ordinary `process_frame` once finishing began is a lifecycle error.

The caller is responsible for the RX-before-SRC ordering because `RxWorker` and `PlaybackWorker` deliberately remain separate modules. Option A does not add a cross-worker token. Tests and finite orchestration must make a missing final RX outcome fatal, not conditional.

## Hidden implementation

### Fixed TX remains separate from live TX

Do not add finite drain to `TxWorker` or `WorkerResampler`. `TxWorker::Disconnected` continues to emit complete accumulated packets, discard a partial media frame, and abandon streaming SRC history exactly as today. Complete finite capture continues through `FiniteTxWorker` and `FiniteFixedRatioConverter` only.

Before conversion, `FiniteTxWorker::process_finite` computes:

```text
useful_media_frames = ceil(capture_frames * 48_000 / capture_rate)
full_packets        = useful_media_frames / opus_packet_frames
remainder           = useful_media_frames % opus_packet_frames
```

It preflights batch capacity and `RequireComplete` before advancing the converter, encoder, or timeline. Under `ZeroPad`, one final PCM buffer is cleared, the valid remainder is copied, and Opus encodes the complete negotiated frame. There is never a short Opus call or a short-duration packet.

### Adaptive state

Add private terminal state to `AdaptiveClockConverter`:

```rust
enum AdaptiveState { Active, Finished, FinishFaulted }

struct AdaptiveClockConverter {
    // existing fields ...
    configured_chunk_frames: usize,
    initial_output_delay_frames: usize,
    saw_input: bool,
    state: AdaptiveState,
}
```

Normal `process_interleaved` behavior is unchanged while `Active`; its first successful call records that input exists. In particular, adding finite finish does not newly fault the low-level live converter after an ordinary processing error. `reset` restores `Active`, the nominal ratio/correction, `saw_input = false`, and the construction-time initial delay. A normal process after successful finish returns `ResampleError::EndOfStream`. A backend/nonfinite-output failure during finish moves to `FinishFaulted`, because retrying a partially advanced terminal pump would make the boundary ambiguous. Slice length, valid-count, nonfinite input, arithmetic, and capacity failures are validated before mutation and leave `Active` so the caller may correct and retry.

### Processing a valid prefix without treating TX pad as media

The decoded last packet is a full `PcmFrame`, but only a prefix may belong to the finite source. Do not feed the decoded padding suffix and later attempt to detect silence. Hidden finish logic uses Rubato's pinned `Resizable::set_chunk_size(valid_input_frames)` on the fixed-input async converter, processes exactly the valid prefix, and then restores the configured full chunk size for the zero pump. This preserves the backend's fractional phase and produces the correct amount of output for the valid prefix. It is preferable to `Indexing::partial_len(valid)` here because that call generates a full-chunk output containing an endpoint that would then need unavailable phase introspection to trim exactly.

The final valid transaction advances the existing one-pole correction once, using its actual valid duration. Finish then freezes the reached Rubato ratio: injected zeros do not advance the controller smoother, estimator, or recovery controller and do not create scheduled clock observations.

After the valid prefix:

1. capture the final `output_delay()`;
2. restore the configured chunk size;
3. call `process_into_buffer(..., Indexing::new().partial_len(0))` into successive output offsets;
4. stop once raw post-end output covers the captured final delay;
5. expose the prefix ending at that delay and report the last block's excess as `trailing_trim_frames`.

Rubato reports its required input size even for `partial_len(0)`; RELAY must report injected zero input separately and must never count it as `valid_input_frames`.

`finish_requirements` is a conservative checked bound derived at construction from the configured sinc length, full chunk, maximum allowed ratio, `output_frames_max`, and one phase-margin block. It covers the final resized transaction plus enough full zero-input transactions to move at least half the sinc support past the endpoint. It must not rely on sample energy becoming small. If the backend reaches the bound without covering the captured delay, return `ResampleError::Backend` and fault rather than truncate.

For the current default 256-tap sinc and minimum 5 ms/240-frame media chunk, one full zero-input transaction covers the half-filter support at every supported playback rate. The implementation should nevertheless retain the derived general bound; no magic “two calls” contract belongs in the public API.

### Playback retention and publication

`playback_pair` allocates a finish workspace to `AdaptiveFinishRequirements::output_workspace_frames * channels` on the constructing thread. The size is checked and reported in `PlaybackBuildError` on overflow/allocation failure. `PlaybackWorker` adds:

```rust
enum PlaybackWorkerState { Running, Finishing, Finished, Faulted }

finish_generated_frames: usize,
finish_valid_frames: usize,
finish_cursor_samples: usize,
finish_leading_trim_frames: usize,
finish_trailing_trim_frames: usize,
finite_integrity: bool,
```

The storage is fixed for the pair lifetime. The caller chooses the ring and pipeline bounds at construction; no finish call grows either allocation.

Before beginning finish, `finite_integrity` must still be true. Any earlier `DroppedFull`, `RendererDisconnected`, converter fault, or discontinuity makes a later claim of complete finite playback invalid and `finish_finite(Final(..))` returns `PriorPlaybackLoss`. This check does not alter live drop behavior; it only refuses to relabel a lossy live history as complete.

After low-level finish succeeds, only its valid prefix is retained for publication. On each finish call, choose the largest whole-frame prefix not exceeding both unpublished samples and `producer.available_samples()`, then call the existing all-or-drop `write` once. Because the sole consumer can only free slots, the preflight cannot become too small; an unexpected `DroppedFull` is treated as an internal/stateful fault, never as successfully pending output. An observed disconnected renderer returns an error carrying exact generated, published, and discarded pending counts.

Do not call `update_controller_if_due` during tail publication. There is no new scheduled media observation, and a correction would affect only synthetic zeros. The report repeats the final trim totals on every `Continue` so a caller does not need to retain the first report merely to trim the collected stream.

### Callback contract

`PlaybackRenderer::render` is byte-for-byte/API-for-API unchanged:

- fill output with zero;
- perform one bounded ring read;
- report complete/underrun/disconnected/misaligned;
- no allocation, drop of heap-owning payload, lock, wait, log, syscall, codec, SRC, or unbounded loop.

Finish generation, report construction, retries, and endpoint destruction remain off the callback.

## State and error matrix

| Operation | Valid state | Success | Recoverable rejection | Stateful failure |
|---|---|---|---|---|
| `FiniteTxWorker::process_finite` | `Ready` | `Completed` | bad input, small batch, incomplete under `RequireComplete`: stays `Ready`, zero progress | backend/codec/packet ambiguity: `Faulted` with committed counts |
| adaptive normal `process_interleaved` | `Active` | `Active` | prevalidation: `Active` | existing live error behavior is unchanged |
| `AdaptiveClockConverter::finish_interleaved` | `Active` | `Finished` | bad final length/count/input/workspace: `Active` | backend/output/bound breach: `FinishFaulted` |
| `PlaybackWorker::finish_finite(Final/Empty)` | `Running` | `Finishing` or `Finished` | invalid end/valid count/prior loss: `Running` | converter/publication ambiguity: `Faulted` |
| `PlaybackWorker::finish_finite(Continue)` | `Finishing` | `Finishing` or `Finished` | no ring slots: `Finishing`, zero progress | disconnected/unexpected drop: `Faulted` |
| normal `process_frame` | `Finishing`/`Finished` | — | lifecycle error, no mutation | — |
| `reset_when_empty` | `Finished`/`Faulted`, empty ring | `Running` with all finite state cleared | nonempty ring: unchanged | existing reset rules |

`PlaybackFinishError` should contain a `cause` plus a `PlaybackFinishReport`-shaped progress snapshot. This mirrors TX's explicit committed-progress design. Suggested causes are `InvalidTransition`, `InvalidValidMediaFrames`, `MissingFinalFrame`, `PriorPlaybackLoss`, `Clock`, `Resampler`, `RendererDisconnected`, and `PublicationInvariant`.

A completed finish is terminal until reset. Repeating `Continue` after `Finished` may return the same zero-pending `Finished` report idempotently; supplying another `Final` is always an error.

## Accounting invariants

### TX fixed SRC

For `N` capture frames and rates `Ri -> 48_000`:

```text
M = ceil(N * 48_000 / Ri)
generated - leading_trim - trailing_trim == M
```

Only that valid range is packetized. Under `ZeroPad`:

```text
packets * opus_packet_frames == M + zero_padded_media_frames
final_valid_media_frames == M % opus_packet_frames  (when remainder != 0)
```

Under `RequireComplete`, success requires `M % opus_packet_frames == 0`.

### RX and adaptive playback

The RX end contributes exactly one existing pending lookahead outcome or explicitly proves the stream empty. No extra `tick` is invented.

Let:

- `S` be device frames generated by earlier adaptive calls,
- `G` be raw frames generated by adaptive finish,
- `L` be reported initial leading trim,
- `T` be reported final trailing trim.

Then the complete finite adaptive output contains exactly:

```text
S + G - L - T
```

useful device frames. The last padded TX suffix is excluded because only `valid_media_frames` from the decoded final packet enters the final adaptive transaction. With a time-varying correction, this measured phase-aware count is authoritative; do not replace it with `ceil(total_media_frames * nominal_ratio)`.

At any high-level return:

```text
finish_valid_frames == published_finish_frames_total + pending_output_frames
pending_output_frames == 0  iff status == Finished
queued_playback_frames == ring_readable_samples / channels
```

Earlier ordinary playback publications must all be `Published`. Ring drops, concealment, and source padding are separate facts; none may be hidden inside “SRC trim.”

## Test plan

### `relay-resample`

1. All 48 kHz-to-{44.1, 48, 96, 192 kHz} pairs, 5/10/20 ms chunks, and correction extrema: process several normal chunks, finish with every valid-prefix boundary `1`, `packet-1`, and `packet`.
2. First-sample and last-valid-sample impulses: concatenate raw earlier and finish output, apply the reported range, and prove both boundaries remain present with bounded gain/area.
3. Alternating correction targets and ramp history before finish: exact deterministic counts, finite output, and no correction/smoother advance during zero pumping.
4. Stereo channel isolation and DC/passband gain after trim.
5. Chunk sizes below half sinc support, including one frame, to prove the derived multi-pump bound rather than the current one-block accident.
6. Transactional validation: wrong full input length, zero/too-large valid count, nonfinite valid prefix, odd interleaving, undersized workspace, and checked overflow leave state and ratio unchanged.
7. Lifecycle: normal after finish rejected, second finish explicit/idempotent as specified, reset restores byte-for-byte deterministic output.
8. Pointer/capacity identity around long processing and finish; source-audit/pinned-backend evidence that `set_chunk_size` and `process_into_buffer` do not allocate.

### `relay-audio` TX

1. Every capture rate and packet duration with non-chunk- and non-packet-aligned source lengths.
2. `ZeroPad` emits no short packet and reports exact valid+padded equality.
3. `RequireComplete` remainder returns zero progress, no packets, unchanged timeline/state; retry with `ZeroPad` succeeds.
4. Exact-aligned `RequireComplete`, empty source, small `PacketBatch`, repeat, codec error, and timeline wrap.
5. Decode emitted packets and use the manifest valid count; never infer it from decoded sample values.

### RX/playback integration

1. Make the final `RxWorker::drain()` outcome mandatory, pass it to `finish_finite(Final(..))`, and assert its sequence/timestamp is the final scheduled position.
2. Pre-fill the playback ring so finish returns `PendingRing`; interleave ordinary renderer calls and `Continue` until `Finished`. Assert FIFO, zero `dropped_samples`, exact pending monotonic decrease, and final empty ring.
3. Collect the complete raw playback, apply leading trim, and prove first/last impulses, finite nontrivial stereo, and the `S + G - L - T` identity for all supported playback rates.
4. Padded last TX packet with valid counts `1`, `packet-1`; prove the output duration follows valid media, not the negotiated full packet or decoded padding suffix.
5. Complete-aligned TX, empty stream, clean lossless loopback, deterministic checksum, and repeated run equality.
6. Wrong ordering (`Continue` first, `Final` twice, processing drained RX normally then trying `Empty`), bad valid count, earlier ring drop, renderer disconnect, reset with nonempty ring, reset after drain.
7. Existing live loopback/media tests run unchanged and retain their current streaming counts/checksums when no finite method is called.
8. Allocation counter around repeated `PlaybackRenderer::render`; callback source audit remains unchanged. Worker finish workspace pointer/capacity stays stable.

The 60-second gate should report TX fixed-SRC leading/trailing trim, final Opus valid/pad count, final RX lookahead identity, adaptive leading/trailing trim, and pending-to-zero progression separately. A checksum alone is not a drain oracle.

## Complexity and memory

- TX finite conversion: existing FFT block complexity, linear input/output validation and packetization; no processing allocation. `PacketBatch`, fixed-SRC workspace, encoder PCM, and encoded bytes are construction-time fixed storage.
- Adaptive finish: one resized valid transaction plus a construction-bounded number of zero-input sinc transactions. Work is `O((valid_final_frames + filter_support) * channels * sinc_len)` for the sinc backend, with constants fixed by construction. There is no energy-dependent or unbounded drain loop.
- Playback publication: `O(k * channels)` copy for the frames published by that call; at most one ring write and no waiting/retry inside the method.
- Additional memory per playback worker: `finish_requirements.output_workspace_frames * channels * size_of::<f32>()` plus scalar state. With current packet sizes/default sinc this is approximately two normal maximum output blocks, but callers must use the authoritative checked requirement, not that approximation.
- Callback memory and complexity: unchanged `O(requested_samples)` zero-fill/copy, fixed ring storage, no allocation.

All sample/frame/byte multiplications and bound sums use checked arithmetic. Allocation failure is a construction error. No destructor for finish storage runs on the callback.

## Tradeoffs and rejected alternatives

### Advantages

- Three lifecycle operations total, only two of them new.
- Reuses the already reviewed finite fixed converter and RX lookahead drain.
- Does not weaken or overload live disconnect behavior.
- Makes partial Opus handling and adaptive endpoint accounting explicit.
- Bounded backpressure retains tail instead of silently dropping it.
- No callback or `relay-rt` API change.

### Costs

- The final TX valid count is side metadata, not part of `MediaPacket`; finite orchestration must preserve it or require exact packet alignment.
- The caller must enforce `RxWorker::drain` before playback finish. A cross-worker token would be stronger but is a larger API.
- The last decoded frame follows a distinct path: it is passed to `finish_finite`, not `process_frame`.
- `relay-resample` pins a small amount of implementation knowledge about Rubato 4 `Resizable`, fractional phase, delay, and sinc support. Upgrade tests must revalidate it.
- Leading trim may already have been rendered in a genuinely live callback. Reports make it accountable but cannot unplay it; sample-exact trimming is for buffered finite capture/lab output.
- A finish after any earlier playback drop is rejected rather than pretending to be complete.

### Not chosen in option A

- Adding `finish` to `WorkerResampler`: it would falsely imply one EOS contract fits fixed live, adaptive live, and finite conversion.
- Changing `CaptureInput::Disconnected`: live disconnect intentionally abandons converter history and must remain cheap/bounded.
- Padding until output energy is “small”: content-dependent, numerically fragile, and not an exact bound.
- Emitting a short Opus packet: violates the fixed negotiated duration and is rejected by RX duration validation.
- Treating decoded trailing silence as padding metadata: silence can be valid content and Opus output is lossy.
- Resetting the adaptive converter at EOS: drops retained tail and erases the delay needed for accounting.
- Running finish/DSP on `PlaybackRenderer`: violates the hard-realtime boundary.
- Automatically manufacturing another RX deadline: converts an endpoint into PLC and changes finite content.

## Acceptance criteria

Option A is implementable only if focused Rubato tests prove the resized-final-prefix plus bounded-zero-pump accounting for every supported rate, correction extreme, and adversarial chunk size. Acceptance requires:

- exact checked capacity bounds before mutation;
- no short Opus encode/decode transaction;
- a real zero-progress incomplete rejection or explicit valid/pad manifest;
- mandatory RX lookahead resolution before adaptive finish;
- exact leading, trailing, and pending counts;
- no earlier playback drop in a run labeled complete;
- no heap growth in processing/finish and no callback change;
- unchanged live behavior when the finite methods are unused.

If the pinned backend cannot support exact prefix/delay accounting under its exposed phase rules, reject this option rather than weaken the report. The fallback is an offline whole-buffer design, not an energy threshold or silent truncation.
