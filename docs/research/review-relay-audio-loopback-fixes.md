# relay-audio loopback review fixes

## Review addressed

This record resolves H1, H2, M1, and tracks M2 from
`review-relay-audio-loopback.md`.

## Corrections

### H1 — final lookahead is mandatory

The harness now requires `RxWorker::drain()` to return the final staged frame,
asserts exactly one emitted output per encoded timeline position, checks
`RxMetrics::emitted_frames`, and binds the final extended sequence and wrapped
RTP timestamp to `encoded_packets - 1`. A missing/truncated final frame can no
longer pass through conditional consumption.

### H2 — unexpected ingress rejection fails immediately

Accepted in-order/reordered statuses remain valid. Only the one deliberate
network duplicate may return `IngressMismatch::Duplicate`; every other rejection
panics the test with its typed status. The harness also requires zero identity,
duration/timestamp, malformed, oversized, extension, late, ahead-of-window, and
codec-error metrics, exact accepted/ingress counts, and exact duplicate count.

### M1 — impairment positions are bound to source outcomes

Each result retains `(extended_sequence, wrapped_timestamp, FrameSource)`. The
fault test asserts dropped packet 4 and the second packet of the 7/8 hole use
`InbandFecOrPlc`, packet 7 uses explicit `PacketLossConcealment`, exactly two
FEC operations are attempted, and exactly one explicit PLC frame is counted.
The name intentionally remains honest about libopus LBRR observability.

### M2 — all-target gate

The review correctly observed that the concurrent 12-hour skeleton made the
package-wide strict gate red. That separate file is being replaced by the
substantive long-run gate; no lint suppression was added. Final all-target and
workspace results are recorded only after that recovery completes.

## Focused validation

```text
cargo test --locked -p relay-audio --test loopback
PASS: 3/3
cargo clippy --locked -p relay-audio --test loopback -- -D warnings
PASS
cargo fmt --all
PASS
```

## Disposition

H1, H2, and M1 are fixed with direct assertions. M2 remains an integration gate,
not a loopback defect, until the active 12-hour test replaces its skeleton and
the coherent package/workspace validation passes.
