# Review dispositions: relay-audio 60-second media gate

## Scope

Disposition of the findings in `docs/research/review-relay-audio-60s-media-gate.md`. Changes are limited to `crates/relay-audio/tests/media_60s.rs` and the 60-second gate evidence. No finite drain API was added or implied.

## Result

**All review wording and parity findings are resolved for the live-stream subgate.** The subgate is complete for exactly 60 seconds of live-stream input and packet media, plus frozen output produced by live calls. Finite capture finish and adaptive playback drain remain explicit, unresolved Phase 1 shutdown gates; therefore this disposition does not mark full Phase 1 complete.

## Finding dispositions

### HIGH — finite capture completion and zero-trim overclaim

**Disposition: corrected by narrowing the claim; finite shutdown remains open.**

- Test comments now say ordinary live calls consume exactly 60 seconds of input and produce exactly 60 seconds of packet media.
- The evidence no longer claims a complete delay-compensated finite capture image, zero trim, or capture-SRC tail recovery.
- No TX end/finish/drain API was invented.
- A bounded public finite capture finish remains an explicit Phase 1 shutdown gate.

### HIGH — playback settling-tail and complete-render overclaim

**Disposition: corrected by narrowing the claim; finite shutdown remains open.**

- Counts and real checksums are unchanged and described only as frozen output produced by live `PlaybackWorker::process_frame` calls.
- The observed extra 568/23/28 frames are not labeled a complete settling tail.
- Ring-empty assertions mean all live publications were consumed; they do not imply adaptive SRC state was drained.
- The mandatory `rx.drain()` remains correctly identified as RX lookahead drain only.
- No adaptive-playback finish/drain API was invented. A bounded public adaptive-playback drain remains an explicit Phase 1 shutdown gate.

### MEDIUM — duplicated-case schedule, endpoint, and ring parity

**Disposition: fixed in all three cases without a core refactor.**

- Packet `i` is scheduled at `(i + 1) × packet_duration`.
- Slot `i` advances to the same time.
- Every typed terminal `NetworkTime` is exactly `60,000,000 µs`.
- Every ingress result is exactly `AcceptedInOrder`; RX metrics freeze the full packet count in order and zero reordered.
- Arrival remains excluded from playback mapping. Playback positions continue to derive only from extended media sequence/timestamp and negotiated rates.
- The 5 and 20 ms cases now freeze literal terminal media and scheduled-local positions, matching the 10 ms case. All three also freeze literal terminal extended sequence and wrapped RTP timestamp.
- The 20 ms case now records a playback-ring high-water and bounds it by `(960 + 64) × 2 = 2,048` interleaved scalar samples.
- The 20 ms case now freezes zero ring underrun events as well as zero underrun samples.
- All three freeze the parallel public deterministic-network, RX, playback, and ring endpoint metrics, including RX deadline/packet-frame counts and playback publication/controller/reset counts.

## Frozen oracles

The scheduling-parity changes preserve media order and scheduled playback positions. Actual output did not change, so the reviewed checksums remain:

- 5 ms: `0x6fcb_0204_27b1_507b`
- 10 ms: `0xe356_f3d9_2461_8601`
- 20 ms: `0x192d_466e_313f_6f7d`

They freeze live-call output only.

## Validation

From the repository root:

- `cargo test --release --locked -p relay-audio --test media_60s -- --nocapture` — **PASS, 3/3; 2.02 s test time**.
- `cargo clippy --release --locked -p relay-audio --test media_60s -- -D warnings` — **PASS**.
- `cargo fmt --all -- --check` — **PASS**.

The prior debug evidence remains diagnostic only and was not rerun for these wording/parity corrections.
