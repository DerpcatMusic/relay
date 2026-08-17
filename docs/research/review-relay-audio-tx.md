# Independent audit: relay-audio accumulator / TX

## Scope and evidence discipline

This is a read-only implementation audit of `crates/relay-audio/src/accumulator.rs`, `crates/relay-audio/src/tx.rs`, their configuration/packet dependencies, the canonical `relay-opus` encoder boundary, and the focused accumulator/TX tests. No implementation files were changed.

Only the three sources already cited by `docs/research/relay-audio-tx-implementation.md` were used:

1. RFC 3550, RTP sequence/timestamp semantics: <https://www.rfc-editor.org/rfc/rfc3550>
2. Xiph.Org libopus 1.6 decoder API, retained for the composition evidence set: <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Rubato 4.0 `Resampler` API, including streaming state, delay, caller buffers, and partial finite processing: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html>

Repository source and repository-local evidence documents were inspected as implementation evidence; no additional external source was introduced.

## Executive disposition

**Changes requested before the TX path is considered transactionally complete.** Normal-path accumulation, FIFO order, bounded backpressure, timeline wrapping, V1 policy construction, fixed allocation layout, and the exercised rate/duration paths are sound. However, two high-severity error-path issues can make an `Error` outcome hide already-emitted packets or allow continued use after an SRC backend error that may have advanced converter state. Several medium-severity commit/reset/finalization semantics also need tightening. The current tests pass but do not exercise these boundaries.

## Severity-ranked findings

### H1 — An input-validation `Error` can contain newly emitted packets and an advanced timeline without reporting either

`TxWorker::process_capture` drains prior pending output before validating the new chunk (`tx.rs:422-431` versus length validation at `tx.rs:446-450` and finite validation inside the converter call at `tx.rs:452-458`). If prior output can be fully drained without filling the supplied batch, packets are appended and the timeline advances; a malformed or non-finite new chunk then returns `TxProcessOutcome::Error(TxError)` with no `TxProcessReport`.

This is not an accumulator-order failure—the packets belong to previously accepted audio—but it is an outcome/accounting failure. A caller that treats `Error` as producing no output can discard valid packets already placed in `PacketBatch`. The current non-finite test starts with no pending output and resets immediately, so it cannot detect this case.

**Potential correction:** validate the submitted chunk's length and finiteness before any drain/mutation, or make the error outcome carry the process report and explicitly require the caller to consume any populated batch. Preserve the existing `input_pending` ownership rule for calls blocked before acceptance.

### H2 — Post-entry resampler failures do not fault the live worker

At `tx.rs:453-458`, every `ResampleError` is returned directly while `LiveState` remains `Active`. `NonFiniteInput` is safely checked before Rubato advances, but `Backend` and `NonFiniteOutput` may occur after backend processing has begun; the local resampler implementation checks output finiteness after `process_into_buffer`. Continuing as if the stream remained coherent can therefore break capture-to-media continuity and make the next chunk's state ambiguous.

**Potential correction:** prevalidate input errors before processing and keep those recoverable; route backend and non-finite-output failures through `self.fail(...)`, requiring explicit reset. If the resampler can guarantee a narrower transactional error taxonomy, encode that guarantee in distinct types rather than treating every error alike.

### M1 — Packet/audio commit and reset are only partially transactional on failures

`Packetizer::emit_complete` removes a full frame from the accumulator before `Encoder::encode` and `MediaPacket` creation succeed (`tx.rs:304-310`). The timeline itself is correctly advanced only after successful packet creation and insertion (`tx.rs:314-332`), but accepted PCM is no longer retained if encode/packet creation fails. The live worker faults, making the loss explicit at epoch level, yet the accumulator contract is not rollback-safe.

The finite path has a sharper version: it can successfully append and advance several packets, then return a bare `Err` on a later encode (`tx.rs:660-683`). `FiniteTxWorker` has no faulted/consumed state, so retry semantics are undefined and can duplicate the source at later sequence/timestamp values.

Reset also mutates the converter and clears pending-output cursors before the fallible encoder reset/policy reapplication (`tx.rs:478-488`). If codec reset fails, reset is partial. `Packetizer::reset` itself changes codec state before clearing the accumulator/timeline (`tx.rs:335-343`).

**Potential correction:** copy/peek a frame, construct and insert its packet, and only then commit accumulator removal. Give the finite worker an explicit one-shot/faulted lifecycle or return a partial-progress result with the error. Stage reset so failure leaves a documented faulted state; do not describe it as atomic unless all fallible work precedes visible state clearing.

### M2 — Finite backpressure reports source frames as consumed even though no conversion occurred

`FiniteTxWorker::process_finite` promises that an undersized batch causes no conversion (`tx.rs:600-601`). Its capacity preflight returns `empty_finite_report(input_frames)` (`tx.rs:642-650`), and that helper sets `FiniteProcessReport.input_frames` to the full source length (`tx.rs:716-723`). In the resampler report, `input_frames` means valid source frames consumed. The returned accounting therefore contradicts the no-conversion behavior and can cause a caller to release or advance a finite source that is still pending.

**Potential correction:** report zero consumed frames on the `batch_full` preflight, or separate `required/source_frames` from a converter report that only exists after conversion. Prefer a distinct `NeedsBatchCapacity { required_packets }` outcome.

### M3 — The disconnect state machine accepts and silently ignores a chunk after disconnect has begun

A disconnect sets `LiveState::Disconnecting` before draining (`tx.rs:417-420`). If the batch fills, the caller receives `BatchFull` and must call again. On the follow-up, a `CaptureInput::Chunk` is not rejected: pending output is drained and the `Disconnecting` branch returns `Disconnected` before the chunk match (`tx.rs:425-445`). The supplied chunk is neither consumed nor reported as pending. This is deterministic but unsafe API behavior for an incorrect event sequence.

**Potential correction:** once `Disconnecting`, accept only another `Disconnected` event and reject `Chunk` explicitly, or make draining a separate no-input operation so an event cannot be silently ignored.

### M4 — `abandoned_converter_delay_frames` is configuration latency, not always actual abandoned audio

The report always copies the converter's fixed `output_delay` (`tx.rs:377, 440`), including an immediate disconnect before any capture chunk was processed. In that case there is no retained source audio to abandon. The only focused disconnect test uses the 48 kHz bypass, where delay is zero, so non-unity semantics are untested.

**Potential correction:** either rename the field to state that it reports configured algorithmic delay, or track whether/how much streaming input established a retained tail and report actual abandoned media frames. Add immediate and post-input disconnect cases for each non-unity rate.

### L1 — The test evidence is broad on happy paths but incomplete on the claimed accounting and boundedness contracts

The integration matrix covers all four supported capture rates and 5/10/20 ms, with exact packet counts/hashes, and a dedicated 44.1 kHz + 5 ms test proves every emitted packet decodes to 240 samples/channel. It does **not** assert `capture_frames_consumed`, `media_frames_produced`, or cumulative SRC input/output counts; the dedicated 44.1 kHz test uses a loose 70–80 packet range. Thus “all SRC counts” are inferred from packet hashes/counts rather than directly proven.

Additional gaps:

- no validation error after prior pending output, so H1 is missed;
- no recoverable-input-error continuation without reset, and no backend/output-error fault test;
- no assertion of `input_pending` ownership across repeated one-slot backpressure;
- no disconnect-under-backpressure event-sequence test and no non-unity disconnect-delay test;
- no finite `RejectIncomplete`, exact-frame, empty-input, insufficient-batch, repeated-call, or mid-encode-error case;
- the non-finite test name says “does not advance,” but it resets the timeline before examining the next packet;
- pointer/capacity identity proves the five owned boxes and packet slots do not move, but there is no allocation-counter gate proving no transient/internal steady-state allocations during process/reset.

**Potential correction:** add focused transactional/accounting tests before expanding broad end-to-end coverage. Use an injectable packetizer/encoder failure seam if otherwise-unreachable codec/packet failures are part of the public error contract. Add allocation instrumentation around warmed processing and reset if allocation freedom is an acceptance gate.

## Confirmed-correct implementation properties

- **Accumulator order/overwrite:** public pushes validate frame alignment, finiteness, and capacity before mutation (`accumulator.rs:83-99`). Split-copy wrap logic and exact pop preserve FIFO order (`accumulator.rs:150-179`), and focused tests cover wraparound plus transactional invalid/full pushes.
- **Capacity and normal backpressure:** the configured minimum accumulator is maximum packet residual plus fixed SRC maximum output (`config.rs:135-168`). Buffered converter output has its own fixed box; the drain loop emits complete frames before appending more, preventing normal-path overwrite. A blocked new chunk remains caller-owned and is identified by `input_pending`.
- **Batch reuse:** `PacketBatch` is a fixed boxed slice of inline `MediaPacket` slots; process requires it empty, packet insertion is bounded, and take/clear reuse slots. `MediaPacket` payload storage is inline, so packet creation does not allocate.
- **Timeline:** sequence wraps by one and the 48 kHz timestamp wraps by the negotiated 240/480/960 samples/channel. `encode_pcm` obtains current values, successfully creates/inserts the packet, then advances. This satisfies the required wire commit point on successful packet creation.
- **Canonical Opus V1 wiring:** `TxStreamConfig` requires typed `EncoderPolicyV1`; `Packetizer::new` creates `EncoderConfigV1::stereo_48k` with the negotiated duration. The `relay-opus` boundary fixes Audio, complexity 10, VBR enabled, fullband, music signal, and DTX disabled, applies bitrate/FEC/loss controls, and reapplies the full policy after reset.
- **44.1 kHz + 5 ms and supported matrix:** capture chunks are not required to equal packet duration. The exact-rate matrix covers 44.1/48/96/192 kHz × 5/10/20 ms. 44.1 kHz uses a 441-frame capture chunk and downstream 240-frame Opus packets, exercising accumulation of two packets per nominal 10 ms converter transaction.
- **Finite trim/pad normal path:** the finite converter's valid output range is used, only complete Opus frames are emitted, and `ZeroPad` clears the entire PCM frame before copying the final useful prefix. Leading/trailing converter trimming and final valid/padded counts are exposed.
- **Fixed storage/reset:** capture input, converter output, packet PCM, encoded bytes, accumulator, and batch slots are constructed at fixed size. Reset reuses those buffers; the unit identity test confirms their addresses/capacities remain stable. Source inspection finds no `Vec` growth in steady-state TX methods.

## Recommended correction order

1. Fix H1 so every outcome has unambiguous batch/timeline progress and input ownership.
2. Fix H2 by faulting on converter failures that may mutate backend state.
3. Define packetizer/finite/reset commit semantics and implement M1.
4. Correct finite `batch_full` consumption accounting (M2).
5. Harden disconnect event sequencing and distinguish configured delay from actual abandoned data (M3/M4).
6. Add the focused tests listed in L1, including direct SRC report counts and an allocation-counting gate.

## Exact verification results

Executed from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` against the checked-in lockfile:

```text
cargo test -p relay-audio --lib --locked accumulator::
PASS — 2 passed; 0 failed; 3 filtered out

cargo test -p relay-audio --test tx --locked
PASS — 8 passed; 0 failed

cargo test --release -p relay-audio --lib --locked accumulator::
PASS — 2 passed; 0 failed; 3 filtered out

cargo test --release -p relay-audio --test tx --locked
PASS — 8 passed; 0 failed

cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings
PASS — no warnings
```

These successful gates validate the existing focused suite in debug/release and strict Clippy. They do not change the disposition because the findings are uncovered source-level state/error boundaries not exercised by that suite.
