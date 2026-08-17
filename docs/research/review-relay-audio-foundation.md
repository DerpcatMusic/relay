# Independent Audit: `relay-audio` Foundation

**Audit date:** 2026-08-17  
**Disposition:** **Changes requested before the foundation is treated as a complete composition contract.** The timeline extension and deterministic-network core are sound for their documented model, and all three narrow validation gates pass. However, the validated configuration currently admits a controller cadence that `relay-clock` will reject, admits rings that cannot be shown to hold one worker transaction, sizes the TX accumulator only for one packet rather than worst-case append state, and does not freeze a canonical Opus encoder profile beyond the fixed wire format and frame duration.

## Scope and method

This was a local, read-only code audit except for replacing this report. No web sources, implementation changes, or subagents were used. The primary evidence inspected was:

- `crates/relay-audio/src/{config,timeline,packet,network,lib}.rs`
- `crates/relay-audio/tests/foundation.rs`
- `docs/research/relay-audio-foundation-implementation.md`
- `docs/research/relay-audio-composition-design.md`
- the relevant `relay-clock` recovery configuration and update contract
- the local `relay-opus` and `relay-resample` public contracts where the foundation claims to compose their bounds

Severity means:

- **High:** an accepted foundation configuration does not establish a required cross-crate invariant.
- **Medium:** the local primitive remains safe, but its public guarantee or validation proof is incomplete.
- **Low:** implementation is sound, but an important adversarial proof is absent.

## Findings, ranked by severity

### F1 — High — Controller cadence is **not** compatible with the clock-recovery maximum

**Decision:** The configuration lacks this compatibility guarantee.

`controller_cadence_frames` is documented as local playback frames (`config.rs:37-38`) but validation only checks that it is nonzero (`config.rs:116`). `relay-clock` defaults `max_update_interval_seconds` to `0.25` (`recovery.rs:31-43`), documents that longer intervals are rejected without state mutation (`recovery.rs:24-28`, `recovery.rs:148-151`), and enforces that rejection in `ClockRecovery::update` (`recovery.rs:202-216`).

Consequently, an input such as 44.1 kHz playback with cadence 11,026 frames is accepted by `AudioPipelineConfig` even though its interval is greater than 0.25 seconds and every corresponding default-controller update will return `UpdateIntervalTooLong`. Much larger values, including `usize::MAX`, are also accepted because cadence is not included in any overflow or upper-bound check.

**Potential correction:** Make the clock policy part of construction rather than duplicating an unrelated constant. Validate

`controller_cadence_frames / playback_rate_hz <= max_update_interval_seconds`

using checked arithmetic or an exact rational representation. Prefer constructing the audio configuration with the validated clock-recovery policy (or a shared validated cadence type) so a non-default maximum cannot diverge silently. For the current default, exact boundary tests should accept 11,025 frames and reject 11,026 at 44.1 kHz, with corresponding at/below/above cases for every supported playback rate.

**Disposition:** Must fix before clock integration.

---

### F2 — High — Ring minima are unspecified, and TX accumulator sizing is insufficient for the stated append contract

**Decision:** Minimum sizing is not established.

All three scalar-sample buffers are checked only for nonzero, channel-aligned capacity (`config.rs:69-83`, `config.rs:287-302`). Only the TX accumulator receives an additional minimum, exactly one 48 kHz interleaved Opus packet (`config.rs:67`, `config.rs:84-90`). Thus a two-sample stereo capture ring and a two-sample stereo playback ring are valid at every supported rate and duration.

That does not establish the composition contract. The design requires fixed converter chunks to be consumed, finite converter output to be appended, and RX output to use all-or-drop writes (`relay-audio-composition-design.md:50-55`, `relay-audio-composition-design.md:62-65`). The resampler contract requires an exact `input_frames_next` input and an output workspace of `output_frames_max` frames. None of those chunk/output bounds are present in `AudioPipelineConfigInput` (`config.rs:14-38`), so the config cannot prove either ring holds one complete transaction.

The accumulator check is also only enough when it is empty. Under the documented “append output, then emit every complete packet” order, it can already contain up to `opus_packet_samples - channels` aligned residual samples. A converter call may then produce as many as its maximum output samples. The safe append-before-drain minimum is therefore based on checked `maximum_residual + maximum_converter_output`, not merely `opus_packet_samples`. Alternatively, the worker algorithm must drain incrementally while appending and encode that different bound in its API.

**Potential correction:** Add or derive the worker transaction sizes at construction:

1. capture ring capacity at least one complete fixed-SRC input transaction;
2. TX output workspace at least the converter's maximum output;
3. TX accumulator large enough for its documented residual-plus-append policy;
4. playback ring capacity at least one complete adaptive-SRC publication, plus whatever validated target/safety margin the controller and soak invariant require.

Perform every frames-to-interleaved-samples and residual-plus-output calculation with checked arithmetic. Do not impose a false “packet duration must be integral in device frames” rule: 44.1 kHz / 5 ms remains valid across the SRC boundary.

Add boundary matrices that reject one aligned frame below each derived minimum and accept exactly the minimum for every supported rate/duration/direction.

**Disposition:** Must fix or explicitly defer these capacities until a later constructor that owns the converter requirements; the present “fully validated” claim is too strong (`config.rs:41-45`).

---

### F3 — High — The configuration does **not** freeze a canonical Opus profile beyond format/duration

**Decision:** It freezes stereo, the 48 kHz Opus boundary indirectly, supported frame duration, and maximum packet storage; it does not freeze the encoder profile requested by the audit brief.

The only codec choice in `AudioPipelineConfigInput` is `frame_duration` (`config.rs:19-22`). Validation imports `CHANNELS`, `FrameDuration`, and `MAX_PACKET_BYTES` (`config.rs:3`) and derives packet samples from duration (`config.rs:67`), but there are no fields or canonical constants for application, bitrate, complexity, VBR mode, bandwidth, signal, DTX, FEC policy, or packet-loss hint.

This is not merely hidden inside `relay-opus`. Its `EncoderConfig` lets each caller choose `Application` and stores mutable FEC/loss controls (`relay-opus/src/lib.rs:89-94`, `relay-opus/src/lib.rs:118-142`); encoder construction applies those values (`relay-opus/src/lib.rs:328-341`), and FEC/loss can be changed later (`relay-opus/src/lib.rs:386-408`). Other listed codec controls are left to libopus defaults because the facade exposes no canonical values for them. Lack of a setter is not a versioned, inspectable profile guarantee.

The foundation implementation evidence accurately says “fixed Opus duration” (`relay-audio-foundation-implementation.md:11`), but that is narrower than a canonical fixed-profile guarantee. The composition design separately requires stereo 48 kHz Opus, negotiated duration, and DTX disabled (`relay-audio-composition-design.md:10-12`); `AudioPipelineConfig` does not represent or verify DTX.

**Potential correction:** Define one typed/versioned canonical Opus profile shared by pipeline and encoder construction. It should explicitly state every intentionally fixed value and every negotiated/dynamic value, including application, bitrate/bounds, complexity, VBR/CBR, bandwidth, signal, DTX, in-band FEC, and loss hint. Construct `relay_opus::EncoderConfig` only from that validated profile, make encoder/decoder duration derive from the same object, and test getters/controls after construction and reset. If adaptive FEC/loss is intended, model its allowed range and ownership explicitly rather than calling the whole profile frozen.

**Disposition:** Must fix the contract or narrow the stated acceptance criterion. Current code cannot prove two pipeline instances select the same canonical encoder behavior.

---

### F4 — Medium — Configuration preflight checks a different network allocation, and config bounds are not coupled to constructed values

The comment says configuration preflights byte arithmetic used by future fixed-storage constructors (`config.rs:118-120`), but `network_capacity` is checked as `Option<MediaPacket>` (`config.rs:124`). The actual network allocates `Option<ScheduledPacket>`, which also includes delivery time and insertion ordinal (`network.rs:181-186`, `network.rs:379-388`). Therefore there are extreme capacities for which `AudioPipelineConfig::new` passes its network byte check but `DeterministicNetwork::new` returns `CapacityOverflow`. The direct network constructor remains safe; the composition config's preflight guarantee is false.

The same separation weakens other cross-field guarantees. `DeterministicNetwork::new` and `DueBatch::new` accept independent capacities (`network.rs:199-221`, `network.rs:362-395`), so callers can construct instances that do not match an existing `AudioPipelineConfig`. Likewise, `packet_capacity` is validated against the global maximum (`config.rs:109-115`), but `MediaPacket::new` validates only against that global maximum (`packet.rs:193-203`), not the smaller per-pipeline value.

**Potential correction:** Centralize exact capacity arithmetic in the owning constructors and expose config-driven factories (or validated capacity newtypes) for the network, due batch, and packet boundary. If preflight remains in `AudioPipelineConfig`, check the exact scheduled slot type through a public helper owned by `network.rs`; do not approximate a private layout. Ensure a configured packet capacity below `MAX_PACKET_BYTES` is actually enforced at packet ingress/encode.

**Disposition:** Fix before claiming that one immutable config establishes all finite queue and packet bounds. This is a validation-coherence issue, not a memory-safety failure.

---

### F5 — Low — Correct serial and network edge behavior lacks complete adversarial proof

The existing tests cover representative wrap directions, exact half ranges, epoch underflow/extended overflow (`foundation.rs:170-223`), stable network order (`foundation.rs:251-281`), partial duplication/full rejection (`foundation.rs:283-307`), batching, reset, time overflow, and invalid primary network capacities (`foundation.rs:309-431`). Important gaps remain:

- no exhaustive/property-style 16-bit sequence extension oracle across reference low bits and forward/backward/half-range distances;
- no timestamp tests immediately on both sides of `2^31`, or near-but-not-overflowing `u64` endpoints;
- no test that associates SSRC changes with a trusted epoch reset (the extension primitive correctly assumes the caller supplies the right epoch);
- no reachable test of insertion-ordinal exhaustion and the all-or-nothing duplicate rejection at that boundary;
- no direct `DueBatch::new(0)` / byte-overflow tests;
- no full metrics reconciliation across submit, schedule, deliver, duplicate, drop, and rejection paths;
- no allocation observer around warmed-up schedule/advance/drain, so the allocation-free claim is supported by source inspection rather than a regression gate;
- no tests for the cadence and sizing failures in F1/F2, nor any canonical-profile test for F3.

**Potential correction:** Add a test-only ordinal seed or factor ordinal advancement into a small pure helper so `u64::MAX` boundaries are testable without billions of submissions. Add an exhaustive sequence oracle (65,536 wire values around selected references), focused timestamp boundary vectors, constructor error tests, metrics reconciliation, and a repository-approved allocation observer that does not weaken the unsafe-code policy.

**Disposition:** Does not invalidate the correct primitives below, but these tests are required before the evidence can call the foundation adversarially complete.

## Verified decisions

### Serial extension is correct under its documented epoch precondition

`extend_sequence` computes modular forward distance, rejects exactly 32,768, uses checked forward addition below half range, and checked backward subtraction above it (`timeline.rs:202-227`). `extend_timestamp` applies the identical rule at `2^31` (`timeline.rs:229-253`). Both reject underflow/overflow rather than aliasing another epoch, and neither permits a remote value to reset the reference. This is the correct nearest-candidate serial extension rule. SSRC/epoch association remains a caller responsibility, as documented by the trusted-local constructors (`timeline.rs:92-110`, `timeline.rs:137-151`).

### Deterministic network steady-state allocation and ordering are sound

Construction performs the only Rust heap allocations for scheduled slots and due-batch slots using fallible reserve then fixed boxed slices (`network.rs:205-221`, `network.rs:369-395`). `MediaPacket` owns a fixed inline payload (`packet.rs:53-66`); duplicate scheduling clones that fixed value only (`network.rs:465-489`). Schedule, advance, drain, and reset do not resize storage.

Work is finite in configured bounds: insertion scans at most `network_capacity` slots (`network.rs:583-588`); extraction emits at most `DueBatch::capacity()` packets and each earliest selection scans scheduled storage (`network.rs:590-610`), for worst-case `O(network_capacity * due_batch_capacity)` per extraction plus an `O(network_capacity)` due count. That is bounded worker-side work, not constant work independent of configuration.

Ordering is the documented `(deliver_at, insertion_ordinal)` key (`network.rs:603-610`). Ordinal space is checked before either copy is inserted (`network.rs:460-474`), the primary and duplicate receive consecutive unique ordinals (`network.rs:476-489`), and overflow returns ownership without partially inserting. The check conservatively refuses an operation that would leave no representable *next* ordinal, so it never wraps or creates a tie. `reset` deliberately starts a new deterministic model epoch at ordinal zero after clearing every slot (`network.rs:554-567`).

## Evidence reconciliation

The implementation evidence's statements about fixed inline packets, stable network ordering, reusable due batches, owned full-queue rejection, and no steady-state storage growth (`relay-audio-foundation-implementation.md:13-20`) agree with the source.

The evidence should not retain the unqualified status “Complete and validated” (`relay-audio-foundation-implementation.md:5`) until F1-F4 are resolved. Its narrower description of validation—especially “fixed Opus duration” and an accumulator large enough for one packet (`relay-audio-foundation-implementation.md:11`)—matches what the code actually does, but does not satisfy the stronger cross-crate cadence, transaction sizing, or canonical-profile requirements.

## Exact validation performed

All requested narrow package gates passed from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`:

```text
cargo test -p relay-audio --locked
PASS: 14 integration tests, 0 failures; library and doc-test targets also passed.

cargo test --release -p relay-audio --locked
PASS: 14 integration tests, 0 failures; optimized library and doc-test targets also passed.

cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings
PASS: finished successfully with no warnings.
```

No workspace-wide command, formatter, web lookup, or implementation edit was run as part of this audit.
