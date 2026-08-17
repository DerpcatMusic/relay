# Independent Audit — `relay-audio` RX Core

## Scope and evidence constraints

- [x] Audited only the current RX core (`crates/relay-audio/src/rx.rs`) and its black-box RX tests (`crates/relay-audio/tests/rx.rs`), consulting the directly used timeline/reorder and Opus wrapper code only to verify invariants at those seams.
- [x] Used no external source beyond the three already listed in `docs/research/relay-audio-rx-core-implementation.md`: RFC 3550, the Xiph.Org libopus 1.6 decoder API, and Rubato 4.0's `Resampler` API.
- [x] Made no implementation edits.

## Executive disposition

**Disposition: conditionally acceptable core, but the evidence overstates test coverage.** I found no critical or high-severity functional defect in the inspected implementation. The separate extended-sequence and reorder heads advance coherently in the current code; RTP sequence/timestamp wrap arithmetic is consistent with the fixed-duration epoch contract; validation precedes reorder/decoder mutation; and the FEC call precedes normal decode of the same following packet as required by libopus. Reset, drain, and `u64` exhaustion logic are internally coherent.

The release claim should nevertheless be narrowed until the medium findings below are corrected. In particular, none of the decoder-error containment branches are exercised, the exact decoder call sequence is not observable in tests, and the claimed capacity/ambiguity/exhaustion and comprehensive deterministic-metrics coverage does not exist.

## Severity-ranked findings

### Medium — F1: the RX evidence materially overclaims boundary and metrics test coverage

**References**

- `docs/research/relay-audio-rx-core-implementation.md:63-75` claims capacity rejection, deterministic metrics, `u64` exhaustion, and a scoped large-enum expectation are covered/evidenced.
- `crates/relay-audio/tests/rx.rs:347-420` covers SSRC, payload type, timestamp, duration, and a statelessly malformed payload, but not `PacketTooLarge`.
- `crates/relay-audio/tests/rx.rs:423-473` covers only a two-packet forward `u16`/`u32` wrap.
- The test file contains no assertion for `AheadOfWindow`, `AmbiguousSequence`, `SequenceBeforeEpoch`, `SequenceOverflow`, or timeline exhaustion, and no assertion for most `RxMetrics` fields.
- `crates/relay-audio/src/rx.rs:495-525` and `529-554` contain the untested extension/exhaustion branches.

**Impact**

The current eight tests do not establish the advertised behavior at the exact serial half-range, before the trusted epoch, at the reorder-capacity edge, or near `u64::MAX`. Those are precisely the cases most likely to expose disagreement between the full-width head and the reorder buffer's wire head. The evidence also says “capacity rejection” although no RX test configures a packet bound smaller than `MAX_PACKET_BYTES` or sends an offset at/over reorder capacity.

**Exact correction**

Add table-driven black-box cases for:

1. accepted offsets `capacity - 1`, rejected `capacity`, and accepted/rejected positions straddling `u16` wrap;
2. exact serial distance `0x8000` (`AmbiguousSequence`);
3. a nearest sequence before epoch zero (`SequenceBeforeEpoch`);
4. `initial_sequence` near `u64::MAX`, including final decision, drain, subsequent `tick() == None`, rejected forward wire sequence as `SequenceOverflow`, and recovery after reset;
5. timestamp wrap for late and far-ahead packets, not only three consecutive accepted packets;
6. a pipeline packet capacity below `MAX_PACKET_BYTES`, proving `PacketTooLarge` ownership return;
7. before/after snapshots for every metrics field relevant to each result.

Alternatively, correct `docs/research/relay-audio-rx-core-implementation.md:63-75` to state only the coverage actually present.

**Disposition:** must correct evidence; should add the tests before treating the boundary contract as release-proven.

### Medium — F2: decoder ordering, decoder-error concealment, and post-error codec usability are not directly tested

**References**

- Required FEC order is implemented at `crates/relay-audio/src/rx.rs:573-599`: `decode_fec(following)` then `decode(following)` into staged storage.
- Normal-decode and FEC errors fall back through explicit PLC at `crates/relay-audio/src/rx.rs:582-590`, `659-676`, and `679-701`; PLC failure falls back to bounded silence at `705-722`.
- Existing FEC tests at `crates/relay-audio/tests/rx.rs:502-548` assert only public source/status/shape with a real decoder.
- Existing malformed test at `crates/relay-audio/tests/rx.rs:411-419` is rejected by stateless packet inspection at `crates/relay-audio/src/rx.rs:518-526`; it never reaches any decoder-error branch.
- No RX test asserts `FrameStatus::ConcealedCodecError` or `RxMetrics::codec_errors`.

**Impact**

The happy-path real-codec tests provide useful integration coverage but cannot prove that the following packet is decoded normally exactly once after the FEC request, that error fallback calls PLC in the intended order, or that a subsequent valid packet remains decodable after each contained error. A regression in any of those branches could pass all eight tests. The libopus source explicitly makes the FEC request and subsequent normal decode distinct operations; testing only `FrameSource::InbandFecOrPlc` does not establish their exact sequence.

**Exact correction**

Introduce a private decoder seam usable by RX unit tests (without widening the public API), then use a scripted decoder to assert call traces and injected results for:

- `fec(N) -> decode(N)` exactly once and in that order;
- FEC error -> PLC for the missing frame -> normal decode of the following packet;
- normal decode error -> PLC -> successful decode of the next packet;
- PLC error -> exact-length zero output and successful subsequent decode;
- the `Ready` path emits staged PCM without a second normal decode.

Retain the real-libopus black-box tests as integration tests. Add at least one parser-valid/decoder-invalid fixture if libopus supplies a stable one; otherwise the injected seam is the deterministic way to cover this contract.

**Disposition:** implementation ordering appears correct; test evidence is insufficient and should be strengthened.

### Medium — F3: malformed-input and metric taxonomy is not fully honest or operationally reconcilable

**References**

- `RxMetrics::metadata_mismatches` is documented as epoch metadata, length, or duration at `crates/relay-audio/src/rx.rs:297-298`.
- `record_rejection` folds `MalformedPacket`, `PacketTooLarge`, `SequenceBeforeEpoch`, and `SequenceOverflow` into that counter through the wildcard arm at `crates/relay-audio/src/rx.rs:733-744`.
- `fec_attempts` counts requests, correctly not proven LBRR recoveries (`crates/relay-audio/src/rx.rs:303-304`, `574`), while a failed FEC request can also increment both `codec_errors` and `plc_frames` (`584-589`, `645-649`).
- The evidence says “A malformed current packet is counted and concealed” at `docs/research/relay-audio-rx-core-implementation.md:31-32`, but a statelessly malformed ingress packet is returned immediately and only later manifests as a missing deadline; it is not decoded or immediately concealed.

**Impact**

The public counter documentation does not describe what the implementation counts. Operators cannot distinguish malformed encoded data from identity/timeline mismatches, before-epoch input, or exhaustion overflow. Nor is there a documented reconciliation equation for emitted frames because FEC attempts are request counts and overlap PLC on failed requests.

**Exact correction**

Either:

- split validation counters into at least malformed packet, identity/timestamp/duration mismatch, packet too large, and timeline extension failure, and add an explicit `emitted_frames` counter; or
- keep the compact structure but rename/document `metadata_mismatches` as all non-reorder validation rejections and document that `fec_attempts`, `plc_frames`, and `codec_errors` are intentionally non-exclusive operation counters.

Also revise the evidence to distinguish (a) stateless malformed ingress rejection followed by ordinary deadline concealment from (b) a parser-valid packet whose stateful decode fails and is immediately concealed.

**Disposition:** metrics are deterministic and saturating, but their public taxonomy/documentation needs correction.

### Low — F4: RX maintains two playout-head representations but never checks their invariant

**References**

- RX initializes/resets the reorder wire head and `next_extended` together at `crates/relay-audio/src/rx.rs:363-380` and `448-460`.
- Packet validation extends relative to `next_extended` at `495-506`.
- `pop_decision` ignores the sequence returned by `ReorderBuffer::pop_at_deadline()` at `535-544` and advances `next_extended` separately at `546-552`.

**Impact**

The representations are coherent today because they are advanced on the same path, including wrap. Ignoring the reorder result makes that an implicit invariant, however; a future change could silently attach the wrong extended sequence/timestamp to a popped packet.

**Exact correction**

At minimum, assert in debug/test builds that every returned reorder sequence equals `next_extended.wire()`, including `MissingAtDeadline`. Prefer deriving the emitted wire identity from the reorder decision and checking it against the extended head. Treat `Playout::Empty` after explicit construction/reset rebase as an invariant violation rather than silently manufacturing a missing decision, unless the empty state is intentionally supported and tested.

**Disposition:** no present divergence found; harden the invariant.

### Low — F5: the inline-storage address promise and its test are stronger than Rust guarantees

**References**

- `PcmFrame` says its address is stable for the life of the containing worker at `crates/relay-audio/src/rx.rs:169-176`.
- `RxWorker` is not pinned or heap-stabilized (`crates/relay-audio/src/rx.rs:343-355`), so moving the worker moves its inline fields.
- The pointer test at `crates/relay-audio/tests/rx.rs:190-220` proves reuse while that local worker remains unmoved, not lifetime-long address stability.
- `PendingDecision` has both `#[allow]` and a reasoned `#[expect]` for the same large-enum lint at `crates/relay-audio/src/rx.rs:311-318`.

**Impact**

There is no memory-safety bug: the borrow prevents mutation/move while a `FrameOutcome` is live. The public documentation nonetheless promises more than the type guarantees. The duplicated lint controls also weaken the claimed “scoped expectation” evidence.

**Exact correction**

Remove the address-stability promise or qualify it as storage reused between calls while the worker is not moved. If a stable address is genuinely required, pin/box the storage and expose that contract explicitly. Keep only the reasoned `#[expect(clippy::large_enum_variant)]`; the inline packet justification is otherwise sound because it avoids per-tick boxing/allocation.

**Disposition:** documentation/lint cleanup; fixed-capacity lifetime design otherwise passes review.

## Checklist disposition

### Correctness

- [x] **Timestamp/sequence extension vs reorder head:** coherent in current paths, including `u16` and `u32` wrap; boundary proof is incomplete (F1) and the duplicate-head invariant should be checked (F4). RFC 3550 §5.1 supplies the 16-bit sequence and 32-bit timestamp wire semantics; the fixed-duration exact timestamp mapping is an explicit narrower stream contract.
- [x] **Validation before mutation:** packet length, identity, extension, timestamp, and stateless duration parsing occur before reorder or decoder mutation (`rx.rs:384-417`, `476-527`). Metrics intentionally record the attempt/rejection before return; rejected-input tests prove the epoch/reorder head did not advance but do not snapshot codec state or all metrics.
- [x] **One-frame state machine:** implementation order is correct by inspection (`rx.rs:420-440`, `557-643`), including FEC then normal decode of the same current packet. Direct call-order/error tests are missing (F2).
- [x] **FEC/PLC taxonomy:** `InbandFecOrPlc` honestly avoids asserting LBRR presence, matching the libopus decoder contract that a FEC request may use PLC. Deadline gaps are not mislabeled as confirmed network loss. Metric reconciliation needs documentation (F3).
- [x] **Malformed fallback/codec state:** stateless malformed packets are rejected before decoder mutation; stateful decode errors have bounded PLC/silence fallback. None of the stateful error paths or subsequent decoder usability is tested (F2/F3).
- [x] **Drain/reset/exhaustion:** drain resolves exactly the pending decision without advancing the reorder deadline; reset constructs the replacement decoder before infallible state replacement/clearing; final `u64::MAX` decision is emitted then ticks stop. Success paths are covered, but fallible reset preservation and exhaustion are not directly tested (F1/F2).
- [x] **Fixed storage/lifetimes/large enum:** two fixed PCM buffers and inline pending ownership avoid visible hot-path growth. The large variant has a valid allocation-free rationale; address documentation and duplicate lint attributes need cleanup (F5).
- [x] **Metrics:** saturating increments are consistently used. Taxonomy and test completeness are inadequate (F1/F3).
- [x] **Rubato boundary:** Rubato is not used by this fixed-48-kHz RX core; adaptive SRC, its delay, and caller-buffer processing remain outside the audited module exactly as the evidence states.

### Test strength

- [x] Existing tests have direct assertions for the three durations, first-tick lookahead, packet drain, reordered/duplicate/late ownership, simple forward sequence/timestamp wrap, consecutive PLC, FEC-request source naming with FEC enabled/disabled, successful reset, finite PCM, and a subset of metrics.
- [ ] No direct tests cover capacity edges, ambiguous half-range, before-epoch, sequence overflow, timeline exhaustion, packet-too-large, stateful decoder failures, PLC failure, failure-atomic reset, saturation, or all metrics.
- [ ] No scripted/mock decoder proves exact call count/order or post-error codec usability.
- [ ] No allocator instrumentation proves the no-allocation tick claim; source inspection supports it, but the current pointer-equality test is not an allocation test.

## Validation results

Run from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` against the audited tree:

```text
cargo test -p relay-audio --test rx --locked
PASS — 8 passed, 0 failed (debug)

cargo test --release -p relay-audio --test rx --locked
PASS — 8 passed, 0 failed (release)

cargo clippy -p relay-audio --test rx --locked -- -D warnings
PASS — no warnings
```

These are deliberately narrow RX commands. Passing results establish the behavior asserted by the existing eight tests; they do not cure the coverage gaps above.

## Source basis (no additional sources used)

1. RFC 3550 — RTP sequence/timestamp and jitter semantics: <https://www.rfc-editor.org/rfc/rfc3550>
2. Xiph.Org libopus 1.6 decoder API — normal decode, PLC, and in-band FEC request contract: <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Rubato 4.0 `Resampler` API — confirmed out of scope for this fixed-rate core: <https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html>
