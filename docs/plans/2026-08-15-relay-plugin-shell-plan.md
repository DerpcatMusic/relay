# RELAY Plugin-Shell Plan

**Date:** 2026-08-15  
**Status:** Validated planning proposal; execution is blocked until the standalone Connect milestone exits  
**Scope:** A thin DAW plugin shell around the already-working RELAY engine; no plugin implementation is part of this planning change  
**Parent plan:** [RELAY master architecture and implementation plan](2026-08-15-relay-master-plan.md)  
**Research proof:** [Plugin-shell plan validation](../research/plugin-shell-plan-validation.md)

## 1. Sequencing rule

Plugin-shell execution MUST NOT begin until standalone Connect has passed the Phase 3 gate in the master plan: two standalone processes exchange stereo audio by direct P2P on Windows↔Linux, Windows↔macOS, and macOS↔Linux, with linked reproducible evidence. This planning and source-validation record may exist earlier, but even the disposable Truce spike is a Phase 5 execution task and therefore waits for that recorded gate. Browser Link remains Phase 4 in the master sequence; this plan does not reorder it.

The shell is an adapter, not a second implementation of RELAY. Networking, session orchestration, buffering, DSP, diagnostics, and reconnect policy remain owned by the portable engine proven by standalone Connect. Authentication and host-state persistence are intentionally not added to the Phase 3 prerequisite: they receive their own plugin-side boundary gates below rather than silently expanding standalone Connect's published exit criteria.

## 2. Desired boundary

```text
DAW input -> allocation-free local dry path -> DAW output (0 reported samples)
                 |
                 +-> bounded tap/commands
                     -> Truce plugin shell (format/lifecycle/parameter/state adapter)
                        -> ProcessorEngineBridge
                           -> existing RELAY engine/session core

UI/editor -> non-RT application/control service
          -> auth broker and OS credential store (outside host state)
          -> same engine/session core
```

The plugin artifact owns only host contracts: format entry points, bus/layout negotiation, activation/deactivation, process callbacks, editor parenting, automation-facing parameters, host state serialization, and translation into the engine's stable interface.

## 3. Gate P0 — standalone Connect prerequisite

Evidence required before opening the implementation epic:

- A tagged or otherwise immutable CI build of standalone Connect and a linked acceptance report.
- Direct-P2P stereo exchange evidence for Windows↔Linux, Windows↔macOS, and macOS↔Linux, exactly matching the Phase 3 master-plan gate.
- Engine API identified as the single implementation reused by standalone and plugin products.
- No standalone UI, device-I/O, or lifecycle type leaks into the engine API that the plugin would have to emulate.
- Known engine defects are triaged; no open blocker is relabeled as plugin work.

Sign-in, reconnect hardening, project state, and plugin packaging are useful later proofs but are not invented here as extra Phase 3 exit conditions.

**Exit gate:** architecture owner and product owner record “Standalone Connect accepted for plugin-shell start,” with evidence links, in the implementation issue.

## 4. Gate P1 — Truce acceptance spike

Create a disposable, non-shipping spike in an isolated workspace/package. It may start only after P0. Its purpose is to prove that the currently selected Truce revision can serve as a thin shell without forcing engine policy into the framework.

### Spike matrix

Build the smallest no-op/passthrough processor supported by Truce and prove:

1. Exact plugin formats, OSes, architectures, Rust/toolchain requirements, and license are compatible with RELAY's release matrix.
2. Audio callback, initialization, activation, reset, sample-rate/block-size changes, bus/layout negotiation, latency notification, state save/load, editor creation/destruction, and host log/error paths are reachable through documented APIs.
3. The callback can borrow host buffers and MIDI/event data without allocation or ownership transfer.
4. A prebuilt engine handle can be installed/removed without constructing, blocking, or dropping heavyweight resources on the audio thread.
5. UI and background workers can send bounded commands and receive immutable/bounded snapshots without locks in the callback.
6. State serialization can remain versioned RELAY data rather than framework-derived runtime state.
7. One artifact per intended format can be produced, inspected, scanned, and loaded in at least one representative host.
8. Panic containment and error reporting behavior at the FFI/host boundary are understood and testable.

Record every required unsafe boundary, framework patch, fork, generated file, and undocumented assumption. Pin the exact accepted revision; do not accept a floating Git reference.

**Exit gate:** an ADR/evidence report says **accept**, **accept with owned patch/fork**, or **reject**. Acceptance requires all required formats and lifecycle/RT hooks to be proven. Rejection returns to framework selection and does not authorize a custom shell by default.

## 5. Gate P2 — Processor/engine bridge

Define a narrow `ProcessorEngineBridge` owned by RELAY, independent of plugin formats and Truce concrete types.

### Control side

- `prepare(sample_rate, max_block, input_layout, output_layout)` builds or selects all processing resources off the audio thread.
- `activate()` publishes a prepared generation atomically.
- `deactivate()` prevents new work, drains/joins background work off-thread, and retires resources only after audio-thread acknowledgement.
- Commands (connect/disconnect, routing, monitor mode, parameter changes) enter bounded queues with explicit full-queue behavior.
- Status arrives as rate-limited immutable snapshots/counters; the editor never queries mutable processor state directly.

### Process side

- `process(block_context, audio, events)` has a borrowed, allocation-free interface.
- It consumes at most a bounded amount of queued control work per block.
- It never starts networking, performs authentication, serializes state, logs synchronously, or calls host APIs not documented as process-safe.
- It defines silence/bypass/failure output for absent, preparing, reconnecting, incompatible, and failed engine states.
- Sample position/transport metadata is optional input; network/session correctness must not depend on a particular host transport behavior.

Use one coherent immutable generation/handle publication mechanism. Do not publish related pointers and revisions separately. Reclamation must be acknowledged by the processing side before the control side destroys a formerly live generation.

**Exit gate:** bridge contract tests run against a fake engine, including queue saturation, generation swaps, zero/maximum blocks, changing layouts, reconnect, and teardown races; standalone and plugin adapters demonstrably depend on the same engine API.

## 6. Gate P3 — realtime contract

The following are forbidden in any host process callback or code reachable from it:

- heap allocation/deallocation or reference-count destruction with an unbounded finalizer;
- mutex/RwLock acquisition, condition variables, waiting, thread creation/join, filesystem/keychain access, DNS, sockets, or other syscalls with unbounded latency;
- synchronous logging/formatting, panic unwinding across FFI, exceptions, or host UI calls;
- unbounded loops, queue drains, resampling/filter design, coefficient generation, or other data-dependent work;
- destruction of an old engine generation or network/session object.

Required:

- fixed-capacity queues/buffers sized from a documented memory budget;
- bounded per-block work and deterministic overflow/underflow policies;
- resources and coefficient families precomputed off-thread;
- denormal/numeric handling and channel-count bounds;
- monotonic counters for dropped commands, underruns, overruns, stale snapshots, and RT contract violations, observed off-thread;
- automated allocation/lock/syscall instrumentation where the platform permits, plus stress runs at the minimum supported block size.

**Exit gate:** RT audit checklist signed off; instrumented tests show no forbidden operation in steady-state, transition, bypass, state-restore, and teardown stress scenarios.

## 7. Gate P4 — host state and authentication separation

Host/project state is portable, deterministic configuration only. It may contain schema version, routing/session references safe to share, parameter values, and reconnect intent. It MUST NOT contain access tokens, refresh tokens, passwords, private keys, keychain material, machine-local cache paths, raw diagnostic captures, or personally identifying session history.

Authentication belongs to a process-external or non-RT auth broker plus OS credential storage. The plugin stores at most a stable account/workspace hint and an opaque, non-secret reference. Loading a project on another machine must produce a clear signed-out/unavailable state, not an authentication failure loop.

State rules:

- versioned envelope with size limits, validation, migrations, and forward-compatibility behavior;
- serialize from a control-thread snapshot, never live audio-thread state;
- restore is two-phase: parse/validate off-thread, then publish a prepared generation/command;
- restore, reset, and async commit use revision checks repeated after every blocking serialization point so stale work cannot overwrite newer state;
- corrupt/oversized/unknown state fails safely and leaves processing defined;
- state bytes and logs are scrubbed in tests for credential-like values.

**Exit gate:** round-trip/golden/migration/fuzz tests pass; a security review proves secrets never enter host chunks, presets, crash reports, or plugin logs.

## 8. Gate P5 — lifecycle model

Specify and test an explicit state machine, rather than inferring lifecycle from optional callbacks:

```text
Created -> Prepared -> Active <-> Suspended -> Deactivating -> Destroyed
                 \-> Failed (recoverable only through a defined control-side transition)
```

Rules:

- Construction and format discovery do no network/auth work and are fast enough for bulk host scans.
- Prepare may be repeated with different sample rates, maximum blocks, and layouts.
- Activation publishes only already-prepared resources.
- Processing before readiness and after suspension has deterministic silence/bypass behavior.
- Editor lifetime is independent: zero, one, or repeated editor instances must not own processor/session lifetime.
- Hosts may call save/load, editor, activate/deactivate, and destroy in surprising orders or concurrently where their contract allows; all entry points are idempotent or explicitly guarded.
- Deactivation first stops publication/use, receives processing acknowledgement, then retires resources off-thread.
- Destruction is bounded from the host's perspective; remote disconnect is best effort and never blocks host shutdown.
- Crashes/panics do not cross ABI boundaries; failed instances become inert and report asynchronously.

**Exit gate:** model-based lifecycle tests and adversarial host sequences pass under sanitizers/race tooling where available, including scan-only instantiation and editor churn.

## 9. Gate P6 — formats and host validation

The accepted Truce spike must establish the authoritative format list; the implementation issue then freezes a V1 matrix. Do not claim a format merely because a framework enum or build target exists.

For each supported format/OS/architecture combination:

1. Build a clean, reproducible release artifact from the pinned toolchain.
2. Inspect architecture, exported entry points, bundle metadata, IDs, versions, and linked runtime dependencies.
3. Run the format vendor's current validator/scanner where one exists.
4. Test cold scan, rescan, instantiate, bus/layout changes, sample-rate and buffer changes, play/stop, bypass, automation, state save/reload, duplicate instance, editor reopen, offline/bounce behavior, suspend/resume, device change where applicable, and project close/host quit.
5. Stress multiple simultaneous instances and distinguish intentionally shared services from instance state.
6. Validate at least one representative host per supported host family plus all hosts promised by product requirements. Record exact host/build and known deviations.

Host test evidence is machine-readable where possible and attached to the release candidate. Validator success alone is not host acceptance.

**Exit gate:** every promised cell is green or has an explicitly approved limitation in release notes; no “untested but expected” cells ship.

## 10. Gate P7 — packaging, identity, and signing boundaries

Separate three trust boundaries:

1. **Build provenance:** locked dependencies/toolchain, CI identity, SBOM/license inventory, checksums, and immutable artifacts.
2. **Plugin binary/bundle signing:** platform-native signing of every executable/nested code object after assembly and before installer packaging; no mutation afterward.
3. **Installer/distribution trust:** installer/package signing and, where required, notarization/stapling or platform submission. Installer trust does not substitute for plugin-bundle signing.

Secrets for signing/notarization live only in protected release infrastructure and are unavailable to PR/fork builds. Unsigned ad-hoc artifacts are clearly labeled and never promoted. Identifiers (manufacturer/vendor, plugin ID/class IDs, bundle IDs, state schema identity) are reserved once, tested for uniqueness, and never derived from build paths.

Verification must occur on a clean machine with no developer certificates or build tree present, and repeat after download/transport. The release manifest binds version, commit, dependency lock, artifacts, hashes, signatures, validator reports, and host matrix.

**Exit gate:** clean-machine install, platform trust verification, scan/load, uninstall, and artifact-hash verification all pass for each shipping platform.

## 11. Executable task breakdown

Tasks are ordered; a later phase cannot start until the previous exit gate is recorded.

### P0: accept standalone Connect

- [ ] Link the immutable standalone build and acceptance matrix.
- [ ] Verify engine portability and close/waive blockers.
- [ ] Record the P0 approval.

### P1: decide Truce

- [ ] Pin a candidate revision and document license/toolchain/support surface.
- [ ] Build the isolated passthrough/no-op spike.
- [ ] Exercise every lifecycle, buffer/event, state, editor, error, and packaging hook needed by V1.
- [ ] Scan/load spike artifacts in representative hosts.
- [ ] Inventory unsafe/patch/fork obligations and write the decision ADR.

### P2: freeze contracts before integration

- [ ] Specify engine portability API and `ProcessorEngineBridge` types without framework types.
- [ ] Specify RT operations, bounds, failure policies, memory budget, and publication/reclamation protocol.
- [ ] Specify host-state schema/migrations and auth-broker boundary.
- [ ] Specify lifecycle state machine and shared-vs-instance ownership.
- [ ] Review contracts with audio, security, and release owners.

### P3: test adapters without plugin product code

- [ ] Build fake host/fake engine contract tests.
- [ ] Add saturation, race, lifecycle-model, state fuzz/golden, and RT instrumentation suites.
- [ ] Demonstrate the standalone adapter and proposed shell adapter target the same engine API.

### P4: implement shell (future implementation epic)

- [ ] Create format entry points and Truce adapter only after P1–P3 pass.
- [ ] Wire bridge, parameters/events, state, lifecycle, editor, diagnostics, and panic containment.
- [ ] Keep all auth UI/credential access on the control/application side.
- [ ] Produce unsigned CI artifacts for validation.

### P5: validate and release

- [ ] Run vendor validators and the frozen host matrix.
- [ ] Complete RT, security, lifecycle, and multi-instance audits.
- [ ] Assemble, sign, notarize where applicable, package, and sign installers in the defined order.
- [ ] Verify on clean machines and publish the evidence-bound manifest.

## 12. Overall release exit gate

The plugin shell may ship only when:

- standalone Connect remains accepted on the engine revision being shipped;
- Truce is accepted at an exact revision with owned maintenance risk documented;
- the shell contains no duplicate networking/session/DSP policy;
- bridge, RT, state/auth, lifecycle, format/host, and signing gates all have linked evidence;
- no secrets appear in host state or artifacts;
- every promised platform/format/host cell is tested; and
- rollback, crash triage, support ownership, and framework-upgrade policy are documented.

A failure in any gate blocks release rather than silently narrowing the evidence.
