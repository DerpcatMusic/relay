# Audio Jitter Acceptance Fixes

## Disposition

**Pass.** All acceptance findings from `docs/research/review-audio-jitter.md` are resolved in `crates/relay-jitter`.

## Fixes

### Playout gaps are not network-loss truth

- Renamed `Playout::Loss` to `Playout::MissingAtDeadline`.
- The enum and `pop_at_deadline` documentation define the result as a playout-timing fact requiring concealment, not proof of network loss.
- The public documentation explicitly forbids copying this event directly into RTCP network-loss accounting and assigns reconciliation to a source-statistics layer with its own reporting horizon.

### Late and duplicate classification has bounded history

- Replaced the emitted-only markers with fixed-capacity per-extended-position history containing `Emitted`, `MissingAtDeadline`, or `LateSeen`.
- Within the most recent `capacity()` deadline decisions, the first packet arriving after its missed deadline is `RejectedPacket::Late`; a repeated copy is `RejectedPacket::Duplicate`.
- History is a preallocated boxed slice of exactly `capacity()` entries. `push` and `pop_at_deadline` remain allocation-free O(1); the reporting horizon and memory remain bounded.

### Trusted reset/rebase seam

- Added `ReorderBuffer::reset_and_rebase(next_sequence)`.
- It drops all queued packets, clears all classification history and burst state, and sets the next playout sequence.
- Its API documentation restricts it to an explicitly trusted local transport/control worker after source restart or discontinuity validation. It is O(capacity), may run packet destructors, must not run on a real-time audio hot path, and must never be triggered merely by surprising remote sequence input.
- `push` still cannot advance or reset the playout head, including for ahead-of-window and ambiguous half-range input.

### Explicit target-delay observation cadence

- Added validated `TargetDelayConfig::observation_interval` and `TargetDelayPolicy::observation_interval()`.
- Zero cadence returns `TargetDelayConfigError::ZeroObservationInterval`.
- Public configuration, policy, signal, and `observe` documentation require exactly one coalesced signal per fixed interval. The adapter aggregates packet events into one pressure signal (using the interval's required delay) or one stable signal; it must not call once per packet.

## Adversarial coverage

The unit suite now covers:

- ordinary `65_535 -> 0` wrap and more than two complete sequence-space traversals;
- both sides of the exact 32,768 half-range ambiguity;
- capacities 1 and 32,767, including the farthest storable position at maximum capacity;
- first late arrival followed by a repeated duplicate;
- trusted reset dropping retained packets, clearing emitted history and burst state, and rebasing;
- explicit cadence-driven hysteresis and rejection of a zero observation interval;
- existing reorder, ahead-of-window, duplicate, missing-burst, clamping, and hysteresis behavior.

## Real-time and safety disposition

- `push`, `pop_at_deadline`, and `TargetDelayPolicy::observe`: O(1), allocation-free after construction, no scans, locks, I/O, or unsafe code.
- `reset_and_rebase`: deliberately O(capacity), trusted worker/control operation only, not a real-time hot-path operation.
- Memory: O(capacity), fixed at construction; capacity remains limited to 32,767.
- Crate retains `#![forbid(unsafe_code)]`.

## Validation

Run from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` after the final source edit:

```text
cargo test -p relay-jitter --locked
```

Result: **pass** — 15 unit tests passed, 0 failed; 0 doc tests.

```text
cargo clippy -p relay-jitter --all-targets --all-features --locked -- -D warnings
```

Result: **pass** — exit 0, no warnings.

```text
cargo fmt --all -- --check
```

Result: **pass** — exit 0, no formatting differences.
