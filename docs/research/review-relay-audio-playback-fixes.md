# relay-audio playback review fixes

## Review addressed

This disposition addresses `review-relay-audio-playback.md`. The review found no
critical/high algorithmic source defect, but requested production-reachable
timestamp-domain evidence and a real composed loop before accepting playback.

## Fixes

1. **Valid media timeline in the module sign test.** The former synthetic
   `481`-tick remote step paired a 480-frame `PcmFrame` with an RX-impossible
   RTP timeline. It now keeps every 10 ms remote step at exactly 480 ticks and
   expresses positive drift only through a 479-device-frame scheduled mapping.
   The expected output/input correction remains negative.
2. **Real public composition.** `tests/loopback.rs` now drives real capture PCM,
   fixed capture SRC, canonical Opus, bounded deterministic network, RX
   reorder/decode, scheduled playback SRC, SPSC ring, and renderer. It covers
   supported rates/durations pairwise, deterministic output, real-codec
   `InbandFecOrPlc`, explicit PLC, duplicate/reorder/loss, both wire wraps,
   bounds/metrics, and final lookahead drain.
3. **Production-reachable drift signs.** A focused real-RX test keeps valid
   fixed-duration RTP increments and maps them to scheduled local device frames
   at +400, zero, and -400 ppm. With fill gains disabled only in this test
   pipeline to isolate feed-forward, it asserts estimator signs and the
   reciprocal output/input correction signs. `NetworkTime` remains solely a
   delivery input.
4. **Formatting gate.** The temporary skeleton import-order failure was removed
   by rustfmt when the substantive test replaced the skeleton.

## Independent validation

```text
cargo test --locked -p relay-audio --test loopback
PASS: 3/3
cargo test --locked -p relay-audio --lib   playback::tests::scheduled_remote_drift_produces_the_correct_output_input_sign
PASS: 1/1
```

The independent loopback review and the 12-virtual-hour convergence/bounds gate
remain pending. Their findings and the final coherent workspace validation are
appended before this review is closed.

## Disposition

The review's production-domain and composed-lifecycle gap is substantively
fixed. Final acceptance remains pending the independent loopback review and
long-run deterministic gate.
