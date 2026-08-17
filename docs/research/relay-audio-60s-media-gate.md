# RELAY Audio 60-Second Media Gate

## Status

**Complete for the 60-second live-stream media subgate; not complete for finite shutdown or full Phase 1.** `crates/relay-audio/tests/media_60s.rs` proves that exactly sixty seconds of live-stream input produce exactly sixty seconds of packet media through the public TX, deterministic-network, RX, scheduled-playback, ring, and renderer APIs at every negotiated Opus frame duration. It freezes the output produced by those live calls.

This evidence does **not** prove a complete finite-input capture image, zero capture trim, an adaptive-playback settling tail, or a complete finite render. Public finite capture finish and adaptive playback drain remain explicit Phase 1 shutdown gates.

The file contains exactly these three tests:

1. `clean_real_public_path_runs_five_ms_with_cross_rate_srcs`
2. `media_60s_10ms_96k_capture_48k_media_44k1_playback`
3. `clean_real_public_path_runs_for_sixty_virtual_seconds`

## Exact live-stream case matrix

Every case generates stereo floating-point PCM over the half-open input interval `[0 s, 60 s)`: a 311 Hz left sine at amplitude 0.22 and a 617 Hz right sine at amplitude 0.17. Ordinary live `process_capture` calls consume the exact input count and report the exact half-open packet-media interval `[0, 2,880,000)` at the fixed 48 kHz Opus/RTP media rate.

| Test / packet duration | Live capture input and chunks | Live capture result | Opus packetization | Playback boundary | Frozen live-call output |
|---|---|---|---|---|---|
| `clean_real_public_path_runs_five_ms_with_cross_rate_srcs` / 5 ms | 44,100 Hz; 2,646,000 frames = 6,000 × 441 = exactly 60 s | fixed 44.1 → 48 kHz SRC reports 2,880,000 media frames | 240 media frames/packet; 12,000 packets | adaptive 48 → 192 kHz SRC | 11,520,568 frames; checksum `0x6fcb_0204_27b1_507b` |
| `media_60s_10ms_96k_capture_48k_media_44k1_playback` / 10 ms | 96,000 Hz; 5,760,000 frames = 6,000 × 960 = exactly 60 s | fixed 96 → 48 kHz SRC reports 2,880,000 media frames | 480 media frames/packet; 6,000 packets | adaptive 48 → 44.1 kHz SRC | 2,646,023 frames; checksum `0xe356_f3d9_2461_8601` |
| `clean_real_public_path_runs_for_sixty_virtual_seconds` / 20 ms | 48,000 Hz; 2,880,000 frames = 6,000 × 480 = exactly 60 s | fixed 48 → 48 kHz live bypass reports 2,880,000 media frames | 960 media frames/packet; 3,000 packets | adaptive 48 → 48 kHz path | 2,880,028 frames; checksum `0x192d_466e_313f_6f7d` |

The output totals and literal checksums freeze only samples reported and published by the live `PlaybackWorker::process_frame` calls. Every such publication is immediately consumed and the ring is empty afterward. The extra 568/23/28 frames relative to nominal 60-second device-rate counts are observed live-call output; without an adaptive-playback finish/drain operation, this gate does not identify them as a complete post-input settling tail or prove that retained converter state was recovered.

### Capture ending is outside this subgate

The capture assertions require every supplied live input frame to be consumed, exactly 2,880,000 media frames to be reported, and exactly 12,000/6,000/3,000 packets to be emitted. The two cross-rate cases therefore prove live-stream count progression, not a complete delay-compensated image of a finite half-open signal. They do not call a TX end/finish operation and do not establish capture trim or recovery of retained filter history. The 48 kHz bypass case is subject to the same intentionally narrow shutdown claim. A public finite capture finish remains required for full Phase 1 shutdown evidence.

## Real codec and SRC composition

`TxWorker::new` constructs the production Opus TX path. Each case uses stereo 48 kHz media, 96,000 bit/s policy, in-band FEC disabled, and a zero packet-loss hint. `process_capture` crosses the public fixed capture converter and real Opus encoder; no synthetic decoded-frame substitute is used.

After deterministic delivery, `RxWorker` validates and decodes each real media packet. Every consumed outcome must be `FrameSource::Packet`; codec errors, FEC attempts, and PLC frames remain zero. The decoded 48 kHz frame then crosses the public adaptive playback converter before complete-or-drop ring publication. `PlaybackRenderer` consumes each live publication and verifies finite, energetic, distinct stereo output.

## Sender/media clock scheduling, not arrival clocking

The media timeline is reconstructed independently of deterministic-network delivery time. For each decoded outcome, the test computes

```text
offset       = extended_sequence - 524,284
media_delta  = offset * packet_frames
rtp_expected = (4,294,966,595 + media_delta) mod 2^32
local_frame  = round(media_delta * playback_rate / 48,000)
```

It asserts the wrapped RTP timestamp, passes the unwrapped media timestamp `4,294,966,595 + media_delta`, and passes only `local_frame` as the scheduled device position to playback. `NetworkTime` is never passed into `process_frame`, the clock observation, or the playback mapping. Arrival controls availability only.

All clean cases now use identical scheduling semantics: packet `i` is scheduled at `(i + 1) × packet_duration`, slot `i` advances to that same time, and the final typed `NetworkTime` is exactly `60,000,000 µs`. Each slot therefore exposes one due packet, and every ingress result is exactly `AcceptedInOrder`; `accepted_reordered` is zero. No loss, duplication, delay variation, or adverse reorder is injected.

The scheduled local mappings are exact on the tested packet grids:

- 5 ms: `local_frame = media_delta × 4` at 192 kHz;
- 10 ms: `local_frame = offset × 441` at 44.1 kHz;
- 20 ms: `local_frame = media_delta` at 48 kHz.

## Mandatory final RX drain and endpoint oracles

RX has one-decision lookahead. After exactly one `tick()` opportunity per packet slot, every case requires

```rust
consume(rx.drain().expect("mandatory final RX drain"));
```

This is an RX lookahead drain only; it is not a capture or adaptive-playback drain. The cases freeze these literal terminal positions:

| Duration | Last extended sequence | Last media delta | Last wrapped RTP timestamp | Last scheduled local start |
|---:|---:|---:|---:|---:|
| 5 ms | 536,283 | 2,879,760 | 2,879,059 | 11,519,040 at 192 kHz |
| 10 ms | 530,283 | 2,879,520 | 2,878,819 | 2,645,559 at 44.1 kHz |
| 20 ms | 527,283 | 2,879,040 | 2,878,339 | 2,879,040 at 48 kHz |

The exact emitted count, RX `deadline_decisions`, and RX `packet_frames` each equal the packet count. The starting points force wire sequence and RTP timestamp wrap.

## Boundedness and frozen no-fault metrics

All packets are scheduled into fixed deterministic-network and due-batch capacities of `packet_count + 16`; the TX batch is `packet_count + 1`, and RX reorder capacity is 64. All packets are resident before virtual delivery, so the exact submitted population exercises the finite network bound.

Every case freezes all public deterministic-network metrics: submitted, scheduled, and delivered copies equal 12,000/6,000/3,000, while simulated drops, duplicate requests/copies, duplicate-capacity rejections, capacity rejections, and time/ordinal overflow rejections are zero.

Every case freezes the parallel public RX endpoint metrics: ingress, accepted-in-order, deadline-decision, emitted-frame, and packet-frame counts equal the packet count; accepted-reordered, duplicates, late, ahead-of-window, identity/duration/timestamp/malformed/oversized/extension rejections, codec errors, FEC attempts, and PLC frames are zero.

Every playback transaction is `Published` with no control fault. Public playback metrics freeze exact input/output frames and published chunks, zero full/disconnected drops and clock discontinuities, the expected controller-update count, and zero resets. These are live-call endpoint metrics, not a playback-drain result.

Playback-ring high-water is measured in interleaved scalar samples after publication and before immediate rendering:

- 5 ms: at most `(240 × 4 + 64) × 2 = 2,048` scalar samples;
- 10 ms: at most `(480 × 44,100 / 48,000 + 64) × 2 = 1,010` scalar samples;
- 20 ms: at most `(960 + 64) × 2 = 2,048` scalar samples.

All three freeze zero dropped samples, zero underrun events, zero underrun samples, and zero samples left in the ring after all live publications have been consumed.

## Validation record and CI disposition

Run from the repository root:

```bash
cargo test --release --locked -p relay-audio --test media_60s -- --nocapture
cargo clippy --release --locked -p relay-audio --test media_60s -- -D warnings
cargo fmt --all -- --check
```

| Gate | Result |
|---|---|
| Locked release test target | **PASS, 3/3; 2.02 s test time** |
| Locked release strict target Clippy | **PASS** |
| Workspace format check | **PASS** |

The previously recorded locked debug run remains a useful periodic/local diagnostic: **PASS, 3/3; 159.40 s test time** (`real 2m39.626s`). Routine CI should keep this real-media target release-only unless that debug cost becomes acceptable. The Clippy result is target-specific, not an all-target/all-feature claim.

## Limits and remaining Phase 1 shutdown gates

- This is a **complete live-stream media subgate**, not full Phase 1 completion.
- Finite capture finish is not exercised. The evidence makes no claim of a complete finite capture image, zero trim, or retained capture-SRC tail recovery.
- Adaptive playback finish/drain is not exercised. The evidence makes no claim of a complete render or a complete playback settling tail.
- The clean deterministic network injects no loss, duplication, delay variation, or adverse reorder. Impairment, FEC/PLC, and drift-sign behavior belong to their dedicated suites.
- The harness calls worker, ring, and renderer APIs in one thread and renders each publication immediately. It does not test hardware, host callback cadence, or worker/callback concurrency.
- Frozen checksums are deterministic regression oracles for the validated toolchain/backend, not audio-quality scores or cross-implementation bit-identity promises.
- Partial final capture chunks are not exercised.

Full Phase 1 shutdown evidence must retain explicit gates for a bounded public finite-capture finish and a bounded public adaptive-playback drain. This document does not invent or imply APIs that do not exist.

## Oracle-change policy

Treat any live output count, literal endpoint, metric, ring high-water, or checksum change as a media-behavior change requiring investigation and explicit review. Do not regenerate checksum constants automatically. Preserve the scheduled sender/media-to-device mapping and do not substitute arrival, socket, or wall time.

## Sources consulted

1. `crates/relay-audio/tests/media_60s.rs` — implemented cases, constants, mappings, metrics, and frozen live-call oracles.
2. `docs/research/review-relay-audio-60s-media-gate.md` — wording and parity findings corrected here.
3. `docs/research/relay-audio-composition-design.md` — real-codec/SRC short gate and finite shutdown requirements.
4. `docs/research/review-relay-audio-playback.md` — scheduled mapping and arrival-time exclusion review.
5. `docs/research/review-relay-audio-loopback.md` — prior final-RX-drain and ingress-status findings.
