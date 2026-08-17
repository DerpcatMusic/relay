# Relay Audio Deterministic TX Implementation

## Scope

Implement only the deterministic `relay-audio` transmit worker described by the finalized composition design: fixed-capacity capture-to-48 kHz conversion, exact-duration stereo Opus framing, RTP-style sequence/timestamp progression, reusable bounded packet output, explicit backpressure/disconnect/error reporting, and deterministic finite-input finalization. No networking, receive, rendering, hidden threads, blocking waits, unsafe code, or unbounded queues are in scope.

## Primary sources

These are the same three primary sources used by `relay-audio-composition-design.md`:

1. RFC 3550, RTP sequence/timestamp and interarrival-jitter semantics: <https://www.rfc-editor.org/rfc/rfc3550>
2. Xiph.Org libopus 1.6 decoder API, PLC and in-band FEC contract: <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Rubato 4.0 `Resampler` API, fixed/adaptive ratios, delay and caller-buffer processing: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html>

For TX specifically, RFC 3550 establishes monotonically incrementing modulo sequence numbers and a media-rate timestamp clock; the existing `relay-opus` boundary pins the legal RELAY frame sizes and canonical stereo 48 kHz format; and the pinned Rubato API supports caller-owned buffers while distinguishing streaming state from finite `partial_len`/drain processing. The libopus decoder source remains in the evidence set for composition continuity even though task 2 does not implement RX.

## Required design decisions

- Accumulate interleaved stereo samples at 48 kHz in an O(1), fixed-capacity ring. Preserve incomplete negotiated frames across calls and never emit a short Opus frame.
- Construct and preallocate the capture-rate converter once. Bypass conversion at 48 kHz without changing downstream framing semantics.
- Drive a canonical stereo Opus encoder with exactly 240, 480, or 960 samples/channel for 5, 10, or 20 ms.
- Advance `TxTimeline` exactly once per emitted packet: wrapping `u16` sequence and wrapping `u32` timestamp by the negotiated 48 kHz frame length.
- Return packets through a reusable fixed-capacity `PacketBatch`; when capacity is exhausted, preserve unconsumed audio and report explicit backpressure rather than allocating or dropping silently.
- Separate live processing from finite-input finalization where converter semantics differ. Finalization may pad only as an explicit policy and must report padded/trimmed samples; it must never encode a short negotiated packet.
- Validate non-finite samples and configuration/input invariants. Reset/drain behavior must be deterministic and explicit.

## Corrections and implementation notes

- **Streaming and finite sources are separate types.** `TxWorker` consumes only exact, converter-required live chunks. A live disconnect emits every already-complete Opus frame, explicitly discards the final partial 48 kHz frame, reports the streaming converter delay whose retained tail is abandoned, and never fabricates finite-source completion. `FiniteTxWorker` instead uses `FiniteFixedRatioConverter`, exposes its leading/trailing trim report, and supports an explicit `ZeroPad` or `RejectIncomplete` final-frame policy.
- **Converter output is staged, not sized into the accumulator.** A preallocated max-size converter output buffer retains any suffix blocked by `PacketBatch`. This lets the accumulator remain at its validated fixed capacity while preventing overwrite, allocation, or silent drop when one converter call produces several Opus frames.
- **Backpressure preserves ownership/state.** `PacketBatch` must be empty on entry. A full batch returns `BatchFull`; pending converter output and accumulator state remain in the worker. The caller consumes the batch and calls again. No retry, channel, thread, wait, or I/O is hidden.
- **Timeline advancement is transactional.** `TxTimeline` exposes the current wire values and advances with wrapping arithmetic only after Opus encode and inline `MediaPacket` construction succeed.
- **Canonical encoder policy is typed and versioned.** `TxStreamConfig` requires `EncoderPolicyV1`, which carries the negotiated bitrate/FEC/loss hint while `relay-opus` explicitly fixes Application::Audio, maximum complexity, VBR, fullband music signal, and DTX disabled. `EncoderConfigV1` combines that policy with the negotiated 5/10/20 ms duration; no libopus behavior is left to an implementation default.
- **Finalized foundation capacities are authoritative.** `AudioPipelineConfig` owns `capture_src_chunk_frames`, derives fixed-converter requirements, validates capture/accumulator/playback transaction minima, and centralizes per-pipeline packet bounds. TX constructs its live/finite converters with that exact chunk size, bounds the encoder output buffer to `packet_capacity`, and creates packets through `AudioPipelineConfig::create_media_packet`.

## Implementation

- `accumulator.rs`: fixed boxed interleaved stereo ring, O(1) read/write index updates, split-copy wrap handling, all-or-nothing validated public operations, and allocation identity tests.
- `tx.rs`: fixed `PacketBatch`, exact wrapping `TxTimeline`, canonical `Packetizer`, caller-driven `TxWorker`, live disconnect report, and preallocated one-shot `FiniteTxWorker` with exact resampler trimming/zero-padding accounting.
- `tests/tx.rs`: rate/duration matrix, 44.1 kHz + 5 ms accumulation, wrapping and one-slot backpressure, Opus round-trip channel isolation, non-finite/reset behavior, deterministic packet hashes/counts, and finite trimming/padding.

## Validation plan

- Rates: 44.1, 48, 96, and 192 kHz capture to 48 kHz.
- Durations: 5, 10, and 20 ms, including 44.1 kHz + 5 ms capture-chunk accumulation.
- Timeline: sequence and timestamp wrapping.
- Audio correctness: stereo channel isolation, non-finite rejection, reset and finite drain/finalize behavior.
- Boundedness: accumulator and packet-batch capacity/backpressure; buffer pointer/capacity stability after construction.
- Reproducibility: deterministic packet counts and packet hashes.
- Commands: locked package/workspace formatting, checks, tests, release build/tests as applicable, and strict Clippy. Exact commands and results will be recorded below.

## Exact validation results

All requested local locked gates pass on the final tree:

- `cargo fmt --all -- --check` — **pass**.
- `cargo check -p relay-audio --all-targets --all-features --locked` — **pass**.
- `cargo test -p relay-audio --all-targets --all-features --locked` — **pass**: 5 unit + 18 foundation + 8 TX integration tests.
- `cargo test --release -p relay-audio --locked` — **pass**: the same 31 tests plus doc tests.
- `cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings` — **pass**.
- `cargo check --workspace --all-targets --all-features --locked` — **pass**.
- `cargo test --workspace --all-targets --all-features --locked` — **pass**.
- `cargo test --release --workspace --all-targets --all-features --locked` — **pass** (the separately marked relay-opus release throughput test remains intentionally ignored by its crate unless explicitly selected).
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — **pass**.

The deterministic TX matrix pins exact packet counts and FNV-1a packet hashes for all 12 capture-rate/frame-duration combinations. The pointer/capacity tests retain identities across repeated process/backpressure/reset cycles for capture input, SRC output, Opus PCM, encoded output, accumulator storage, and `PacketBatch` slots.
