# relay-audio foundation implementation evidence

## Final disposition

**Complete and validated.** `relay-audio` is now a workspace member and contains only the bounded composition foundation requested for this task. TX, RX, decode/FEC/PLC, resampling execution, rendering, lifecycle workers, sockets, threads, async tasks, and callback work remain intentionally unimplemented.

## Scope and implementation

The implementation adds:

1. `AudioPipelineConfigInput` and immutable `AudioPipelineConfig`, validating the closed Phase-1 device-rate set (44.1/48/96/192 kHz), stereo-only media, fixed 5/10/20 ms Opus duration, nonzero capacities/cadence, interleaved-frame alignment, a TX accumulator large enough for one 48 kHz stereo Opus frame, reorder capacity below 32,768, due-batch capacity not exceeding network capacity, packet capacity not exceeding `MAX_PACKET_BYTES`, and byte-size overflow.
2. Typed `Ssrc`, `SequenceNumber`, `RtpTimestamp`, `ExtendedSequence`, and `ExtendedTimestamp` values. Sequence/timestamp extension selects the unique nearest epoch-relative position and returns typed errors for exact-half-range ambiguity, before-epoch underflow, and `u64` overflow.
3. A validated seven-bit `PayloadType` and owned `MediaPacket` with exactly `MAX_PACKET_BYTES` of inline storage, a validated nonzero length, and access only to the initialized payload prefix.
4. `DeterministicNetwork`, whose scheduled slots are fixed at construction. `Deliver`, `Drop`, `Duplicate`, and `Delay` are explicit caller actions. Extraction order is the stable key `(delivery_time, insertion_ordinal)`, not backing-slot order.
5. Reusable fixed `DueBatch` storage. Each advance/drain is bounded by both the batch's capacity and the network's configured per-call maximum. Unconsumed batches cannot be overwritten.
6. Allocation-free full-queue rejection through a `ScheduleOutcome` result object containing small `ScheduleStatus` plus an optional returned inline packet. This avoids both `Box` allocation and a size-imbalanced result enum. A duplicate with only one free slot retains the original and records rejection of the second copy.
7. Fake-network truth metrics kept separate from receiver/playout terminology, plus explicit drain and reset. Reset retains allocations while clearing queued packets, virtual time, insertion ordinal, and counters.
8. Fourteen foundation integration tests covering every supported rate/duration, invalid config classes and overflow, wire wrap, both extension wrap directions, exact half ranges, timeline underflow/overflow, payload bounds, network capacity/owned rejection, ordering, bounded batches, duplicate/drop/delay behavior, time regression/overflow, drain, reset, and invalid construction.

Steady-state packet, timeline, schedule, advance, and drain operations do not grow storage. Construction uses fallible `Vec::try_reserve_exact` only to create fixed boxed slices. The crate forbids unsafe code and contains no thread, async, socket, codec-call, callback, or unbounded-history implementation.

## Sources

1. RFC 3550, RTP sequence/timestamp and interarrival-jitter semantics: <https://www.rfc-editor.org/rfc/rfc3550>
2. Xiph.Org libopus 1.6 decoder API, PLC and in-band FEC contract: <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Rubato 4.0 `Resampler` API, fixed/adaptive ratios, delay and caller-buffer processing: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html>

No additional external source was used.

## Design corrections resolved during implementation

- The supported sample rates are the closed `relay-resample::SUPPORTED_SAMPLE_RATES` set, not an arbitrary numeric range.
- A negotiated duration must be integral at the 48 kHz Opus boundary, but it must **not** be required to equal an integral number of device frames at every supported device rate: 5 ms at 44.1 kHz is 220.5 frames. Fixed SRC chunking is independent of packet duration. Configuration therefore validates device rings as complete interleaved sample frames and derives the exact packet size only at 48 kHz.
- RTP payload type validation accepts the complete seven-bit field (`0..=127`); fixing a negotiated value per epoch belongs to the later RX/TX epoch state machines.
- Exact half-range serial distances are rejected as ambiguous. Remote values never select or reset an epoch.
- A primary full-queue rejection returns the original `MediaPacket`. Requested duplication is deterministic but not atomic with respect to capacity: with one free slot the original is scheduled and the unavailable duplicate is counted separately.
- Simulated drops are network-model truth only. No playout-gap or receiver-loss counter exists in this foundation.
- Reset clears truth counters as well as queue/timeline state so reuse reproduces construction-state behavior. Drain preserves stable delivery order without changing virtual time.

## Decisions

- Reuse `relay_opus::FrameDuration`, `CHANNELS`, and `MAX_PACKET_BYTES`, and `relay_resample::SUPPORTED_SAMPLE_RATES`, rather than introducing composition-layer copies that could drift.
- Represent raw configuration in a documented input value and expose only validated immutable configuration to future workers.
- Keep wire and extended values distinct; extension is a pure, deterministic calculation relative to a trusted local reference.
- Preallocate scheduled slots and due output slots during construction. Scan bounded storage to choose the globally earliest stable delivery key; backing-array holes cannot perturb order.
- Saturate observational counters while rejecting timeline/virtual-time arithmetic overflow explicitly.
- Keep the returned-packet ownership seam allocation-free. Packet drops and batch clearing are documented as worker/control-path operations, not callback operations.

## Files

- `Cargo.toml`: registers `crates/relay-audio` as a workspace member.
- `Cargo.lock`: records the local `relay-audio` package and its focused dependencies.
- `crates/relay-audio/Cargo.toml`
- `crates/relay-audio/src/lib.rs`
- `crates/relay-audio/src/config.rs`
- `crates/relay-audio/src/timeline.rs`
- `crates/relay-audio/src/packet.rs`
- `crates/relay-audio/src/network.rs`
- `crates/relay-audio/tests/foundation.rs`
- `docs/research/relay-audio-foundation-implementation.md`

## Validation

All commands ran from the repository root with Rust 1.92 and completed successfully:

```text
cargo fmt --all -- --check
PASS

cargo check -p relay-audio --all-targets --all-features --locked
PASS

cargo test -p relay-audio --all-targets --all-features --locked
PASS: 14 relay-audio integration tests; 0 failures

cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings
PASS: no warnings

cargo check --workspace --all-targets --all-features --locked
PASS

cargo test --workspace --all-targets --all-features --locked
PASS: all workspace targets; 14 relay-audio foundation tests and all pre-existing suites; 0 failures

cargo test --release -p relay-audio --locked
PASS: 14 integration tests plus library/doc-test targets; 0 failures

cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
PASS: no warnings
```

An initial locked check correctly reported that `Cargo.lock` needed the newly added local package. `cargo check -p relay-audio --all-targets --all-features --offline` updated only lock resolution for the local workspace member; every recorded final gate above then ran with `--locked`.
