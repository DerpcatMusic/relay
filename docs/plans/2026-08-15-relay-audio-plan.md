# RELAY Phase 1 Audio Engine / Audio Lab Plan

**Date:** 2026-08-15  
**Status:** Validated and in implementation; focused primitives complete, `relay-audio` composition and lab gates open  
**Scope:** Approved Phase 1 audio engine plus deterministic fake-network loss/jitter/drift proof; no production transport
**Composition contract:** [`../research/relay-audio-composition-design.md`](../research/relay-audio-composition-design.md)  
**Primitive review/fixes:** [`../research/review-audio-rt-clock-fixes.md`](../research/review-audio-rt-clock-fixes.md), [`../research/review-audio-jitter-fixes.md`](../research/review-audio-jitter-fixes.md), [`../research/review-audio-resample-fixes.md`](../research/review-audio-resample-fixes.md), [`../research/review-audio-opus-fixes.md`](../research/review-audio-opus-fixes.md)

## Outcome

Deliver a headless, testable audio-engine core plus a minimal audio-lab harness that can accept decoded mono/stereo PCM, cross the device/engine boundary without blocking the realtime callback, adapt sample rates outside the callback, and expose enough deterministic diagnostics to prove callback safety and continuity. This phase establishes seams; it does not attempt the complete RELAY media product.

## Dependency order and one-agent task packets

Each task is deliberately owned by one agent and should land as one reviewable commit. An agent edits only the files listed in its packet; changes to another packet require an explicit hand-off.

### A1 — Freeze audio contracts and test fixtures

**Depends on:** none  
**Owner:** Audio contracts agent  
**File ownership:**
- `crates/relay-audio/src/lib.rs`
- `crates/relay-audio/src/config.rs`
- `crates/relay-audio/src/frame.rs`
- `crates/relay-audio/tests/config_contract.rs`
- `crates/relay-audio/tests/frame_contract.rs`

**Work:** Define validated stream configuration (sample rate, channel count, bounded block/ring capacities), interleaved PCM frame/block types, explicit channel/sample units, and error types. Keep device, codec, transport, and UI types out of the core contract.

**Tests:** reject zero/unsupported dimensions and capacity overflow; prove frame/channel accounting; compile-time `Send` checks for hand-off values; property-style boundary cases for duration/frame conversions.

**Acceptance gate:** public contracts have documented units and invariants; malformed configurations fail before any audio thread starts; core types do not depend on a device, Opus, or UI crate.

### A2 — Build the bounded realtime ingress/egress seam

**Depends on:** A1  
**Owner:** Realtime queue agent  
**File ownership:**
- `crates/relay-audio/src/queue.rs`
- `crates/relay-audio/src/metrics.rs`
- `crates/relay-audio/tests/queue_contract.rs`

**Work:** Wrap a preallocated bounded SPSC queue with role-specific producer/consumer handles. Define nonblocking full/empty policy, counters, startup prefill, and shutdown/disconnect behavior. No queue allocation, locks, waits, logging, formatting, or destruction of heap-owning payloads in the callback.

**Tests:** FIFO and wraparound; full/empty/disconnect; producer/consumer stress; counters; bounded callback-side operations; optional allocation-counting test around steady-state pop/push.

**Acceptance gate:** callback-facing methods are nonblocking and allocation-free in steady state; overflow/underflow policy is deterministic and observable; queue endpoints have exactly one producer and one consumer.

### A3 — Add off-thread sample-rate adaptation

**Depends on:** A1, A2  
**Owner:** Resampling agent  
**File ownership:**
- `crates/relay-audio/src/resample.rs`
- `crates/relay-audio/tests/resample_contract.rs`
- `crates/relay-audio/tests/fixtures/resample/README.md`

**Work:** Adapt decoded stream-rate PCM to the device/engine rate on a worker thread using Rubato's asynchronous resampler. Reuse preallocated input/output buffers, preserve channels independently, define startup latency and flush/end-of-stream behavior, and feed fixed-capacity blocks to the SPSC seam.

**Tests:** 48→48 kHz bypass; 44.1→48 and 48→44.1 kHz duration/frame-count tolerance; stereo channel isolation; finite outputs; reset/flush; chunk-boundary continuity; no steady-state worker buffer growth.

**Acceptance gate:** resampling never occurs in the device callback; output duration and channel mapping meet documented tolerance; latency and required chunk sizes are queryable and covered by tests.

### A4 — Add an off-thread Opus decode adapter

**Depends on:** A1, A2  
**Owner:** Codec adapter agent  
**File ownership:**
- `crates/relay-audio/src/opus.rs`
- `crates/relay-audio/tests/opus_decode.rs`
- `crates/relay-audio/tests/fixtures/opus/README.md`

**Work:** Wrap libopus decoding behind a narrow adapter that turns complete encoded packets into PCM on a decode worker. Preallocate for the documented maximum frame size, distinguish packet loss from malformed packets, and expose decoded sample count/channels without leaking FFI types into the engine.

**Tests:** known packet fixture; mono/stereo accounting; malformed packet error; loss-concealment call; maximum supported decoded frame; decoder reset.

**Acceptance gate:** no Opus call occurs on the device callback; all output-buffer bounds derive from documented Opus limits; malformed input cannot overrun or poison subsequent decode.

### A5 — Compose the worker pipeline and lifecycle

**Depends on:** A2, A3, A4  
**Owner:** Pipeline agent  
**File ownership:**
- `crates/relay-audio/src/pipeline.rs`
- `crates/relay-audio/src/runtime.rs`
- `crates/relay-audio/tests/pipeline_integration.rs`

**Work:** Compose packet input → decode worker → optional resampler → bounded PCM queue. Make start, prefill/ready, stop, drain, reset, and worker failure explicit. Publish metrics via atomics or off-thread snapshots only.

**Tests:** deterministic fake packet source; ordered lifecycle transitions; prefill gate; worker failure propagation; stop while starved/full; repeated start/stop; bounded queue depth.

**Acceptance gate:** no unbounded channel exists; shutdown cannot require the callback to wait; failure and starvation produce deterministic silence/status rather than blocking; lifecycle is repeatable.

### A6 — Implement the callback-side renderer

**Depends on:** A2, A5  
**Owner:** Render agent  
**File ownership:**
- `crates/relay-audio/src/render.rs`
- `crates/relay-audio/tests/render_contract.rs`

**Work:** Render queued PCM into caller-provided device buffers. Handle arbitrary callback buffer lengths with a preallocated cursor/staging block. On underflow, zero-fill the remainder and increment an atomic counter. Apply only constant-time, allocation-free channel copy/mapping agreed in A1.

**Tests:** exact/partial/multiple-block callbacks; starvation zero-fill; resume after starvation; channel mapping; finite-output guard; allocation counter around repeated render; callback-time budget smoke benchmark (non-gating in CI unless a stable runner exists).

**Acceptance gate:** render performs no allocation, lock, wait, I/O, log, panic path, codec call, or resampler call; every output sample is initialized; callback work is bounded by requested frames/channels.

### A7 — Build the minimal audio-lab harness

**Depends on:** A5, A6  
**Owner:** Audio-lab agent  
**File ownership:**
- `apps/audio-lab/Cargo.toml`
- `apps/audio-lab/src/main.rs`
- `apps/audio-lab/src/diagnostics.rs`
- `apps/audio-lab/tests/headless_smoke.rs`
- `docs/audio-lab.md`

**Work:** Provide a headless-first lab command that selects a fixture/synthetic source, requested device/sample-rate configuration, duration, and optional real device output. Print an off-thread final diagnostics summary: rendered frames, underflows, overflows, queue high-water mark, worker errors, and effective rates.

**Tests:** headless synthetic run; fixture run when fixture licensing/provenance is recorded; invalid configuration; clean bounded shutdown. Manual smoke: supported device opens, plays, and exits without hang.

**Acceptance gate:** CI needs no audio device; a developer can run one documented command for deterministic headless proof and one opt-in device smoke; the callback never prints diagnostics.

### A8 — Phase gate and evidence record

**Depends on:** A1–A7  
**Owner:** Verification agent  
**File ownership:**
- `docs/research/audio-phase-1-evidence.md`
- `docs/plans/2026-08-15-relay-audio-plan.md` (status/evidence links only)

**Work:** Run formatting, lint, unit/integration tests, headless lab, allocation-safety proof, and manual device smoke where available. Record commands, platform, results, known limitations, and deferrals.

**Acceptance gate:** all automated gates pass; callback safety checklist has direct code/test evidence; no Phase 2 feature was pulled into the core; exceptions are explicit and approved.

## Realtime thread model

| Context | Owns | May do | Must not do |
|---|---|---|---|
| Device callback (hard realtime) | Device output slice; consumer cursor; SPSC consumer; primitive counters | Bounded pop/copy/zero-fill; relaxed atomic increments; finite numeric checks | Allocate/free; lock; wait/sleep; perform file/network I/O; decode; resample; log/format; call UI; panic/unwind |
| Decode/resample worker (soft realtime) | Opus decoder; Rubato state; reusable PCM work buffers; SPSC producer | Decode complete packets; resample; bounded queue push; update metrics; discard according to policy | Touch device API; block callback; use unbounded queues; grow buffers in steady state |
| Control/main thread | Configuration; worker/device lifecycle; diagnostics snapshots | Validate; allocate/prewarm; open/close device; start/stop workers; report results | Mutate callback-owned state directly; assume callback has stopped without an acknowledgement |
| Transport/source thread (external seam) | Complete encoded packets and timing metadata | Deliver bounded packet values; report loss/discontinuity | Call codec/device callback directly; leak transport types into audio core |

**Publication/lifetime rule:** all callback-visible buffers and configuration are allocated and initialized before stream start. Ownership crosses only through bounded SPSC handles or immutable values. Stop first prevents new production, the callback is detached/stopped through the device API, workers are joined off the callback, and only then are callback-visible allocations dropped.

**Underflow policy:** initialize the full device buffer, consume available PCM, zero-fill missing frames, increment a primitive atomic counter, and continue.  
**Overflow policy:** producer never waits for the callback; apply one documented drop/reject policy and count it. The exact choice must be fixed by A2 tests.  
**Panic policy:** callback-facing code is total for validated configuration; failures become silence plus counters/status, never unwinding across an audio/FFI boundary.

## Cross-phase acceptance gates

1. **Architecture:** codec, resampling, transport, device, and UI remain behind separate seams; the engine can be tested with caller-provided buffers.
2. **Realtime safety:** callback transitive call graph contains no allocation/deallocation, mutex/RwLock, blocking channel, I/O, formatting/logging, decoder, resampler, or panic-based control flow.
3. **Boundedness:** every queue and work buffer has a validated maximum; steady-state memory is constant.
4. **Correctness:** frame/channel units are explicit; all output samples are initialized and finite; resampling duration/channel tolerances pass.
5. **Resilience:** starvation, overflow, malformed packet, worker failure, shutdown, and restart are deterministic and tested.
6. **Portability:** automated tests and the default lab smoke require no physical audio device; platform/device proof is separately recorded.
7. **Evidence:** each gate links to a test, inspection, or recorded command rather than an assertion alone.

## Non-goals

- Production network transport, encryption, signaling, route selection, or a general RTP stack. A bounded deterministic fake network and the existing jitter/clock primitives are required by the master Phase 1 gate.
- Adaptive bitrate/product codec negotiation or protocol versioning. Fixed-profile Opus encoding, packetization, FEC/PLC, and decode are required for the Phase 1 loopback.
- Mixing multiple peers, spatial audio, effects, echo cancellation, noise suppression, or automatic gain control.
- Production device discovery/hot-plug UX, cross-platform device backend bakeoff, or mobile support.
- GUI/waveform editor, persistence, telemetry service, or release packaging.
- Claiming glitch-free production performance from CI timing alone.
- General-purpose graph/plugin architecture or premature zero-copy abstractions across every boundary.

## Commit slices

1. `audio: define validated stream and PCM contracts` (A1)
2. `audio: add bounded realtime SPSC seam and counters` (A2)
3. `audio: add off-thread asynchronous sample-rate adapter` (A3)
4. `audio: add bounded off-thread Opus decoder adapter` (A4)
5. `audio: compose explicit worker pipeline lifecycle` (A5)
6. `audio: add allocation-free callback renderer` (A6)
7. `audio-lab: add deterministic headless harness and diagnostics` (A7)
8. `docs: record Phase 1 audio acceptance evidence` (A8)

Each commit must compile and pass its owned tests; later commits may add integration coverage but must not silently rewrite earlier contracts. If validation changes a foundational contract, amend/reorder the affected slice before implementation rather than hiding the correction in a later commit.

## Validation still required

Before this plan is approved, reconcile names/paths and Phase 1 scope with the repository master plan, then validate queue semantics against rtrb, codec bounds against libopus, asynchronous resampler constraints against Rubato, and callback prohibitions against a primary realtime-audio source. Record findings and exact corrections in `docs/research/audio-plan-validation.md`.
