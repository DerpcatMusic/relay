# Review: relay-audio 60s media gate

## Scope

Independent review of `crates/relay-audio/tests/media_60s.rs` and `docs/research/relay-audio-60s-media-gate.md` against the approved full-media-path 60-second requirement in `docs/research/relay-audio-composition-design.md`. No source files were edited.

## Status

**FAIL** — 0 critical, 2 high, 1 medium findings.

## Findings

### HIGH — The two cross-rate capture cases do not prove a complete finite 60-second signal, and the evidence overclaims zero trim

The approved design requires finite capture input to finish through `FiniteFixedRatioConverter` rather than silently abandoning the converter ending (`relay-audio-composition-design.md:67`). The 5 ms and 10 ms tests instead construct the live `TxWorker` (`media_60s.rs:309`, `media_60s.rs:566`), submit only ordinary `CaptureInput::Chunk` calls (`media_60s.rs:316-332`, `media_60s.rs:573-589`), and perform no TX end/finish/drain before destroying it. Their count assertions (`media_60s.rs:334-344`, `media_60s.rs:591-601`) prove that 60 seconds of input frames were accepted and that 2,880,000 streaming converter output frames were packetized; they do not prove that the output is the complete delay-compensated image of the finite half-open input interval or that no filter history/tail was abandoned.

The evidence is internally inconsistent on this point. It correctly admits that the tests never call a TX end operation and do not test capture-SRC flush (`relay-audio-60s-media-gate.md:29`, `relay-audio-60s-media-gate.md:124`), but nevertheless claims “actual trim of zero” and that no converter tail or media sample is hidden/discarded (`relay-audio-60s-media-gate.md:25-27`); the test comments repeat that unsupported conclusion (`media_60s.rs:339-341`, `media_60s.rs:596-598`). A frozen checksum cannot repair this because it merely freezes the output of the un-drained live-stream behavior.

Required correction: either use the finite public TX path and assert its explicit leading/trailing trim plus complete output, or narrow the gate/evidence to “60 seconds of live-stream input and 60 seconds of packet media” and stop claiming complete finite-input trim/tail coverage.

### HIGH — RX drain is tested, but playback SRC drain/settling is not; “complete render” and “settling tail” are unsupported

The design's shutdown contract requires draining both RX pending FEC state and SRC state (`relay-audio-composition-design.md:84-85`, `relay-audio-composition-design.md:106`). Each case does perform the mandatory final `rx.drain()` (`media_60s.rs:199`, `media_60s.rs:451`, `media_60s.rs:710`), which correctly exposes the last staged RX frame. However, every playback publication is rendered immediately inside `consume`, and after the last RX frame the tests only inspect counts/empty-ring state; there is no playback-converter finish/drain operation.

Consequently, the comments that the excess 28/568/23 device frames are a playback-SRC “settling tail” (`media_60s.rs:204-206`, `media_60s.rs:456-458`, `media_60s.rs:723-725`) and the evidence's claims of a “Frozen complete render” and complete settling tails (`relay-audio-60s-media-gate.md:17-23`) are not established by the exercised calls. The exact counts and literal checksums are useful deterministic streaming regression oracles, but they neither identify those frames as post-input settling output nor prove that the playback SRC's retained state was drained.

Required correction: exercise an explicit bounded playback/SRC drain and freeze its final accounting, or describe the current totals only as output produced during the 2,880,000 input-frame streaming calls and list playback-SRC tail recovery as untested.

### MEDIUM — Duplicated case bodies have already diverged in required ring and endpoint oracles

The three independently copied test bodies are not semantically parallel:

- only 5/10 ms record a playback-ring high-water and assert `ring.underruns == 0` (`media_60s.rs:376`, `media_60s.rs:502-509`, `media_60s.rs:635`, `media_60s.rs:769-777`); 20 ms checks only dropped/underrun **sample** counts (`media_60s.rs:250-252`), omitting the underrun-event metric and any high-water bound;
- only 10 ms freezes literal final media/local positions (`media_60s.rs:712-720`) and pins its final due time to exactly 60,000,000 microseconds (`media_60s.rs:622`); 5/20 derive final sequence/timestamp correctly but lack equivalent literal endpoint oracles;
- 5/20 schedule at `index * duration` and advance with a four-slot lead (`media_60s.rs:351-355`, `media_60s.rs:431-450`, `media_60s.rs:107-113`, `media_60s.rs:179-198`), while 10 ms schedules/advances at `(index + 1) * duration` (`media_60s.rs:608-620`, `media_60s.rs:690-709`). This explains the divergent clean acceptance metrics (one in-order plus the remainder “reordered” for 5/20 versus entirely in-order for 10 ms), but it means the cases are not testing the same clean scheduling semantics.

The evidence acknowledges the missing 20 ms high-water/event checks (`relay-audio-60s-media-gate.md:84-89`, `relay-audio-60s-media-gate.md:128`) and correctly warns that `accepted_reordered` is a reorder-buffer classification, not injected network reordering (`relay-audio-60s-media-gate.md:95`). Thus there is no false claim of adverse network reorder, but “parity” is not complete. Consolidate the common scenario driver and require duration/rate-specific parameters plus one shared set of endpoint, metric, and high-water assertions. High-water units in the existing 5/10 ms checks are correct: `renderer.available_samples()` is bounded in interleaved scalar samples, and both formulas multiply a frame bound by `CHANNELS` (`media_60s.rs:506-509`, `media_60s.rs:773-776`).

## Requirements audit

- **Exact 60-second count progression:** PASS as streaming count/accounting. Capture assertions pin 2,646,000 at 44.1 kHz, 5,760,000 at 96 kHz, and 2,880,000 at 48 kHz; all cases pin 2,880,000 48 kHz media frames and 12,000/6,000/3,000 packets (`media_60s.rs:93-100`, `media_60s.rs:334-344`, `media_60s.rs:591-601`). This does not cure the finite-tail finding above.
- **Public real codec and SRC path:** PASS. The cases use public `TxWorker::new/process_capture`, packet ingress/`RxWorker::tick`, `PlaybackWorker::process_frame`, and renderer calls; every emitted source is required to be `FrameSource::Packet` and codec errors/FEC/PLC are zero. The 5/10 ms cases exercise non-unity capture and playback SRC; the 20 ms case intentionally covers the design's 48 kHz capture bypass and adaptive 48→48 path (`relay-audio-composition-design.md:116`, `media_60s.rs:39-68`, `media_60s.rs:280-309`, `media_60s.rs:537-566`).
- **Bounded, non-vacuous network:** PASS for the clean scenario. Network/due capacities are fixed at packet count + 16, all packets are resident before delivery, all scheduling outcomes are required accepted, and submitted/scheduled/delivered plus every network rejection/error metric are exact (`media_60s.rs:49-50`, `media_60s.rs:107-119`, `media_60s.rs:216-226`, with parallel 5/10 ms assertions). This proves a finite near-capacity population, not impairment/overload behavior.
- **Scheduled timestamp/local mapping excludes arrival:** PASS. Media delta comes from extended sequence, the wrapped RTP timestamp is asserted, and playback receives unwrapped media position plus a rate-derived scheduled local frame; `NetworkTime` is never used for the playback mapping (`media_60s.rs:139-155`, `media_60s.rs:384-406`, `media_60s.rs:643-665`).
- **Final tick/drain/count/sequence/timestamp/wrap:** PASS. There is exactly one tick opportunity per packet slot followed by mandatory RX drain, exact emitted counts, extended final sequence, and a wrapping final RTP timestamp in all cases (`media_60s.rs:179-211`, `media_60s.rs:431-463`, `media_60s.rs:690-730`). Sequence and timestamp starting points force wire wraps.
- **Frozen oracle:** PASS. Each checksum is a literal constant, not a run-versus-self comparison (`media_60s.rs:254`, `media_60s.rs:266`, `media_60s.rs:511`, `media_60s.rs:523`, `media_60s.rs:779`).
- **Finite, nontrivial stereo:** PASS for produced streaming output. Every rendered frame must be finite, both channel energies exceed 1, and channel difference exceeds 1 (`media_60s.rs:162-172`, `media_60s.rs:212-213`, with equivalent 5/10 ms checks).
- **Error/rejection/ring accounting:** PASS for network and RX rejection/error classes, playback publication/control faults, drop/disconnect/clock-discontinuity counters, and ring sample losses; FAIL parity because 20 ms omits `ring.underruns` and high-water. RX `deadline_decisions`/`packet_frames` and playback `published_chunks`/`controller_updates`/`resets` are also not frozen, although per-frame source/publication and total emitted/input/output assertions indirectly constrain the primary clean path.
- **Clean-only limitation:** PASS/honest. The evidence explicitly states that this gate injects no loss, duplication, delay variation, or adverse reorder, and does not claim hardware/callback concurrency (`relay-audio-60s-media-gate.md:119-124`).
- **Performance/tooling evidence:** PASS. The documented debug result remains parent-proven at 159.40 s and is explicitly relegated to periodic/local diagnostics; the release-only CI recommendation and target-specific Clippy limitation are clear (`relay-audio-60s-media-gate.md:99-117`).

## Validation performed

From the repository root:

- `cargo test --release --locked -p relay-audio --test media_60s -- --nocapture` — **PASS, 3/3**, 2.02 s test time (2.07 s wall observed).
- `cargo clippy --release --locked -p relay-audio --test media_60s -- -D warnings` — **PASS**.
- `cargo fmt --all -- --check` — **PASS**.
- Debug was not rerun; the supplied parent-proven result is **PASS, 3/3, 159.40 s**.
