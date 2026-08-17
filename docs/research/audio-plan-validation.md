# Audio Plan Validation

**Reviewed plan:** `docs/plans/2026-08-15-relay-audio-plan.md`  
**Validation date:** 2026-08-15  
**Disposition:** **Conditionally valid as a Phase 1 foundation slice; not valid as the complete master-plan Phase 1. Revise the plan before implementation.**

## Scope

This record validates `docs/plans/2026-08-15-relay-audio-plan.md` against a deliberately narrow evidence set: `rtrb` 0.3.4 documentation, official libopus 1.6 decoder documentation, Rubato 4.0.0 asynchronous-resampling documentation, and the repository-selected audio real-time principles. It also checks the reviewed plan's names, paths, and claimed Phase 1 boundary against `docs/plans/2026-08-15-relay-master-plan.md`.

This is a planning/evidence review only. No master plan, focused plan, or code was edited.

## Validation criteria

1. **Realtime safety:** callback work is nonblocking, allocation/deallocation-free, syscall/I/O/log-free, and bounded by the requested frame/channel count.
2. **Queue contract:** the proposed queue matches `rtrb`'s SPSC, fixed-capacity, full/empty, endpoint-lifetime, and abandonment semantics.
3. **Codec bounds:** decoder rates, channels, maximum frame storage, PLC inputs, ordering, and state recovery are explicit and testable.
4. **Resampler contract:** the selected Rubato 4 API avoids steady-state allocations, accounts for variable async chunk sizes and delay, and defines end-of-stream handling.
5. **Repository alignment:** crate ownership and the stated phase exit agree with the master plan, or the smaller slice is explicitly named and bounded.
6. **Evidence quality:** plan gates can be proven by tests, inspection, or recorded commands rather than timing assertions alone.

## Source table

Only the following four primary-source families were consulted.

| # | Primary source | Material consulted | Facts used |
|---|---|---|---|
| 1 | `rtrb` 0.3.4 rustdoc | [crate](https://docs.rs/rtrb/0.3.4/rtrb/), [`Producer`](https://docs.rs/rtrb/0.3.4/rtrb/struct.Producer.html), [`Consumer`](https://docs.rs/rtrb/0.3.4/rtrb/struct.Consumer.html), [`PushError`](https://docs.rs/rtrb/0.3.4/rtrb/enum.PushError.html), [`PopError`](https://docs.rs/rtrb/0.3.4/rtrb/enum.PopError.html) | One producer/one consumer; capacity allocated at construction; queue operations are lock-free, wait-free, and immediate; full returns `PushError::Full(T)` without overwrite; empty returns `PopError::Empty`; disconnect is separately observed with `is_abandoned()` and is not a synchronizing operation. |
| 2 | Official libopus 1.6 decoder API | [Opus Decoder](https://www.opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html) | Decoder `Fs` is 8/12/16/24/48 kHz and channels are 1 or 2; output `frame_size` is samples **per channel**; maximum packet duration is 120 ms (5760 samples/channel at 48 kHz); PLC uses null data/zero length and requires the exact missing duration, in a multiple of 2.5 ms; Opus decoding is stateful and packets must be serial and ordered. |
| 3 | Rubato 4.0.0 rustdoc | [crate guidance](https://docs.rs/rubato/4.0.0/rubato/), [`Async`](https://docs.rs/rubato/4.0.0/rubato/struct.Async.html), [`Resampler`](https://docs.rs/rubato/4.0.0/rubato/trait.Resampler.html), [`Adjustable`](https://docs.rs/rubato/4.0.0/rubato/trait.Adjustable.html) | `process_into_buffer()` is the preallocated real-time path; async resampling cannot fix both input and output sizes; callers must query next/max frame requirements; `output_delay()` reports startup delay; `Indexing::partial_len` zero-pads a short final input; `reset()` clears state; ratio changes are supported by `Adjustable`. |
| 4 | Audio engineering principles selected for this repository task | `/home/derpcat/.agents/skills/audio-engineering-principles/SKILL.md` (SHA-256 `736120d3a357310722f15b27156e55e18d134047208e6d1c0d4d9fcd1baf8e0f`) | On the audio thread: no allocation, locks, logging, file I/O, syscalls, exceptions, dynamic resizing, or blocking; preallocate and use fixed buffers/lock-free queues/atomics; require deterministic tests and numeric hygiene. |

The reviewed audio plan and repository master plan are the objects being reconciled, not additional external evidence sources.

## Findings

### 1. Realtime architecture is directionally correct

**Validated.** The plan keeps libopus and Rubato on a worker, gives the callback caller-owned output memory plus an SPSC consumer/cursor, zero-initializes missing output, publishes primitive counters, and moves construction/destruction to control-thread lifecycle. That agrees with the repository RT rules and Rubato's own advice to keep resampling out of the callback.

The acceptance language is stronger than a generic “lock-free” claim: it checks the callback's transitive call graph for allocation/deallocation, waits, I/O, formatting, codec calls, resampling, and panic-based control flow. Keep that gate.

**Gap:** add the remaining explicit RT prohibitions from the skill—no syscalls, no dynamic container resizing, and no exception/FFI unwind—and add denormal handling to numeric tests or document why the chosen renderer operations cannot create problematic denormals.

### 2. `rtrb` supports the SPSC seam, but payload and disconnect details need correction

**Validated:** A2 correctly requires exactly one producer and one consumer, construction-time capacity, immediate full/empty policy, and no spinning/waiting. `rtrb` does not overwrite a full queue; `push()` returns ownership in `PushError::Full(T)`. `pop()` reports only `PopError::Empty`.

**Required clarification:** `rtrb`'s “no more memory is allocated” guarantee is qualified by “unless `T` does that internally.” A callback that pops a heap-owning `Vec`/box/block and lets it drop would deallocate on the callback even though the ring itself does not allocate. A2/A6 must choose a trivially destructible fixed-size payload, copy samples through chunk/slice APIs, or recycle owned blocks to an off-thread reclamation path. The allocation test must cover both successful consumption and underflow/full-error paths.

**Required correction:** full/empty errors are not disconnect errors. Endpoint destruction is exposed through `is_abandoned()`, described by `rtrb` as a crude signal; since Rust 1.74 it is not synchronizing by itself. A producer may even still push after its consumer is gone. Therefore A2's “disconnect” tests must separately exercise `is_abandoned()`, and A5 lifecycle must retain a dedicated stop/acknowledgement protocol. It must not infer safe reclamation from `Empty` or abandonment alone. The plan's publication/lifetime paragraph already points toward the correct device-stop acknowledgement and off-callback endpoint destruction.

### 3. Opus buffer bounds are valid but underspecified

**Validated:** decoding on one worker, complete packet boundaries, preallocation, mono/stereo accounting, malformed-input tests, reset, and no FFI types in the engine are appropriate.

**Required correction:** state exact units and limits in A4:

- decoder output rate must be one of 8, 12, 16, 24, or 48 kHz;
- channels must be 1 or 2;
- output capacity is 5760 samples **per channel** at the repository's 48 kHz network clock, hence 11,520 scalar samples for maximum-duration stereo output;
- the returned decoded count is per channel.

**Required correction:** a generic “loss-concealment call” is not enough. PLC is invoked with null data and zero length, while `frame_size` must equal the known missing duration and be a multiple of 2.5 ms. The adapter's loss input therefore needs missing-duration/timestamp information, not just a boolean “packet missing.” FEC, if later enabled, has the same exact-duration constraint and should not be accidentally implied by this decode-only slice.

**Required clarification:** Opus is stateful and requires serial, ordered packets. The official API documents negative errors for corrupt packets or insufficient output, but does not establish the plan's desired post-error recovery policy. Replace “cannot ... poison subsequent decode” as a bare assertion with an explicit adapter policy and a test that decodes a valid packet after an error (reset/discontinuity behavior must be defined rather than assumed).

### 4. Rubato 4 supports the worker design, but the exact streaming mode must be selected

**Validated:** an asynchronous Rubato resampler on the worker is consistent with the master plan's future drift-correction architecture. Rubato 4 provides adjustable ratios and a nonallocating `process_into_buffer()` method. Queryable delay and buffer sizes support A3's proposed acceptance gates.

**Required correction:** name `process_into_buffer()` as the permitted steady-state API; the convenience `process()` allocates its output. Keep Rubato's `log` feature disabled in the time-sensitive worker path.

**Required design decision:** an asynchronous resampler cannot have both sides fixed. A3 must choose `FixedAsync::Input` or `FixedAsync::Output` and document the corresponding accumulator:

- fixed input means `input_frames_next()` is fixed and output length varies; or
- fixed output means output length is fixed and `input_frames_next()` varies.

In either case buffers must be preallocated to `input_frames_max()` and `output_frames_max()`, and every call must respect `input_frames_next()`/`output_frames_next()`. “Feed fixed-capacity blocks” is not by itself a complete chunking contract.

**Required correction:** replace ambiguous “flush/end-of-stream behavior” with a concrete drain rule. Rubato 4 exposes `output_delay()`, `reset()`, and `Indexing::partial_len` (which inserts silence for a short final chunk), but no generic `flush()` operation in this API. The plan must state how much zero-padding is supplied, how startup delay/tail samples are trimmed or counted, and when reset occurs. Test expected duration after that policy, not merely that a method named flush was called.

### 5. The plan is not the complete Phase 1 described by the master plan

The reviewed document calls itself a “small, executable Phase 1 slice” and explicitly excludes jitter buffering, clock synchronization, drift correction, Opus encoding, and a fake network. The master plan's Phase 1 instead builds `relay-rt`, `relay-opus`, `relay-resample`, `relay-clock`, `relay-jitter`, and `relay-audio`; tests `audio file → encode → fake network → decode → output`; injects jitter/loss/clock drift; and exits after 12 virtual hours without latency drift.

That is not a source-level audio error, but it is a blocking plan-label error. The focused document is a useful **Phase 1A / audio-foundation slice** and cannot close the master-plan Phase 1 gate. A later slice must own encoder, packet/fake-network path, jitter/clock/ASRC control, injected impairments, and the 12-hour virtual soak.

### 6. File ownership conflicts with the master architecture

The focused plan places queue, metrics, Opus, and resampling modules under `crates/relay-audio/`. The master plan defines separate durable crates (`relay-rt`, `relay-opus`, `relay-resample`, with `relay-audio` as composition/profile/tx/rx). One of these layouts must be authoritative before task packets are issued.

The safer correction is to preserve the master plan's dependency seams:

- A1 and composition/render contracts in `relay-audio`;
- A2/metrics in `relay-rt`;
- A3 in `relay-resample`;
- A4 in `relay-opus`;
- pipeline composition in `relay-audio` or a clearly named lab/runtime adapter.

If a monolithic incubation crate is intentional, the master plan must explicitly approve it and include a migration gate; otherwise the focused file-ownership list silently reverses an architectural decision.

### 7. Underflow/overflow policies need one repository-wide answer

The focused plan defers the exact overflow drop/reject choice to A2 and uses immediate zero-fill on receiver underflow. The master plan already says sender overflow is **drop new input**, while receiver underflow uses a short fade to zero and fade-in on recovery. Decide whether this foundation slice deliberately tests a simpler immediate-silence policy or implements the master fade state machine. Either is bounded, but contradictory specifications will produce contradictory tests.

## Explicit potential corrections to the master plan

These are proposed master-plan clarifications; none were applied in this task.

1. **Split Phase 1 into named gates.** Add Phase 1A “RT seam / decode / SRC / headless lab” with the reviewed plan's bounded scope, followed by Phase 1B “encode / fake network / jitter / clock recovery / drift soak.” Retain the 12-virtual-hour criterion only for the full Phase 1 exit.
2. **Declare crate ownership authoritative.** Keep `relay-rt`, `relay-opus`, and `relay-resample` as separate crates, or explicitly authorize temporary incubation inside `relay-audio` and specify the extraction gate.
3. **Pin evidence and dependency major together.** The stack table says “Rubato 4 Async”; require an exact compatible 4.x workspace version and validate against that version's API rather than `latest` docs (which are now 5.x).
4. **Make the 48 kHz decoder contract explicit.** The master already adopts a 48 kHz network clock; state that Opus decode capacity is 5760 samples/channel (11,520 stereo scalars) and resampling occurs between network and device clocks outside the callback.
5. **Specify PLC metadata.** State that a loss event delivered to the decoder includes the exact missing duration/timestamp progression required by libopus, even before the full jitter buffer exists.
6. **Reconcile underrun/overrun language.** Confirm “drop newest input” for sender overflow and either require bounded short fades on receiver starvation/recovery or explicitly permit immediate zero-fill in Phase 1A.
7. **Strengthen queue lifecycle language.** State that `rtrb::is_abandoned()` is diagnostic, not the lifecycle synchronization/reclamation barrier; device stop acknowledgement and off-callback destruction remain mandatory.

## Explicit corrections required in the reviewed audio plan

Before implementation task packets are issued:

1. Retitle/relabel it **Phase 1A audio foundation** and state that it cannot satisfy the master Phase 1 exit.
2. Reconcile every owned path with the master's separate `relay-rt`, `relay-opus`, and `relay-resample` crates.
3. Constrain queue payloads so successful pop, failed push, cursor replacement, and endpoint destruction cannot free heap memory on the callback.
4. Separate rtrb full/empty behavior from `is_abandoned()` and retain an explicit lifecycle acknowledgement.
5. Add Opus rate/channel/per-channel bounds and an exact-duration PLC input contract.
6. Define serial decoder ownership and deterministic post-error/reset/discontinuity behavior.
7. Select Rubato 4 `FixedAsync` mode, exact `process_into_buffer()` use, max/next buffer sizing, startup-delay accounting, and EOS drain/reset policy.
8. Reconcile immediate zero-fill/drop policy with the master plan's fade/drop-new language.
9. Add explicit no-syscall/no-resize/no-unwind wording and a denormal/numeric-hygiene check or rationale.

## Decisions reflected in the plan

The following decisions are supported by the evidence and should survive the corrections:

- device callback and decode/resample/control contexts are separate;
- codec and SRC never execute on the device callback;
- callback-visible state is constructed before start and reclaimed only after device-stop acknowledgement;
- the PCM crossing is bounded SPSC with one producer and one consumer per queue;
- callback operations return immediately and never spin for full/empty recovery;
- the callback initializes every output sample and emits deterministic silence on starvation;
- buffer sizes, channel/frame units, and latency are validated/queryable rather than implicit;
- worker buffers are reused with no steady-state growth;
- metrics are primitive atomics or off-thread snapshots, and diagnostics print only off-thread;
- CI proof is headless and deterministic; device timing is supplemental, not proof of RT safety;
- codec, resampler, transport, device, and UI types remain behind seams.

## Final disposition

**Revise, then approve as Phase 1A.** The core thread model and acceptance philosophy are sound and well supported by the four sources. The plan should not proceed unchanged because its phase label and file ownership conflict with the master plan, and its `rtrb`, Opus PLC, and Rubato streaming contracts omit implementation-critical details. These are plan corrections, not reasons to reject the architecture.

## Validation proof

- Evidence file was created before any source consultation and all placeholders were subsequently replaced.
- Exactly four primary-source families are recorded in the source table; no broad research or subagents were used.
- Source access and plan review occurred on 2026-08-15 UTC.
- Reviewed-plan SHA-256 before/after validation: `da5a8e526714ca8ec21eac77445af5fad440651415093c7e4ec04f0543a9ef3b` (unchanged).
- Master-plan SHA-256 before/after validation: `30a88481906e232c4d9f53cb0c43bd3b4292992293098eff0b3106a761338b61` (unchanged).
- No implementation commands were required: this task validates a plan and records evidence only.
- The only file written by this task is `docs/research/audio-plan-validation.md`.
