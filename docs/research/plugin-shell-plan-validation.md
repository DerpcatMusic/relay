# Plugin Shell Plan Validation

## Scope

Validate `docs/plans/2026-08-15-relay-plugin-shell-plan.md` against a narrow set of primary sources covering the official Truce plugin shell and host plugin validation/lifecycle expectations. This review does not edit the plan or implementation, does not select a shipping format matrix, and does not accept Truce ahead of the plan’s P1 spike.

## Validation Criteria

- The plan’s Truce build, packaging, and realtime-validation claims match official Truce material.
- The plan accounts for validation expectations of intended plugin formats and hosts.
- Lifecycle-sensitive requirements (initialization, activation, processing, reset, teardown, and thread constraints) are explicit enough to implement safely.
- Unsupported assumptions are identified as potential corrections rather than silently accepted.

## Source Table

| # | Primary source | Why consulted | Plan claims checked | Result |
|---|---|---|---|---|
| 1 | [Truce Install guide](https://truce.audio/docs/guide/install/) | Verify toolchain, formats, packaging prerequisites, and revision pinning | P1 items 1 and 7; P7 signing/package separation | Supports the acceptance spike and exact pinning; exposes platform/format prerequisites the spike must freeze |
| 2 | [Truce Real-time safety guide](https://truce.audio/docs/guide/rt-paranoid/) | Verify callback instrumentation and its limits | P1 items 3 and 5; P3 instrumentation and forbidden operations | Supports allocation checks, but requires broader RELAY-owned instrumentation and path coverage |
| 3 | [Official CLAP `clap_plugin` lifecycle contract](https://github.com/free-audio/clap/blob/main/include/clap/plugin.h) | Check callback ordering, threads, activation bounds, reset, and destruction | P1 item 2; P2 activation; P5 lifecycle model | Finds a lifecycle-model gap: CLAP separates main-thread activation from audio-thread start/stop/reset |
| 4 | [Official `clap-validator` README](https://github.com/free-audio/clap-validator/blob/master/README.md) | Check validator behavior, configuration, isolation, and fuzz evidence | P1 item 7; P5 adversarial sequences; P6 validator evidence | Supports automated scan/fuzz gates, but validator provenance/configuration must be captured and success is not host acceptance |

These four targeted sources are the complete source set for this validation.

## Findings

### Overall assessment

**Conditionally validated as a conservative execution-gating plan.** The plan is right to keep Truce provisional, require a disposable spike, isolate the engine from framework types, separate realtime/state/auth/release boundaries, and demand both validator and real-host evidence. The sources do not justify accepting Truce now; the plan correctly assigns that decision to P1.

Before implementation, the lifecycle model should represent the format-neutral equivalent of CLAP’s separate activation and processing phases. The bridge contract should also say how a format’s legal block-size interval maps into RELAY rather than exposing only an unexplained maximum. The remaining potential corrections below strengthen evidence reproducibility and prevent packaging/instrumentation claims from outrunning the sources.

### Source 1 — Truce Install guide

- Truce currently requires Rust **1.92+**, a platform C/C++ compiler, and the `cargo-truce` CLI. The guide documents installing the CLI at an exact crates.io version or exact Git tag, supporting P1’s requirement to pin rather than use a floating reference.
- The default first-timer surface is CLAP and VST3 across macOS, Windows, and Linux. AU v2 is macOS-only; AU v3 requires full Xcode and a real Apple developer identity; AAX is macOS/Windows-only and additionally requires the Avid SDK plus PACE/iLok for retail releases. The plan is therefore correct not to infer the V1 matrix from nominal framework support.
- `cargo truce doctor` checks optional validator binaries, while `cargo truce package` has distinct platform signing inputs. The guide says Linux has no signed-installer support yet. Separate spike, validation, and release gates are appropriate; one CLI command is not proof of shippability.
- Windows packaging is described as universal x64 + ARM64 by default only when the Rust target and MSVC cross-tools are installed. The spike must record produced and inspected architectures, not merely requested targets.

### Source 2 — Truce Real-time safety guide

- Truce’s optional `rt-paranoid` feature guards the real `process()` path used by its test driver and can also be enabled in a diagnostic plugin loaded by a DAW. With the feature disabled, its guard and custom allocator compile away.
- Its primary assertion detects allocations. Deallocation detection is opt-in, and lock detection only covers `truce::rt::Mutex` / `RwLock`; the guide explicitly says it does **not** see `std::sync::Mutex`, `parking_lot`, or directly reached OS primitives. It does not claim syscall, wait, network, or deadline instrumentation.
- The guide warns that only executed paths are checked and specifically calls out parameter-change and state-load paths as requiring scripted coverage. P3 is correct to require transition/state-restore stress rather than accepting a steady-state passthrough test.
- `allow_alloc` can suppress checks inside a region, and `Mode::Count` reports after a block. A green result requires retaining checker configuration and auditing suppressions, not merely reporting “no failure.”

### Source 3 — Official CLAP plugin lifecycle contract

- CLAP specifies `init` on the main thread, followed by a deactivated state. `activate` / `deactivate` are main-thread calls; activation receives sample rate plus **minimum and maximum** frame counts and may allocate/prepare resources.
- CLAP separately specifies `start_processing`, `stop_processing`, and `reset` on the **audio thread**. `process` is valid only while active and processing. Destruction is main-thread-only and requires prior deactivation.
- While active, CLAP requires sample rate and process-frame bounds to remain stable, and requires latency and port configuration to remain constant until deactivation. Re-prepare/layout/latency tests are appropriate, but the adapter must not assume those properties can change in place during an active CLAP interval.
- Pointers in `clap_process_t` and its nested structures are borrowed only until `process()` returns. This supports P1’s borrowed-data requirement, but does not by itself prove that the selected Truce revision exposes every required borrowing shape; the spike still must prove that.

### Source 4 — Official `clap-validator`

- The validator tests `.clap` artifacts for common bugs and incorrect behavior. Tests run in separate processes by default so a plugin crash does not take down the validator.
- All tests, including pedantic tests, run by default, but CLI filters and `clap-validator.toml` can exclude or disable tests. A report is reproducible only when the validator version/revision, configuration, invocation, artifact hash, and complete output are retained.
- Its experimental multiprocess fuzzer varies parameters, notes, and transport while checking crashes, hangs, and conformance; it emits seeds for reproduction and warns of possible false positives. This is useful P5/P6 evidence, but does not replace model-based lifecycle tests or real-DAW validation.
- Tracing is explicitly timing-sensitive and may mask a crash. Trace-enabled reruns are diagnostic evidence, not substitutes for untraced runs.

## Explicit Potential Corrections to the Master Plan

These are proposed corrections/clarifications only; this task deliberately did not edit the plan.

1. **Pin the complete Truce toolchain tuple.** P1’s “exact accepted revision” should mean compatible exact versions/revisions of `cargo-truce`, linked Truce crates, generated scaffold/templates, and any owned patch or fork—not one ambiguous framework revision.
2. **Treat `rt-paranoid` as partial evidence, not the P3 gate.** Require its effective features/mode, deallocation checks, lock-wrapper coverage, and every `allow_alloc` site to be recorded. Supplement it with RELAY/platform instrumentation for lock types, syscalls, waits, and deadlines it cannot observe.
3. **Represent activation and processing separately in P5.** The diagram currently collapses both into `Active`. Add a format-neutral processing substate plus explicit start, stop, and reset events so the model can map CLAP without leaking CLAP types into the engine API.
4. **Define the bridge’s block-size interval mapping.** CLAP activation supplies `[min_frames_count, max_frames_count]`, while P2’s `prepare` takes only `max_block`. Either carry the legal interval/capabilities or explicitly specify why RELAY may ignore the minimum and prove correct behavior for every legal frame count. Keep zero-frame tests only for formats/adapters whose contracts permit them.
5. **Freeze validator provenance and effective configuration.** P6 should retain exact validator version/revision, filters/configuration, invocation, artifact hash, full output, fuzz seed/duration/worker count, and explicit waivers for disabled/skipped tests. “Current validator” alone is not reproducible.
6. **Do not imply Truce supplies Linux signed installers.** The current official guide says it does not. Any Linux distribution promise needs a RELAY-owned packaging path or an explicitly approved unsigned/package-manager route. The existing P7 trust-boundary design is compatible with this clarification.

## Decisions Reflected in the Plan

| Plan decision | Validation result | Evidence/rationale |
|---|---|---|
| Keep Truce provisional behind P1 | **Confirmed** | Official docs show useful coverage but format/platform prerequisites and validation gaps that require an exact-revision spike |
| Keep the shell thin and framework types out of `ProcessorEngineBridge` | **Confirmed** | CLAP has format-specific lifecycle/thread states that should be translated at the adapter boundary, not become engine policy |
| Borrow host buffers/events and forbid ownership transfer in process | **Confirmed as a requirement; spike proof still required** | CLAP process pointers expire on return; the selected Truce revision’s Rust borrowing surface remains a P1 question |
| Use automated RT instrumentation plus stress/path coverage | **Confirmed with correction 2** | Truce offers useful allocation/deallocation/selected-lock checks but documents blind spots and executed-path dependence |
| Freeze formats only after artifacts are built, scanned, and host-loaded | **Confirmed** | Truce prerequisites vary by format/platform; validator behavior is configurable and cannot prove host acceptance |
| Model lifecycle explicitly and adversarially | **Confirmed with corrections 3–4** | CLAP’s separate main/audio-thread phases and block interval require more detail than the current diagram/API shows |
| Separate build provenance, bundle signing, and installer trust | **Confirmed** | Truce documents distinct signing/package inputs and a Linux installer limitation; RELAY must own the release evidence |
| Keep authentication/secrets outside host state | **Not contradicted; outside the four-source scope** | None of the consulted sources justify weakening P4; dedicated security validation remains required |

## Final Disposition

**Conditionally validate the plugin-shell plan for continued planning, not for Truce acceptance or implementation start.** Preserve P0 and the P1 accept/patch/reject spike. Before the future implementation epic opens, incorporate or explicitly resolve corrections 1–6—especially the separate processing lifecycle/reset model, block-interval mapping, partial nature of `rt-paranoid`, and reproducible validator provenance. No source reviewed here supports bypassing P0, preselecting the shipping format matrix, or claiming that Truce has already passed P1.

## Validation Proof

- This evidence file was created before any source consultation, with scope, criteria, source table, findings, potential corrections, decisions, and proof sections already present.
- It was updated immediately after each of the four targeted source fetches; no fifth primary source was consulted.
- All four source fetches returned `HTTP 200`. Snapshot SHA-256 values recorded during this validation:
  - Truce Install guide HTML: `84115f9864d35259203126060c252803b62dd62d93834be17e4e1bafe917f205`
  - Truce Real-time safety guide HTML: `7b4a91e9562b189520662c9404fb78fbe499c64762dc13f64cd90dac807a8b06`
  - CLAP `include/clap/plugin.h`: `d2a178137349749eb1dd9c1d6a437300309044626dcc14b0f6e31995f1c5f25b`
  - `clap-validator` README: `dc507d90130808c817b416a5f276fbec5c138e829d8619ac2e9b347b07043687`
- The Truce pages and GitHub `main` / `master` links are moving references. The hashes make this review’s fetched content identifiable, but P1/P6 must replace moving references with exact released versions or commit IDs in execution evidence.
- No plan or code file was edited by this validation task.
