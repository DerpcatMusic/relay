# Independent review: `relay-jitter`

## Disposition

**Conditional pass, with integration blockers.** The fixed-capacity ring and its modular comparisons are internally coherent and bounded, including at ordinary `u16` wrap. No critical or high-severity memory-safety, panic, allocation-growth, or ring-aliasing defect was found. Before production RTP statistics or restart handling use this API, the owner should resolve the two medium-severity classification gaps and provide a locally authorized discontinuity/reset path.

This was a read-only review of `crates/relay-jitter`; no implementation files were changed.

## Sources (three total)

1. [RFC 3550](https://www.rfc-editor.org/rfc/rfc3550), §6.4.1 and Appendix A.1 — extended sequence counts, modular wrap, source probation, bounded dropout/misorder, and restart validation.
2. [RFC 3611](https://www.rfc-editor.org/rfc/rfc3611), §§4.2 and 4.7.1–4.7.2 — duplicate reporting and the distinction between network loss and jitter-buffer discard/late arrival.
3. [RFC 7005](https://www.rfc-editor.org/rfc/rfc7005), §§3.3 and 4.2 — adaptive de-jitter buffers, nominal/maximum delay, and adaptation in response to a significant proportion of late packets.

These RFCs provide invariants and reporting terminology; they do not prescribe this crate's particular ring or hysteresis implementation.

## Severity-ranked findings

### Medium — An absent packet is finalized as `Loss` at the first playout deadline, then the same sequence may also be classified `Late`

**Evidence:** `Playout::Loss` is documented as a missing packet that "is now classified as lost" (`crates/relay-jitter/src/lib.rs:105-110`). Every empty expected slot immediately produces that result (`crates/relay-jitter/src/lib.rs:239-273`), while a later copy is separately rejected as `Late` (`crates/relay-jitter/src/lib.rs:221-231`, and the contract at `crates/relay-jitter/src/lib.rs:234-238`).

**Impact:** The playout action is correct—audio must conceal an absent packet at its deadline—but the name is too strong for network-loss accounting. At that instant the receiver cannot know whether the packet was lost on the network or will arrive late. A consumer that increments a network-loss counter on `Playout::Loss` and a discard/late counter on the subsequent rejection will double-classify one sequence. RFC 3611 §4.7.1 calls for separate network-loss and jitter-buffer-discard metrics; it permits sufficiently late packets to be categorized as lost, but says the loss threshold should be significantly greater than the discard threshold. This API exposes only the first playout deadline.

**Potential correction:** Rename the event to `MissingAtDeadline`, `PlayoutGap`, or similarly explicit playout terminology. Keep concealment/burst state local to playout, but let a source-statistics layer reconcile eventual late arrival versus network loss over a defined reporting horizon. If `Loss` is retained, document it strictly as *playout loss*, prohibit direct use as RTCP network loss, and specify counter reconciliation.

### Medium — Repeated copies of a late packet are all reported as `Late`, not one late arrival followed by duplicates

**Evidence:** Duplicate detection behind the head consults only `recently_emitted` (`crates/relay-jitter/src/lib.rs:221-230`). A missing deadline clears that marker (`crates/relay-jitter/src/lib.rs:266-273`), and rejecting the first late arrival records no observation. Therefore, after sequence 101 is missed, two later pushes of 101 both return `Late`. The current test covers only one late arrival (`crates/relay-jitter/src/lib.rs:543-564`). RFC 3611 §4.2 defines a later occurrence of an already observed sequence within the reporting period as a duplicate, even when it is not adjacent.

**Impact:** Playout remains safe, but duplicate, late, and discard telemetry can be inflated or understated under packet replication or adversarial replay. The same limitation applies to repeatedly rejected ahead-of-window packets: because no accepted observation is recorded, they never become duplicates.

**Potential correction:** Define whether classification is a storage outcome or an RTP observation taxonomy. If the latter, retain bounded per-extended-position observation state (for example, emitted/missed/late-seen) so the first post-deadline copy is `Late` and later copies are `Duplicate`. Keep the reporting horizon explicitly bounded; do not attempt unbounded sequence history.

### Medium — Large discontinuities have no recovery operation in this crate

**Evidence:** An ahead packet at distance `>= capacity` is rejected (`crates/relay-jitter/src/lib.rs:192-199`), and the documentation deliberately forbids `push` from advancing playout (`crates/relay-jitter/src/lib.rs:169-173`). The only head advance is one position per `pop_at_deadline` (`crates/relay-jitter/src/lib.rs:234-279`). There is no clear/rebase/reset method.

**Impact:** A legitimate sender restart, SSRC/source reset routed incorrectly, or long dropout can leave all new packets rejected until the caller reconstructs the buffer or emits enough deadline losses for the old head to catch up. Reconstruction allocates, and draining can create a long concealment run. The design correctly prevents a single remote jump from forcing an automatic reset, but it does not provide the trusted local control needed to recover. RFC 3550 Appendix A.1 uses probation and a two-packet restart confirmation and then resets sequence statistics; that source validation is outside this crate, but it still needs a recovery seam into the crate.

**Potential correction:** Add an explicitly caller-authorized reset/rebase operation, invoked only after SSRC-aware transport validation. Specify whether it drops queued packets and resets loss/duplicate history. O(capacity) clearing is bounded but not O(1); if reset must be constant time, use generation tags with overflow behavior tested rather than allowing an untrusted packet to trigger a scan.

### Low — Target adaptation depends on call count, with no observation-cadence contract

**Evidence:** Every `Pressure` call increments by at least `increase_step`, even when `required_delay` is below the current target (`crates/relay-jitter/src/lib.rs:430-438`). Every `stable_observations_before_decrease` calls lower the target (`crates/relay-jitter/src/lib.rs:439-447`). The API carries neither elapsed time nor interval identity, and its docs do not define the expected cadence.

**Impact:** Bounds and arithmetic are safe, but semantically identical observations delivered at different rates produce different targets. Repeated pressure notifications can ratchet immediately to `max_delay`; repeated stable notifications can retract immediately in wall-clock time. RFC 7005 §3.3 speaks in terms of a significant proportion of late packets, which requires a measurement interval or equivalent aggregation. This is principally an integration/tuning risk because callers, not network input, invoke `observe`.

**Potential correction:** Specify one observation per fixed interval and coalesce packet events in the adapter, or make elapsed interval/evidence count explicit in the API. Define how late, reorder, burst loss, and `required_delay` are aggregated. Add fake-clock tests proving equivalent behavior under different packet arrival batching.

### Low — Boundary and adversarial coverage is thinner than the core invariants

**Evidence:** The suite checks one `65_535 -> 0` wrap with capacity 10 and one exact half-range rejection (`crates/relay-jitter/src/lib.rs:480-505`, `crates/relay-jitter/src/lib.rs:585-598`), but not both sides of the half range, capacity 1/32,767, repeated wraps, randomized reorder/loss traces, repeated late duplicates, or discontinuity recovery.

**Potential correction:** Add model/property tests over multiple small capacities and starting sequences; assert ordered playout, no two live extended positions share a slot, occupancy never exceeds capacity, exact half-range always rejects, and classification remains stable across wrap. Add explicit regression tests for the findings above.

## Confirmed invariants

- **Half-range handling is correct locally:** `forward == 32_768` is explicitly ambiguous; values below it are ahead and values above it are behind (`crates/relay-jitter/src/lib.rs:184-230`). Capacity is restricted to `1..=32_767` (`crates/relay-jitter/src/lib.rs:14-15`, `crates/relay-jitter/src/lib.rs:136-142`).
- **Extended-position indexing avoids the non-divisor wrap bug:** live slots are indexed by `expected_extended + distance`, not raw `u16 % capacity` (`crates/relay-jitter/src/lib.rs:201-205`, `crates/relay-jitter/src/lib.rs:281-300`). A live window of at most `capacity` consecutive extended positions maps injectively modulo `capacity`.
- **Storage and hot-path work are bounded:** construction performs two capacity-sized allocations; `push`, `pop_at_deadline`, and `observe` are O(1) and do not resize or scan (`crates/relay-jitter/src/lib.rs:114-119`, `crates/relay-jitter/src/lib.rs:144-154`, `crates/relay-jitter/src/lib.rs:384-389`). Memory is O(capacity), with capacity capped at 32,767 packets.
- **Adversarial duration arithmetic is bounded:** target growth uses saturating addition and clamps to `max_delay`; shrink uses saturating subtraction and clamps to `min_delay` (`crates/relay-jitter/src/lib.rs:434-447`). The stable counter also saturates.
- **The ring does not let an arbitrary sequence jump advance the head:** out-of-window and exact-half-range values return owned rejections, while only the caller-selected deadline advances state.

## Validation

Run from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`:

```text
cargo test -p relay-jitter --locked
```

Result: **pass** — 9 unit tests passed, 0 failed; 0 doc tests.

```text
cargo clippy -p relay-jitter --all-targets --all-features --locked -- -D warnings
```

Result: **pass** — exit 0, no warnings.

## Recommended acceptance gate

Accept the bounded ring implementation as a Phase-1 primitive after documenting that `Loss` means a playout gap, not automatically an RTCP network loss. Before RTP integration, require (1) a source-validated reset/rebase seam, (2) a deliberate bounded policy for duplicates of rejected late packets, and (3) an observation-cadence contract for target-delay inputs.
