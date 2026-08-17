# Relay Audio Foundation Fixes (F1, F2, F4)

## Scope and implementation mapping

| Audit item | Resolution |
| --- | --- |
| **F1 — cadence/config compatibility** | `AudioPipelineConfigInput` now carries `ClockRecoveryConfig`, `AdaptiveClockConfig`, and an explicit `capture_src_chunk_frames`. Construction validates the controller by constructing `ClockRecovery`, constructs the adaptive converter to validate its policy, requires the adaptive correction clamp to contain the controller's full output clamp, and compares cadence to `max_update_interval_seconds` using the exact dyadic rational represented by the configured `f64` (no rounded seconds conversion). |
| **F2 — transaction minima** | Configuration constructs temporary fixed and adaptive resamplers off-thread and retains their authoritative `FrameRequirements`. Checked frame-to-sample arithmetic derives the capture-ring minimum from fixed `input_frames_next`, the TX-accumulator minimum from `(Opus packet samples - one interleaved frame) + fixed output_frames_max * channels`, and the playback-ring minimum from adaptive `output_frames_max * channels`. Aligned capacities one frame below these minima are rejected. Factories recreate the validated fixed/adaptive resamplers and clock controller off-thread. |
| **F4 — exact allocation and packet boundaries** | The false `Option<MediaPacket>` network-layout preflight was removed. Config-owned factories delegate deterministic-network and `DueBatch` allocation/layout checks to their actual constructors. Config-owned packet creation and validation enforce the configured `packet_capacity`; production TX uses `AudioPipelineConfig::create_media_packet`, retaining fixed inline packet storage. |

## Public composition boundary

`AudioPipelineConfig` now exposes:

- validated clock/adaptive policies and capture chunk size;
- authoritative fixed/adaptive resampler requirements;
- the three derived minimum scalar-sample capacities;
- off-thread factories for fixed SRC, adaptive SRC, clock recovery, deterministic network, and due batch;
- typed/raw packet creation plus existing-packet validation constrained by the per-pipeline packet capacity.

`MediaPacket::new` remains source-compatible and retains its global fixed-inline maximum. The config factory uses a crate-private maximum-aware constructor, so no payload heap allocation or steady-state storage growth was introduced.

## Arithmetic and realtime properties

- Every derived frames-to-interleaved-samples operation uses checked multiplication.
- The append-before-drain accumulator calculation uses checked subtraction/addition and reserves the maximum packet-aligned residual plus the fixed SRC's maximum output.
- Cadence comparison is integer/dyadic and exact for the actual finite positive `f64` policy value.
- Resampler/controller/network/batch construction remains off the hard-realtime callback.
- Packet creation still copies into fixed inline storage; config packet validation adds no allocation.
- Existing processing paths remain bounded and preallocated after construction.
- 44.1 kHz + 5 ms remains valid: packet duration is exact only at the 48 kHz media boundary and is not required to be an integral device-frame duration.

## Tests added/expanded

`crates/relay-audio/tests/foundation.rs` now covers:

- all 4 supported rates × all 3 packet durations;
- exact cadence below/at/above the recovery maximum for every rate/duration (44.1 kHz accepts 11,025 frames and rejects 11,026 with the default 0.25 s policy);
- capture-ring, TX-accumulator, and playback-ring one aligned frame below / exactly at / one frame above each derived minimum for every rate/duration;
- configured packet capacity below/at/above the inline maximum and payload below/at/above a smaller configured bound for every rate/duration;
- adaptive/recovery correction-range equality and rejection when adaptive range is smaller;
- queried resampler requirements matching recreated factories;
- exact network/due-batch factory capacities and owning-constructor overflow rejection.

## Files in the scoped foundation change

- `crates/relay-audio/Cargo.toml` — adds the direct `relay-clock` composition dependency.
- `crates/relay-audio/src/config.rs` — validated policies, exact cadence, derived minima, and factories.
- `crates/relay-audio/src/packet.rs` — crate-private configured-capacity creation boundary.
- `crates/relay-audio/src/lib.rs` — re-exports the owned policy/requirements types.
- `crates/relay-audio/tests/foundation.rs` — boundary matrices and factory checks.
- `docs/research/relay-audio-foundation-fixes.md` — this evidence.

No RX, codec/profile, or Opus implementation was changed by this scoped work. TX integration of the public packet factory was performed by the separately owned TX work while both changes were present in the shared tree.

## Validation evidence

All commands were run from the repository root with the checked-in lockfile:

```text
cargo fmt --all -- --check
PASS

cargo check -p relay-audio --all-targets --all-features --locked
PASS

cargo test -p relay-audio --all-targets --all-features --locked
PASS (foundation matrix: 18/18; all package targets passed)

cargo test --release -p relay-audio --all-targets --all-features --locked
PASS

cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings
PASS

cargo check --workspace --all-targets --all-features --locked
PASS

cargo test --workspace --all-targets --all-features --locked
PASS

cargo test --release --workspace --all-targets --all-features --locked
PASS

cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
PASS
```

## Disposition

F1, F2, and F4 are fixed. F3 is intentionally outside this change's ownership.
