# `relay-opus` profile review fixes

## Scope and baseline

This is the implementation/disposition record for
[`review-relay-opus-profile.md`](review-relay-opus-profile.md). The change is
limited to `crates/relay-opus`, `crates/relay-opus-sys`, their in-module tests,
and this document. No new web research was performed.

The linked development library reported by `pkg-config --modversion opus` is
**1.6.1**. The raw variadic ABI remains private to `relay-opus-sys`; the safe
crate still forbids unsafe code.

## Exact finding-to-fix mapping

| Finding | Disposition | Implementation and evidence |
|---|---|---|
| M1: constrained VBR relied on a default | **Fixed.** V1 explicitly selects constrained VBR. | `VbrConstraint::{Unconstrained, Constrained}` is typed in the safe crate; V1 fixes `VBR_CONSTRAINT` to `Constrained`. The sys quarantine owns typed boolean set/get calls for requests 4020/4021. Construction, reset, runtime observation, constant tests, and set/get tests cover it. |
| M2: reset failure could expose a usable partial policy | **Fixed.** Policy application is transactional at the safe boundary. | `Encoder` starts/enters `policy_applied = false` before initial application and before `OPUS_RESET_STATE`. Encode and every public encoder control getter/setter call `require_policy`. The flag becomes true only after all 11 controls succeed. An injected-failure test fails after every application step, verifies every operation rejects with `EncoderPolicyNotApplied`, and verifies a later complete reset recovers. |
| M3: current bandwidth was forced fullband over the entire bitrate domain | **Fixed.** V1 requests automatic active bandwidth with a fullband ceiling. | V1 fixes `BANDWIDTH = Auto` and `MAX_BANDWIDTH = Fullband`. The sys boundary adds typed max-bandwidth requests 4004/4005 and explicitly sends bandwidth request 4008 with `OPUS_AUTO`. `OPUS_GET_BANDWIDTH` reports the encoder's currently selected concrete bandwidth, not the stored `Auto` request; the safe method documents this distinction and the policy getter remains the authoritative Auto decision. |
| M4: exact 1.6.1 was a generic package invariant while the runtime compatibility contract was not enforced | **Fixed for the current system-linking boundary.** | Encoder construction now rejects a linked version below libopus 1.6. The ordinary test accepts compatible 1.6+, rejects 1.5/malformed versions, and validates the linked runtime against the floor. Exact 1.6.1 moved to the ignored `linked_libopus_1_6_1_artifact_smoke` gate for the pinned CI/artifact environment. Portable vendoring remains distribution work rather than a generic unit invariant. |
| L1: application was neither observable nor explicitly reapplied | **Fixed.** | Application set/get requests 4000/4001 are typed and range checked in sys. V1 applies `Audio` first on construction and first after reset, and exposes a runtime `Encoder::application()` getter. |
| L2: mode-2 FEC and realtime evidence was overstated | **Fixed/narrowed.** | Mode 2 remains `EnabledWithoutSilkSwitch`. A practical three-packet test encodes mode 2, decodes the first packet, drops the second, invokes FEC/PLC recovery from the following packet, then normally decodes that same following packet. The existing mode-1 loss test and release steady-state gate remain. Instrumented allocation counting and target-specific deadlines are explicitly deferred below. |
| L3: positive Send and ABI evidence was incomplete | **Fixed for Rust traits; validation procedure documented for the foreign header.** | Both crates contain compile-time positive `Send` assertions for encoder/decoder owners. The existing `PhantomData<Rc<()>>` continues to suppress `Sync`. Request values, accepted enum/range values, setter argument types, getter pointer types, and linked round trips are tested. The external header/artifact probe procedure is specified below. |
| Boundary acceptance gap | **Fixed.** | Both layers positively exercise bitrate 500/512000, complexity 0/10, and loss 0/100, in addition to negative out-of-range tests. |
| Control/reset/realtime regression coverage | **Fixed to the locally enforceable level.** | Tests cover all 11 fixed/negotiated controls, negotiated updates, complete reset reapplication, every injected partial-application failure, recovery, all frame durations, mode-2 loss handling, caller-owned buffers, and the release steady-state gate. |

## Final V1 policy decision

| Decision | V1 value | Application behavior |
|---|---|---|
| Application | Audio | Passed to construction, then explicitly set/read; reapplied first after reset. |
| Bitrate | Negotiated concrete 500..=512000 bit/s | Typed, range checked, and retained across reset. |
| Complexity | 10 | Explicitly set/read and reapplied. |
| VBR | Enabled | Explicitly set/read and reapplied. |
| VBR constraint | **Constrained** | Explicit V1 decision; request 4020 with integer 1, observed with 4021. |
| Active bandwidth request | **Auto** | Request 4008 receives `OPUS_AUTO`; libopus may report the currently selected concrete bandwidth through 4009. |
| Maximum bandwidth | **Fullband** | Request 4004 receives 1105; observed through 4005. |
| Signal | Music | Explicitly set/read and reapplied. |
| DTX | Disabled | Explicitly set/read and reapplied. |
| In-band FEC | Negotiated 0/1/**2** | Mode 2 is retained and covered by a practical loss sequence. |
| Expected packet loss | Negotiated 0..=100 percent | Typed, range checked, and retained across reset. |

The V1 choice is constrained VBR rather than reliance on libopus's default. The
bandwidth choice is automatic selection capped at fullband, rather than forcing a
20 kHz bandpass at every admitted bitrate.

## Poison/reset contract

The safe encoder has two observable lifecycle states:

1. **Ready:** all 11 policy controls completed successfully; encode, getters, and
   negotiated setters are allowed.
2. **Policy not applied:** entered before reset or application. Any reset/control
   failure leaves the encoder here. Encode and all public encoder getters/setters
   return `Error::EncoderPolicyNotApplied`; they cannot observe or use a partial
   libopus configuration. `reset()` is the only recovery operation and publishes
   Ready only after the complete policy succeeds.

The injected test covers failures after each of these ordered steps: application,
bitrate, complexity, VBR, VBR constraint, maximum bandwidth, automatic bandwidth,
signal, DTX, FEC, and loss percentage. Failing after the final setter is also
kept poisoned, so success publication—not merely the apparent underlying
values—is the transaction boundary.

## Unsafe and ABI disposition

New variadic calls did not broaden the unsafe surface:

- request numbers and raw calls are private constants/operations in
  `relay-opus-sys`;
- setters pass `core::ffi::c_int` values, never Rust `bool`, through C varargs;
- getters pass writable `*mut c_int` stack locations;
- application and maximum-bandwidth setters validate their admitted enum values;
- constrained VBR converts the safe boolean to 0/1 before the call;
- `relay-opus` remains `#![forbid(unsafe_code)]`.

For each pinned CI/distribution artifact, validate the selected foreign header
and the actually loaded library together:

1. Compile a small C probe against the artifact's `opus/opus.h` and statically
   assert request values 4000/4001, 4004/4005, 4020/4021, the existing request
   matrix, and enum values `OPUS_AUTO == -1000` and
   `OPUS_BANDWIDTH_FULLBAND == 1105`.
2. Assert/record `sizeof(opus_int32)`, `_Alignof(opus_int32)`, `sizeof(int)`, and
   `_Alignof(int)` for the target ABI. The Rust boundary represents these CTL
   values/pointers with `c_int`; any target where the selected header disagrees
   must be rejected rather than patched with an untyped variadic escape hatch.
3. Link the probe through the same search path as the Rust artifact, call
   `opus_get_version_string()`, and require exact `libopus 1.6.1` for the pinned
   artifact smoke. Run the ignored Rust exact-version smoke under the same
   loader environment.
4. Keep the Rust request-literal and runtime set/get tests. The C header probe
   establishes header correspondence; the Rust round trips establish the actual
   variadic value/pointer call shape against the loaded library.

## Deferred evidence

- **Allocation instrumentation:** the wrapper streaming/control/reset paths use
  caller-owned/stack storage and contain no Rust allocator calls, locks, logging,
  or I/O, but an allocator-hook/forbidden-allocation test is deferred.
- **Target CPU deadlines:** the existing release gate is a coarse local
  throughput regression test, not a per-call worst-case guarantee. Per-duration
  encode/decode/control/reset deadlines on production target CPUs remain
  deferred.

These deferrals do not permit raw CTLs outside `relay-opus-sys`, do not relax the
poison contract, and do not weaken the safe crate's unsafe-code prohibition.

## Validation evidence

All commands used the locked dependency graph and the linked local libopus 1.6.1.

| Command | Result |
|---|---|
| `pkg-config --modversion opus` | **PASS:** `1.6.1` |
| `cargo test --locked -p relay-opus-sys -p relay-opus --all-targets --all-features` | **PASS:** safe 18 passed / 1 exact-artifact smoke ignored; sys 4 passed |
| `cargo test --locked --release -p relay-opus-sys -p relay-opus --all-targets --all-features` | **PASS:** safe 18 passed / 2 ignored; sys 4 passed |
| `cargo test --locked --release -p relay-opus-sys -p relay-opus --all-targets --all-features -- --ignored` | **PASS:** exact 1.6.1 artifact smoke and release steady-state gate both passed |
| `cargo clippy --locked -p relay-opus-sys -p relay-opus --all-targets --all-features -- -D warnings` | **PASS:** no warnings |
| `cargo check --locked --workspace --all-features` | **PASS** |
| `cargo check --locked --release --workspace --all-features` | **PASS** |
| `cargo test --locked --workspace --lib --all-features` | **PASS:** all workspace library tests; relay-opus safe 18 passed / 1 ignored and sys 4 passed |
| `cargo test --locked --release --workspace --lib --all-features` | **PASS:** all workspace library tests; relay-opus safe 18 passed / 2 ignored and sys 4 passed |
| `cargo test --locked --workspace --all-targets --all-features` | **BLOCKED outside scope:** pre-existing `relay-audio/tests/tx.rs:256` refers to removed field `abandoned_converter_delay_frames` instead of current `abandoned_converter_tail_frames` |
| `cargo test --locked --release --workspace --all-targets --all-features` | **BLOCKED by the same unrelated relay-audio integration-test compile error** |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | **BLOCKED by the same unrelated relay-audio integration-test compile error** |

The forbidden `relay-audio` file was not edited. Package gates and workspace
library/check gates are green; the only incomplete full-workspace gates stop at
the unrelated integration-test source error above, before any test failure in
this change.

## Final disposition

M1, M2, M3, M4, L1, and the actionable portions of L2/L3 are addressed. The V1
profile is now explicit about constrained VBR, application, automatic bandwidth
and its fullband ceiling; reset/application is failure-safe; mode 2 has practical
loss-path coverage; the generic runtime contract is version-compatible while the
exact 1.6.1 check is an artifact gate; and the unsafe quarantine remains narrow.
Allocation instrumentation and production-target deadline characterization are
documented follow-ups rather than unsupported realtime claims.
