# RELAY Audio Composition Design

## Status and scope

**Disposition: implementation-ready.** This is the Phase 1 contract for composing `relay-testkit`, `relay-rt`, `relay-clock`, `relay-jitter`, `relay-resample`, and `relay-opus` into the smallest deterministic end-to-end media path. It adds orchestration and typed packet/timeline values; algorithms remain owned by the existing focused crates. Transport security, signaling, sockets, device discovery, and a general RTP stack remain out of scope.

## Existing contracts to preserve

- The hard callback only reads/writes caller buffers through `relay-rt`; it never decodes, resamples, allocates, locks, logs, performs I/O, waits, or drops callback-visible owners.
- Workers own Opus/Rubato state and all reusable buffers. Every queue has a validated maximum and an explicit overflow outcome.
- Media is stereo 48 kHz Opus with one negotiated `FrameDuration` (5/10/20 ms) and DTX disabled. A normal decode must equal that duration.
- `Playout::MissingAtDeadline` requests concealment. It is not automatically network-loss or RTCP loss telemetry.
- Clock observations are created only from scheduled media progression against an extended local device-frame timeline. Socket/packet arrival time never reaches `DriftEstimator` or `ClockRecovery`.
- Positive `ClockRecoveryOutput::ratio_multiplier` correction means a larger output/input ratio. Convert it with `OutputInputRatioCorrectionPpm::from_ratio_multiplier`; never pass raw remote drift to the resampler.

Dependency direction is `relay-audio -> {relay-domain, relay-testkit, relay-rt, relay-clock, relay-jitter, relay-resample, relay-opus}`. Those crates must not depend on `relay-audio`, transport, device, UI, or async-runtime types.

## Minimal public API

`relay-audio` should expose deterministic state machines, not spawn threads:

```rust
pub struct AudioPipelineConfig { /* validated capacities/rates/profile */ }
pub struct Ssrc(u32);
pub struct ExtendedSequence(u64);
pub struct ExtendedTimestamp(u64);
pub struct MediaPacket { /* SSRC, seq, timestamp, payload type, fixed payload storage + len */ }
pub struct TxWorker;
pub struct RxWorker;
pub struct PlaybackRenderer;
pub struct DeterministicNetwork;
pub enum Lifecycle { Created, Priming, Running, Draining, Stopped, Faulted }
```

Construction validates nonzero capacities, stereo/channel alignment, supported capture/playback rates, sequence window `< 32_768`, a packet capacity `<= MAX_PACKET_BYTES`, whole-frame ring sizes, and controller cadence. `TxWorker::process_capture`, `RxWorker::ingest`, `RxWorker::on_playout_tick`, network `enqueue/advance_to`, and renderer `render` are caller-driven and return typed outcomes. No method hides a blocking channel, background task, sleep, or wall clock.

### Typed timeline rules

- Wire sequence is `u16`; accepted packets are extended relative to the locally validated stream epoch. The jitter crate owns modular ordering.
- RTP timestamp is `u32` at 48 kHz and advances by exactly `FrameDuration::samples_per_channel()` per emitted packet, including silence. Extension uses the nearest position in the current SSRC epoch; the exact half range is ambiguous and rejected.
- SSRC change, authenticated sender restart, seek, or validated discontinuity starts a new epoch. Remote input alone never triggers a reset.
- Payload type and negotiated duration are fixed per epoch. Mismatch is rejected before decoder state changes.

## Ownership, capacity, and backpressure

`MediaPacket` uses fixed-capacity inline payload storage plus a length, avoiding per-packet heap allocation and keeping fake-network/reorder memory bounded. Construction preallocates packet slots, resampler work buffers, a 48 kHz TX accumulator, decode/FEC staging frames, adaptive-SRC buffers, and playback rings.

| Boundary | Full/empty policy |
|---|---|
| capture ring -> TX | short read waits for more complete frames; disconnect drains then stops |
| TX accumulator | configuration guarantees worst-case SRC output fits; otherwise fault before overwrite |
| TX -> fake network | reject/drop-new with `network_capacity_drops` |
| fake network -> RX ingress | return owned packet and count ingress overload |
| reorder window | use typed `Accepted`/`Duplicate`/`Late`/`AheadOfWindow`/ambiguous outcomes |
| RX -> playback ring | all-or-drop-new chunk; never wait for callback |
| renderer <- playback ring | copy available samples, zero-fill every missing output sample, count underrun |

No unbounded `Vec`, channel, retry list, or packet history is permitted in steady state. Packet values may be dropped only on worker/control paths, never in the device callback.

## TX worker state machine

1. `Created -> Priming`: allocate/prewarm fixed SRC, Opus encoder, accumulator, and packet storage off callback.
2. Consume caller/capture-ring PCM in the fixed converter's required chunk size. Fixed 48->48 uses zero-delay passthrough.
3. Append finite output to the bounded 48 kHz stereo accumulator.
4. For each complete negotiated frame, encode exactly one Opus packet into fixed storage; attach current SSRC/sequence/timestamp; increment sequence by one and timestamp by the exact frame sample count.
5. Deliver or return a typed backpressure outcome. Packet emission never changes timestamp advancement policy.
6. `Draining`: finish finite SRC input with `FiniteFixedRatioConverter` when the source is finite; an incomplete final Opus frame is either explicitly zero-padded with a recorded valid-frame count for lab use or rejected by live mode. Do not silently emit a short negotiated packet.
7. `Stopped/Faulted`: no more production. Destruction happens after worker/callback acknowledgement.

## Deterministic bounded network

`DeterministicNetwork` owns a fixed number of scheduled slots. A seeded or explicit `NetworkAction` per packet selects deliver, drop, duplicate, or delay. Delivery ordering is `(deliver_at_virtual_time, insertion_ordinal)`; ties are stable. Advancing virtual time returns due packets in bounded batches. Reordering is a consequence of delay, not array iteration order. Duplicate creation consumes another slot; if unavailable it is deterministically rejected and counted. The model records simulated drops separately from RX playout gaps so tests can reconcile truth without teaching production code that every gap is network loss.

## RX worker and one-frame FEC lookahead

The receiver owns bounded ingress, `ReorderBuffer<MediaPacket>`, Opus decoder, one pending-decision slot, adaptive SRC, clock estimator/recovery, and playback producer.

At each caller-scheduled playout tick:

1. Call `pop_at_deadline()` exactly once and associate the decision with the expected extended RTP timestamp and scheduled local device-frame position.
2. Keep a one-frame decode lookahead so in-band FEC is useful. Publish the frame resolved for decision **N-1** while resolving decision **N**.
3. If **N-1** was missing and packet **N** is present, call `decode_fec(packet_N)` for **N-1**, then call `decode(packet_N)` normally and stage **N**. The following packet is not discarded after FEC.
4. If both **N-1** and **N** are missing, call PLC for **N-1** and keep **N** pending. If **N-1** was present, publish its normal decode irrespective of decision **N**.
5. On drain/end, resolve the final pending gap with PLC and publish the final staged frame. Initial priming includes one negotiated frame of lookahead in the latency budget.
6. Convert each resolved 48 kHz frame with adaptive SRC, using only a validated output/input correction derived from `ClockRecoveryOutput::ratio_multiplier`, then perform one all-or-drop write to the playback ring.

Malformed, wrong-duration, wrong-SSRC, wrong-payload-type, duplicate, late, or ambiguous packets do not mutate decoder/clock epochs. A trusted reset calls `ReorderBuffer::reset_and_rebase`, `Decoder` reconstruction off-thread, resampler reset, estimator/recovery reset, clears pending FEC state and staged audio, and re-enters `Priming`.

## Clocking and loss taxonomy

Every playout position—present or missing—advances the expected RTP timestamp. `PlayoutClockObservation::from_scheduled_playout` is built from that extended media position and the extended local device-frame position selected by the playout scheduler, never from fake/real delivery time. Ring-fill error is sampled once at the configured stable worker phase; the controller's filter/deadband handles bounded quantization.

Metrics must keep these facts separate:

- `missing_at_deadline`: concealment decisions;
- `fec_recovered`, `plc_generated`, `malformed`, `late_discarded`, `duplicates`;
- `simulated_network_drops` only in test/network truth;
- later reconciled/confirmed network-loss telemetry in a future transport statistics layer.

A sequence may have one playout gap and one later late discard; consumers must not sum those as two network losses. Repeated post-deadline copies become duplicates within the jitter history horizon.

## Renderer and shutdown

`PlaybackRenderer::render(&mut [f32])` reads available scalar samples, zero-fills the untouched suffix, rejects/handles non-frame-aligned buffers according to validated device contract, and updates primitive counters. It owns only the consumer cursor/staging memory established before start.

Shutdown order is: stop ingress -> drain/stop TX -> deliver or explicitly discard scheduled network slots -> drain RX pending FEC/SRC state -> stop/detach callback with host acknowledgement -> join workers -> drop endpoints/buffers off callback. Starved/full/faulted paths cannot require callback acknowledgement by waiting inside the callback.

## Metrics

Snapshots expose packet counts by rejection class, missing/FEC/PLC counts, network-model truth (test only), encoded/decoded frames, SRC input/output frames, ring dropped/underrun samples, target delay, fill, correction ppm/ratio, all saturation flags, reset reason/count, and lifecycle/fault. Atomic fields are observational, not a coherent transaction; coherent final reports are assembled after stop.

## Deterministic verification matrix

### Full media-path tests

- 48 kHz exact bypass, 44.1->48 capture, and 48->44.1 playback; mono is rejected at the V1 wire boundary.
- 5/10/20 ms fixed-duration packetization, sequence/timestamp wrap, stereo isolation, finite samples, and deterministic encode/decode output.
- Single loss with real FEC, no-FEC PLC fallback, two/three-packet bursts, late/duplicate/reorder/ambiguous inputs, malformed and cross-duration packets.
- Bounded network/ingress/playback overload, starvation zero-fill, trusted restart, drain, repeated start/stop, and endpoint disconnect.
- A short (at least 60 virtual seconds) full Opus/SRC/fake-network loop at each frame duration with reproducible fixture hashes/metrics.

### Twelve-hour exit gate

Run the same jitter/recovery/reorder scheduling logic for 12 virtual hours without sleeping. To keep CI bounded, codec fidelity is separately covered by the full-path tests; the 12-hour test may use valid fixed-duration synthetic decoded frames while retaining real clock, jitter, adaptive-SRC count, bounded ring-fill, wrap, loss and scheduling state machines. Cover remote drift `[-250,-100,-20,0,20,100,250]` ppm, zero-mean jitter, 1-10 ms delay steps/ramps, 0/1/5% loss, deterministic bursts, duplicates and reorder.

Acceptance invariants:

- no panic, non-finite output, allocation growth, unbounded queue, sequence alias, or automatic remote-triggered reset;
- correction and target delay stay within configured bounds; no long-term monotonic latency/fill trend;
- final ring-fill error is within one negotiated frame and maximum fill remains within the configured safe ring margin;
- produced/consumed frame accounting differs only by explicitly reported SRC delay, pending lookahead, concealment, or dropped chunks;
- nominal-clock zero-mean jitter keeps correction within the controller's tested peak/RMS limits;
- results and metric snapshots are bit/deterministically repeatable for a fixed scenario.

## Implementation task breakdown

1. `relay-audio` config/timeline/packet types and boundary tests.
2. TX accumulator, fixed SRC, packetization and bounded network model.
3. RX reorder plus one-frame FEC/PLC decision machine and reset taxonomy.
4. Scheduled-playout clock/adaptive-SRC integration and playback-ring publication.
5. Callback renderer/lifecycle/metrics.
6. Full deterministic media loop, then 12-hour control/media-count soak and Phase 1 evidence.
7. Minimal headless `audio-lab` wrapper only after the core tests pass.

Each task updates its own research evidence, lists plan corrections, and runs locked fmt/check/test/Clippy before hand-off.

## Plan corrections

- The older focused audio plan calls jitter/clock/networking non-goals, but the approved master Phase 1 explicitly includes `relay-clock`, `relay-jitter`, fake-network jitter/loss/drift injection, and a 12-hour exit. The master scope wins and the focused plan status must be updated.
- FEC needs an explicit one-frame lookahead; decoding a missing frame at its first deadline without the following packet can only use PLC.
- Raw packet-arrival time is prohibited from clock recovery; scheduled playout/device-frame progression is the only estimator input.
- The 12-hour CI gate should exercise real control/jitter/SRC accounting but need not run millions of expensive Opus calls; codec fidelity and FEC remain real in shorter full-path tests.
- Phase 1 system-libopus is a local/CI seed. Portable vendoring and artifact license notices remain a release gate.

## Validation commands

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-targets --all-features --locked
cargo test --release -p relay-audio --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Hosted Linux/Windows/macOS execution remains required before Phase 0/1 cross-platform closure.

## Sources

1. RFC 3550, RTP sequence/timestamp and interarrival-jitter semantics: <https://www.rfc-editor.org/rfc/rfc3550>
2. Xiph.Org libopus 1.6 decoder API, PLC and in-band FEC contract: <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Rubato 4.0 `Resampler` API, fixed/adaptive ratios, delay and caller-buffer processing: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html>
