# `relay-rt` implementation research

**Status:** implemented and locally validated  
**Date:** 2026-08-16  
**Scope:** `crates/relay-rt` only; bounded preallocated SPSC transport for interleaved `f32` samples. No root-workspace edit, networking, codec, resampling, DSP, device integration, or lifecycle coordinator is included.

## Acceptance criteria

- Construction may allocate a fixed-capacity queue; callback-facing reads/writes may not allocate, lock, wait, retry, log, format, or perform I/O.
- Exactly one producer and one consumer endpoint; payload slots contain plain `f32`, never heap-owning per-item values.
- Writes are all-or-drop-new when the full input slice does not fit.
- Reads may be partial into caller-owned memory and report exactly how much was copied.
- Full/empty and endpoint destruction are distinct outcomes.
- Relaxed atomic diagnostics expose dropped scalar samples, underrun operations, and missing scalar samples.
- Capacity, physical wrap, disconnected endpoints, partial reads, and counters have deterministic tests.

## Primary sources consulted

Only the following three upstream/official sources were used for this focused task (accessed 2026-08-16):

1. [`rtrb` 0.3.4 crate documentation](https://docs.rs/rtrb/0.3.4/rtrb/) and its linked [`Producer`](https://docs.rs/rtrb/0.3.4/rtrb/struct.Producer.html) / [`Consumer`](https://docs.rs/rtrb/0.3.4/rtrb/struct.Consumer.html) API pages. This establishes construction-time fixed allocation, SPSC ownership, immediate lock-free/wait-free operations, non-overwriting full behavior, whole-slice push, partial-slice pop, and `is_abandoned()` endpoint observations.
2. [Rust standard library `std::sync::atomic::Ordering`](https://doc.rust-lang.org/1.92.0/std/sync/atomic/enum.Ordering.html). `Relaxed` is sufficient for independent observational counters because those counters do not publish or protect queue data; `rtrb` owns the queue's acquire/release protocol.
3. [Rust Reference: conditional compilation, `target_has_atomic`](https://doc.rust-lang.org/1.92.0/reference/conditional-compilation.html#target_has_atomic). The crate rejects targets without native 64-bit atomic support instead of risking a non-lock-free callback counter implementation.

The repository's required `audio-engineering-principles` and `audio-dsp` skills were also applied as engineering constraints; they are not counted as external primary sources.

## Findings and decisions applied

### Ring shape and data ownership

`audio_ring(capacity_samples)` constructs `rtrb::RingBuffer<f32>` and returns non-cloneable `AudioProducer` / `AudioConsumer` role handles plus a separate metrics handle. Capacity and operation counts are scalar interleaved samples, not frames. This keeps the primitive payload trivially destructible and avoids a callback-side `Vec`, `Box`, `Arc`, or other heap-owning item drop.

Construction is `O(capacity_samples)` memory and may allocate. A successful write or read is `O(slice length)` copying; an immediate rejected write is `O(1)` plus a counter increment. Queue memory remains `O(capacity_samples)`. No operation grows storage.

### Overflow and underflow

`AudioProducer::write()` uses the upstream whole-slice operation. It publishes the complete input or publishes none of it. An insufficient-capacity result becomes `WriteOutcome::DroppedFull` and increments `dropped_samples` by the full new slice length. There is no overwrite, spin, retry, sleep, or callback logging.

`AudioConsumer::read()` uses the upstream partial-slice operation. It fills only the leading returned range and deliberately leaves the caller's remainder unchanged. A short read increments both the underrun-event count and missing-sample count. Silence/concealment remains a higher-level renderer policy rather than hidden DSP in this crate.

### Disconnection and destruction

`WriteOutcome::Disconnected` and `ReadState::Disconnected` explicitly expose an already-observed destroyed counterpart. Buffered samples can still be drained after producer destruction. Because upstream `is_abandoned()` is documented as a crude, non-synchronizing observation, it is **not** treated as a stop acknowledgement or memory-reclamation barrier; a concurrent destruction race can only be observed on this call or a later call.

Endpoint destruction must happen off the audio callback. The final endpoint drop can destroy buffered items and deallocate the ring, while the final metrics-handle drop can deallocate the counters. The embedding device lifecycle must stop/detach the callback and receive its API-specific acknowledgement before endpoints and metrics are destroyed on a control/worker thread. Keeping an extra metrics handle does not keep the ring storage alive.

### Atomic diagnostics

Counters use native `AtomicU64::fetch_add`/`load` with relaxed ordering. They are telemetry, not queue synchronization. A snapshot is intentionally observational and not transactionally coherent across fields. Counters wrap at the integer limit rather than introducing a compare/exchange retry loop. The crate compile-gates targets without native 64-bit atomics.

## Realtime contract audit

| Operation | Allocation | Lock/wait/retry | Logging/I/O/DSP | Bound |
|---|---:|---:|---:|---:|
| `audio_ring` | yes, control thread only | no | no | `O(capacity)` initialization |
| `AudioProducer::write` | no | no | no | `O(input.len())` success; immediate reject |
| `AudioConsumer::read` | no | no | no | `O(output.len())` |
| counter update/snapshot | no | no | no | `O(1)` |
| endpoint/last metrics destruction | may deallocate | no | no | off-callback only |

The callback-facing implementation contains no allocation macro/container growth, mutex, wait/yield/sleep, loop-based retry, formatting, logging, file/network/device call, or DSP operation. `rtrb` owns its small acquire/release atomic queue protocol; this wrapper adds only bounded copies, endpoint observations, and relaxed counter atomics.

## Deterministic tests

`tests/ring.rs` covers:

- zero-capacity rejection and use of every configured slot;
- full-queue rejection and all-or-drop behavior when only part of a new slice would fit;
- FIFO preservation across physical wrap;
- connected partial read with untouched output remainder;
- consumer destruction observed by the producer;
- producer destruction with buffered drain followed by disconnected empty read;
- dropped-sample, underrun-event, and missing-sample snapshots;
- empty operations without false counter increments.

## Validation evidence

Run from the repository root using only the crate's isolated manifest and the already-cached dependency:

```text
cargo fmt --manifest-path crates/relay-rt/Cargo.toml
cargo test --manifest-path crates/relay-rt/Cargo.toml --offline
# 8 integration tests passed; unit/doc tests passed

cargo clippy --manifest-path crates/relay-rt/Cargo.toml --all-targets --offline -- -D warnings
# passed

cargo check --manifest-path crates/relay-rt/Cargo.toml --all-targets --offline
# passed
```

The crate-local `[workspace]` permits isolated validation while the Phase-1 crate is not yet admitted by the root workspace. `rtrb` is pinned to `=0.3.4`; `crates/relay-rt/Cargo.lock` records the isolated resolution. No network access was used.

## Potential corrections to the master plan

1. **Behavioral correction:** endpoint abandonment must remain diagnostic, not lifecycle synchronization. Safe reclamation requires device-stop acknowledgement followed by off-callback destruction.
2. **Payload clarification:** the realtime queue should continue to carry plain samples (or another trivially destructible fixed-size value), not heap-owning per-item audio blocks.
3. **File-layout clarification:** the master plan's separate `input_ring.rs`, `output_ring.rs`, `counters.rs`, and `snapshot.rs` paths are more granular than this initial deep module needs. One `ring.rs` keeps the shared SPSC policy together while `counters.rs` owns diagnostics; split only when independently meaningful behavior appears.

No correction is needed to the master plan's selection of a bounded `rtrb` SPSC seam.
