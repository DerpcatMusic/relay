# relay-audio RX core implementation research

## Scope and state-machine outline

This task implements only the bounded 48 kHz receive reorder/decode/FEC/PLC core in
`relay-audio`. The worker is configured from a trusted stream epoch (SSRC, payload
type, initial extended sequence, initial RTP timestamp, and a fixed Opus packet
duration). Ingress first validates duration, SSRC, payload type, and the exact RTP
timestamp implied by the extended sequence; validation occurs before any reorder or
decoder state is mutated. Accepted packets enter a bounded `ReorderBuffer` and are
classified separately as accepted in order, accepted reordered, duplicate, late,
ahead-of-window, ambiguous sequence, or metadata mismatch, with rejected packet
ownership returned to the caller.

Each playout tick pops exactly one reorder decision and advances a one-frame
lookahead state machine:

1. With no pending decision, stage the decision and emit nothing (initial latency).
2. If the pending decision is present, decode and publish its normal frame, then
   stage the current decision.
3. If the pending decision is missing-at-deadline and the current packet is present,
   attempt in-band FEC from the current packet for the prior frame, then decode that
   same current packet normally and stage its decoded frame for the next tick.
4. Libopus may satisfy the FEC-request call with PLC fallback when the packet has no
   recoverable FEC. The public `InbandFecOrPlc` source therefore records the honest
   operation without claiming that LBRR data was present; the current packet is still
   decoded normally and retained for the next tick.
5. For consecutive missing decisions, emit PLC for the prior gap and stage the
   current missing decision; a deadline gap remains distinct from confirmed network
   loss.
6. A malformed current packet is counted and concealed without poisoning the worker
   or panicking. Explicit drain resolves the last staged present frame or PLC gap.

Frames and outcomes use fixed inline/preallocated 48 kHz storage sized for the
maximum supported 20 ms duration. No playout tick allocates. Trusted reset/rebase
constructs a replacement decoder before swapping it into the worker, then clears
reorder history, epoch state, and the pending lookahead decision.

## Primary sources from the composition design

1. RFC 3550, RTP sequence/timestamp and interarrival-jitter semantics:
   <https://www.rfc-editor.org/rfc/rfc3550>
2. Xiph.Org libopus 1.6 decoder API, PLC and in-band FEC contract:
   <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Rubato 4.0 `Resampler` API, fixed/adaptive ratios, delay and caller-buffer
   processing: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html>

## Corrections and implementation decisions

- The receive core accepts only a 48 kHz RTP clock; adaptive sample-rate conversion,
  clock recovery, playback rings, rendering, threads, and I/O are outside scope.
- `MissingAtDeadline` is a playout scheduling observation, not proof of network loss;
  late ingress is measured independently.
- Sequence extension is resolved against the trusted epoch. The half-range case is
  rejected as ambiguous rather than guessed.
- Timestamp validation uses wrapping RTP arithmetic derived from the trusted epoch
  and extended sequence, including u16 sequence and u32 timestamp wrap.
- Decoder reset is treated as fallible off-thread preparation: a new decoder must be
  constructed before any live worker state is cleared.

## Validation and evidence

The final implementation is `crates/relay-audio/src/rx.rs`, exported by the crate
and covered by `crates/relay-audio/tests/rx.rs`. The eight black-box cases exercise:
all 5/10/20 ms durations; initial lookahead and drain; ordered/reordered/duplicate/
late ingress; consecutive-gap PLC; honest FEC-or-PLC request behavior with consecutive
frames from one encoder and with FEC disabled; 16-bit sequence and 32-bit timestamp
wrap; SSRC, payload-type, timestamp, duration, malformed-packet and capacity rejection
with ownership returned; finite PCM; trusted reset; and deterministic metrics.

The worker exposes no packet-arrival time, wall clock, device clock, SRC, playback
ring, or renderer input. Extended `u64` exhaustion stops further timeline decisions
until trusted reset rather than wrapping or saturating. `PendingDecision` deliberately
stores the fixed-inline packet inside the preallocated worker; a scoped Clippy
expectation documents why heap indirection/per-tick allocation is rejected.

Final commands from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`:

```text
cargo fmt --all -- --check
PASS

cargo test -p relay-audio --all-targets --all-features --locked
PASS: 11 unit + 18 foundation + 8 RX + 14 TX tests; 0 failures

cargo test --release -p relay-audio --all-targets --all-features --locked
PASS: same 51 tests; 0 failures

cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings
PASS: no warnings
```

**Disposition:** RX reorder/decode/FEC-or-PLC core passes its Phase 1A scope. Adaptive
SRC, scheduled-playout clock control, bounded playback publication and callback
rendering remain the next composition task.
