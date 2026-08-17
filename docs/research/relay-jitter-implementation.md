# `relay-jitter` worker-side policy implementation

Status: implemented and isolated-crate validated.

## Scope

This Phase-1 slice implements only worker-owned policy primitives:

- a fixed-capacity generic `ReorderBuffer<T>` for one RTP sequence-number stream;
- deterministic in-order/reordered, duplicate, late, ahead-of-window, and ambiguous-distance classification;
- explicit playout-deadline decisions that classify an absent expected packet as loss and count consecutive-loss bursts; and
- a bounded `TargetDelayPolicy` with explicit minimum/maximum/initial delay, immediate growth, and slow hysteretic shrink.

It intentionally contains no RTP parsing, SSRC/source validation, arrival-time estimator, codec, depacketizer, FEC, socket, async runtime, or network integration. The media worker chooses when a playout deadline occurs and supplies the policy's pressure/stability observations.

## Primary sources reviewed

Research was limited to these three IETF standards:

1. [RFC 3550, *RTP: A Transport Protocol for Real-Time Applications*](https://www.rfc-editor.org/rfc/rfc3550) — §6.4.1 defines the interarrival-jitter estimator and extended highest sequence; Appendix A.1 demonstrates modular 16-bit sequence validation, wrap-cycle tracking, and distinct dropout/misorder bounds. The implementation follows its modular-wrap premise, but uses a deliberately smaller caller-configured reorder window and rejects the exactly-half-range comparison as ambiguous.
2. [RFC 3611, *RTP Control Protocol Extended Reports (RTCP XR)*](https://www.rfc-editor.org/rfc/rfc3611) — §§4.7.1–4.7.2 distinguish network loss from jitter-buffer discard/late arrival and describe loss/discard bursts. This implementation therefore declares loss only when the local caller advances an absent playout position; a later arrival is classified as late, duplicates are separate, and consecutive deadline losses expose a bounded burst counter.
3. [RFC 7005, *RTCP XR Block for De-Jitter Buffer Metric Reporting*](https://www.rfc-editor.org/rfc/rfc7005) — §3.3 describes adaptive de-jitter buffering that begins at low delay and extends when a significant proportion of packets arrive late; §4.2 distinguishes nominal and maximum delay. The implemented controller makes its current target and hard maximum separate, grows immediately under pressure, clamps all inputs, and retracts only after a configurable stable interval.

The RFC algorithms are references for sequence/taxonomy invariants, not copied as a complete receiver. In particular, RTP source restart/probation and RTCP reporting belong outside this crate.

## Design and invariants

### Sequence reorder core

`ReorderBuffer<T>::new(capacity)` validates `1..=32_767`. Keeping the live window below half of the `u16` serial space makes forward/backward ordering unambiguous; an exact distance of 32,768 receives an explicit `AmbiguousSerialDistance` rejection.

The buffer uses an internal monotonically extended position for slot indexing. This is important when the configured capacity does not divide 65,536: indexing directly by `u16_sequence % capacity` would alias distinct live positions across sequence wrap. Tests exercise wrap with a non-power-of-two capacity.

`push(sequence, packet)` has these outcomes:

- the expected sequence is `InOrder`;
- a sequence ahead but inside the fixed window is `Reordered { depth }`;
- a sequence already buffered or recently emitted is `Duplicate`;
- a sequence behind the playout head and not recently emitted is `Late`;
- an ahead sequence that cannot fit is `AheadOfWindow`; and
- an exactly-half-range sequence is `AmbiguousSerialDistance`.

Rejected calls return ownership of `T`. They do not resize, advance playout, or panic. An arbitrary remote jump cannot force the receiver to discard queued packets or synthesize an unbounded loss run.

`pop_at_deadline()` is the only operation that advances the playout head. It returns the expected packet when present or a `Loss` with a saturating consecutive burst length when absent. The transport/media worker, not remote arrival, owns this timing decision. A loss decision is final: a later packet for that sequence is late and cannot reopen history.

### Target-delay policy

`TargetDelayConfig` validates inclusive minimum/maximum bounds, an in-range initial target, non-zero increase/decrease steps, and a non-zero stable-observation threshold.

For every `DelaySignal::Pressure` observation, the policy:

1. clears accumulated stability;
2. raises the target immediately by at least `increase_step`, or to `required_delay` if larger; and
3. clamps to `max_delay`, including adversarial `Duration::MAX` input.

For `DelaySignal::Stable`, it holds the target until `stable_observations_before_decrease` consecutive stable observations, then lowers it by one `decrease_step` and clamps to `min_delay`. Any intervening pressure resets the shrink interval. The deliberately asymmetric steps plus consecutive-stability gate prevent rapid `20 → 60 → 20 → 60 ms` oscillation.

The policy consumes observations rather than computing arrival variance itself. This preserves separation between RTP/timestamp measurement and latency policy and makes the state machine deterministic under a fake clock.

## Deterministic coverage

Unit tests cover:

- `u16` wraparound with a non-power-of-two storage capacity;
- out-of-order insertion followed by ordered playout;
- duplicates both while buffered and after playout;
- consecutive loss-burst lengths, recovery/reset, and a late packet after loss;
- bounded rejection of ahead-of-window and half-range input;
- immediate target growth and maximum clamping, including `Duration::MAX`;
- slow target shrink to the minimum; and
- hysteresis reset under intermittent pressure, with no premature shrink/oscillation.

Invalid capacity/policy configurations return typed errors rather than panicking.

## Complexity and allocation behavior

| Operation | Time | Allocation / growth |
|---|---:|---|
| `ReorderBuffer::new(C)` | O(C) | Two fixed heap allocations (`slots`, recent-history metadata) |
| `ReorderBuffer::push` | O(1) | None; one bounded slot lookup |
| `ReorderBuffer::pop_at_deadline` | O(1) | None; one bounded slot lookup |
| `TargetDelayPolicy::new/observe` | O(1) | None |

Memory is O(C) and immutable in size after construction. No post-construction operation grows a collection, scans a gap, logs, performs I/O, locks, or uses `unsafe`. Arithmetic that can be influenced by remote input uses wrapping/saturating operations and explicit clamps. Packet destruction can run `T`'s destructor, which is another reason this worker-owned type must not be moved onto the audio callback.

## Potential master-plan corrections / integration decisions

These are recorded, not applied in this focused task:

1. **Define source discontinuity ownership.** RFC 3550 source probation/restart/large-jump validation should live in the RTP transport adapter (with SSRC context). The master plan should not imply that the reorder window silently accepts a restart. A later integration API needs an explicit, locally authorized reset/discontinuity operation.
2. **Specify loss/discard taxonomy.** “Late packet detection” and “loss classification” should explicitly distinguish network loss (absent at the local playout deadline), late jitter-buffer discard, duplicate discard, and local window-overload rejection, consistent with RFC 3611. These counters should not be collapsed into one loss percentage.
3. **Define observation cadence and pressure estimator.** The profile ranges (~10–25, ~20–50, ~40–100 ms) do not yet specify sampling interval, quantile/arrival-variance estimator, or how burst loss maps to `required_delay`. Those need deterministic fake-clock contracts before tuning production constants.
4. **Keep network timing separate from drift correction.** The target-delay policy must consume network-pressure observations; the ASRC/clock controller must continue to handle slow clock drift rather than feed per-packet jitter into resampling.
5. **Workspace integration is deferred.** Because the root `Cargo.toml` was explicitly out of scope, this crate has a temporary empty `[workspace]` table for isolated validation. The integration change should add `crates/relay-jitter` to root workspace members and then remove that nested table.
6. **Reorder before depacketization, after RTP validation.** The receiver diagram's combined “reorder/depacketize” box should be read as source validation/parser → sequence reorder → payload depacketization, avoiding codec-aware behavior in this policy crate.

## Validation

Run from repository root:

```text
cargo fmt --manifest-path crates/relay-jitter/Cargo.toml -- --check
cargo test --manifest-path crates/relay-jitter/Cargo.toml
cargo clippy --manifest-path crates/relay-jitter/Cargo.toml --all-targets -- -D warnings
```

Final result: formatting clean; 9 unit tests and 0 doc tests passed; Clippy completed with warnings denied.
