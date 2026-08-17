# Independent Review: `relay-opus` Canonical V1 Profile and `relay-opus-sys` FFI

## Scope and review baseline

This is an independent, read-only audit of the new canonical V1 encoder policy in
`crates/relay-opus` and its variadic-control quarantine in `crates/relay-opus-sys`.
No production code was changed. The review checked:

- every implemented encoder CTL request, input value type, getter pointer type,
  enum/range check, and reset call;
- the libopus 1.6.1 meaning of in-band FEC value `2`;
- canonical policy completeness and the application/bitrate/complexity/VBR/
  bandwidth/signal/DTX/FEC/loss decisions;
- reset policy reapplication and the state exposed after a failure;
- `Send`/`!Sync`, allocation/lock/logging behavior, API compatibility, and tests;
- debug, release, release steady-state, strict Clippy, and the actually linked
  libopus version.

## Normative sources

Per the task constraint, this review used only the three official sources already
listed by `relay-opus-canonical-profile-controls.md`:

1. [libopus 1.6.1 `opus_defines.h`](https://github.com/xiph/opus/blob/v1.6.1/include/opus_defines.h)
2. [libopus 1.6 encoder API](https://opus-codec.org/docs/opus_api-1.6/group__opus__encoder.html)
3. [RFC 6716](https://www.rfc-editor.org/rfc/rfc6716)

The installed header used for local comparison and the dynamically loaded library
both report 1.6.1.

## Findings

### Critical

None.

### High

None.

### Medium

#### M1 — “Explicit VBR” still depends on libopus's constrained-VBR default

`EncoderPolicyV1` fixes only `VbrMode::Enabled` and `apply_policy` calls only
`OPUS_SET_VBR(1)` (`relay-opus/src/lib.rs:297-303, 693-716`). The official header
states that the exact VBR type is controlled by `OPUS_SET_VBR_CONSTRAINT`, whose
default is constrained VBR. There is no request wrapper, typed policy decision,
getter, reset reapplication, or test for request 4020/4021.

Consequently the claims that no encoder behavior is left to an implementation
default and that the complete V1 policy is reapplied are too strong. The resulting
codec is valid, but a material bitrate/buffering behavior remains implicit.

**Correction:** Decide constrained versus unconstrained VBR as part of V1; add
typed set/get wrappers for 4020/4021 using `opus_int32` / `opus_int32 *`; apply it
on construction and reset; expose and round-trip it in tests. Alternatively narrow
the stated guarantee and explicitly document reliance on the 1.6.1 default.

#### M2 — A failed reset can leave a usable, partially configured encoder whose `config()` still claims the full policy

`reset()` first mutates libopus with `OPUS_RESET_STATE`, then performs eight
fallible setters in sequence (`relay-opus/src/lib.rs:591-595, 693-716`). If any
setter fails, the method returns `Err`, but the object remains callable and
`config()` still returns the desired policy. `encode`, getters, and negotiated
setters have no poisoned/unconfigured-state guard (`:585-690`). Failure can
therefore leave default or partially reapplied controls behind the canonical V1
type.

The happy-path reset test (`:1156-1200`) does not exercise this state. Prevalidated
values make `BAD_ARG` unlikely against 1.6.1, but libopus can still report other
errors, and the API currently provides no post-failure contract.

**Correction:** Make failure terminal in the type/API: either consume and return
the encoder from reset, or set a `policy_applied`/poison flag before reset and
reject encode/control use until a later full reset succeeds. Document the
contract and add injected-failure tests at each reapplication step. This can be
done without allocation or locking.

#### M3 — Forced fullband conflicts with the admitted 500..=512000 bitrate domain and upstream's quality guidance

The profile forces `OPUS_SET_BANDWIDTH(OPUS_BANDWIDTH_FULLBAND)` for every
accepted concrete bitrate (`relay-opus/src/lib.rs:92-102, 297-303, 693-716`). The
1.6.1 header explicitly says applications should normally use
`OPUS_SET_MAX_BANDWIDTH` and leave `OPUS_SET_BANDWIDTH` at `OPUS_AUTO`, allowing
the encoder to reduce its bandpass at low rates for better quality. A 48 kHz input
rate permits fullband; it does not by itself justify forcing a 20 kHz bandpass,
especially down to 500 bit/s.

This is not an invalid CTL value, but the current rationale (“matching 48 kHz”)
does not resolve the quality conflict across the public V1 bitrate range.

**Correction:** Prefer max-bandwidth=fullband plus bandwidth=auto, or establish a
measured V1 minimum bitrate at which forced fullband is acceptable. Record
quality/CPU evidence for every admitted operating region before retaining the
current force.

#### M4 — FEC mode `2` is a 1.6.1 profile promise, but library compatibility is enforced only by a unit-test assertion

The header confirms that `OPUS_SET_INBAND_FEC(2)` means enabled without
necessarily switching music to SILK. The binding and typed enum are correct
(`relay-opus-sys/src/lib.rs:248-257`; `relay-opus/src/lib.rs:230-254`). The local
runtime is exactly 1.6.1 and accepts/returns `2`.

However, `relay-opus-sys` uses unconditional `#[link(name = "opus")]` with no
build-time minimum/exact-version discovery. A normal build can therefore link a
different system libopus; the exact `assert_eq!("libopus 1.6.1")` runs only in
tests. It also makes the otherwise portable package test fail on a later
compatible library, contrary to the research note's instruction not to
over-constrain portable consumers.

**Correction:** Enforce the supported libopus floor/pin in packaging or build
discovery, and keep exact-1.6.1 verification as an environment/artifact smoke
gate rather than a generic unit invariant. If system linking remains supported,
document and test the runtime failure behavior for mode `2` on unsupported
versions.

### Low

#### L1 — Application is not included in runtime CTL observation or explicit reset reapplication

Application `Audio` is correctly passed as mandatory construction value 2049 and
has a policy getter. Unlike every other profile field, there is no safe wrapper
for `OPUS_GET_APPLICATION` (4001), no runtime `Encoder::application()`, and no
`OPUS_SET_APPLICATION` reapplication after reset. The official API permits
setting application on a newly initialized or freshly reset encoder.

The original application should survive/equate to fresh initialization, so this
is not evidence of wrong runtime behavior. It is nevertheless a gap in the
claim that every profile decision is runtime-observable and explicitly
reapplied.

**Correction:** Add the paired typed application CTLs and runtime assertion, or
state precisely that the mandatory creation argument is the authoritative
application invariant and is intentionally not one of the reapplied CTLs.

#### L2 — Test names overstate FEC-mode and realtime evidence

`all_fec_modes_are_compatible...` only constructs and reads CTLs for modes 0/1/2
(`relay-opus/src/lib.rs:1203-1219`); it does not encode with mode `2` or prove
recoverable FEC behavior. The release gate (`:1245-1265`) measures 10,000
encode/decode iterations under a generous aggregate deadline, but does not count
allocations, detect locks, measure worst-case per-call latency, or exercise CTL
updates/reset. Complexity 10 therefore has only a coarse single-environment CPU
check.

**Correction:** Add mode-2 encode/decode coverage, deterministic loss sequences,
an allocation-counting/forbidden-allocation gate, and target-device per-call
deadline measurements for all durations and negotiated control changes.

#### L3 — Trait and ABI assertions are mostly structural rather than compile/foreign-header checks

The ownership design is sound by inspection: `PhantomData<Rc<()>>` suppresses
auto-`Sync`, explicit `unsafe impl Send` restores only `Send`, all stateful calls
require `&mut self`, and the safe wrapper inherits those traits
(`relay-opus-sys/src/lib.rs:109-117, 299-301`). There is no compile-time positive
`Send` assertion or negative `Sync` test.

Likewise, constant tests compare Rust constants with literals, while the
set/get roundtrip is good runtime evidence for the paired requests and pointer
ABI. There is no C-header compile assertion tying the Rust definitions to the
selected header.

**Correction:** Add compile-time trait tests and a small header-driven ABI probe
in the versioned integration environment.

## Detailed CTL and FFI matrix

The implemented request/value/pointer pairs match `opus_defines.h` 1.6.1:

| Control | Setter request and input | Getter request and output | Range/value audit | Result |
|---|---:|---:|---|---|
| Bitrate | 4002, `opus_int32` | 4003, `opus_int32 *` | Concrete 500..=512000; sentinels deliberately excluded | Correct |
| VBR enabled | 4006, `opus_int32` | 4007, `opus_int32 *` | 0/1; Rust `bool` converted to C integer | Correct, but constrained-VBR subtype omitted (M1) |
| Bandwidth | 4008, `opus_int32` | 4009, `opus_int32 *` | `OPUS_AUTO` -1000 or 1101..=1105 | ABI correct; policy concern M3 |
| Complexity | 4010, `opus_int32` | 4011, `opus_int32 *` | 0..=10 | Correct |
| In-band FEC | 4012, `opus_int32` | 4013, `opus_int32 *` | 0, 1, or 2 in 1.6.1 | Correct |
| Loss percentage | 4014, `opus_int32` | 4015, `opus_int32 *` | 0..=100 | Correct |
| DTX | 4016, `opus_int32` | 4017, `opus_int32 *` | 0/1; V1 forces 0 | Correct |
| Signal | 4024, `opus_int32` | 4025, `opus_int32 *` | -1000, 3001, or 3002; V1 uses Music 3002 | Correct |
| Reset | 4028, no variadic argument | N/A | Freshly initialized codec state | Correct call shape |
| Application | Constructor value 2049 | 4001 would be `opus_int32 *` | Audio is correct for faithfulness/music program | Constructor correct; runtime CTL omitted (L1) |

`c_int` is the correct C integer representation on the reviewed target; all
getters pass writable stack `c_int` pointers. Raw variadic calls and requests are
private to `relay-opus-sys`. No Rust `bool` crosses a C variadic boundary.

## Profile decision disposition

- **Application Audio (2049):** correct for faithfulness to music/mixed program;
  restricted-low-delay would disable useful modes, while VoIP biases speech.
- **Concrete bitrate:** exact libopus range is implemented; sentinels are
  intentionally excluded. The product should still narrow its operational range
  if fullband is forced (M3).
- **Complexity 10:** valid and explicit maximum-quality choice. Retain only with
  target deadline evidence; the current throughput test is not a worst-case RT
  proof.
- **VBR enabled:** valid, but incomplete until constrained/unconstrained VBR is
  made explicit (M1).
- **Fullband:** valid CTL but not justified solely by 48 kHz, and upstream advises
  automatic bandpass selection for quality at low bitrate (M3).
- **Music signal (3002):** correct as a mode-selection hint for the product's
  music/master constraint.
- **DTX disabled:** correct and explicitly applied/read back; avoids dropping
  quiet continuous program audio.
- **FEC and loss hint:** independently typed and range checked. Modes 0/1/2 and
  0..=100 are correct; mode 2 needs versioned distribution enforcement (M4).
- **No DRED:** none was introduced.

## Reset and failure-state disposition

On success, reset reapplication covers bitrate, complexity, VBR enable,
bandwidth, signal, DTX, FEC, and loss percentage, including negotiated updates.
The success test reads all eight back after reset. Application is supplied at
construction rather than explicitly set/read by CTL, and the constrained-VBR
subtype is absent. On failure, the encoder is neither poisoned nor consumed;
therefore the canonical-state guarantee does not hold unless every caller treats
any reset error as terminal (M2).

## Safety and realtime disposition

- **`Send` / `!Sync`:** correct by inspection for both sys and safe encoders. The
  state may be moved to its owner thread but cannot be shared safely. There are
  no shared-reference state operations.
- **Allocation:** construction allocates libopus state off-thread. Encode,
  setters, getters, successful reset/reapply, and wrapper error mapping use
  stack values and caller-owned buffers; no Rust heap allocation occurs in those
  implementations.
- **Locks/logging/I/O:** none appears in the wrapper streaming/control paths.
- **Proof strength:** the tests do not instrument the allocator or locks and do
  not establish a target-specific worst-case libopus bound (L2). Thus the code
  inspection supports the claim, but the release gate does not prove it.
- **Unsafe quarantine:** all raw state pointers, extern declarations, variadic
  CTLs, and destruction remain inside `relay-opus-sys`; `relay-opus` forbids
  unsafe code.

## Compatibility and API surface

The prior research note documents an `EncoderConfig` API; it has been replaced by
`EncoderPolicyV1` plus `EncoderConfigV1`, so this is a source-breaking public API
change with no deprecated alias or compatibility constructor. All in-tree callers
have migrated, `relay-audio` checks successfully, both packages are unpublished
0.1 crates, and the new surface intentionally prevents callers from selecting a
noncanonical application/control set.

**Disposition:** acceptable as a coordinated internal breaking change. If any
out-of-tree callers exist, provide a migration note or temporary deprecated
adapter; do not restore a legacy constructor that can bypass V1 policy.

## Test-strength summary

Strengths:

- exact request/enum literals and runtime set/get roundtrips for all implemented
  CTLs;
- safe-layer negative range checks;
- all fixed and negotiated policy getters;
- successful reset/readback after negotiated updates;
- all three FEC CTL values and all 5/10/20 ms durations;
- FEC recovery, PLC fallback, malformed input, and linked-version smoke tests.

Gaps:

- no constrained-VBR decision/test;
- no reset failure injection or poisoned-state contract;
- no runtime application getter;
- no positive boundary acceptance checks for every 500/512000, 0/10, and 0/100
  edge at both layers;
- mode 2 is only CTL-roundtripped, not exercised in an encode/loss sequence;
- no compile-time `Send`/negative-`Sync`, allocation, lock, or worst-case deadline
  gate;
- exact-version checking is a generic unit test rather than a packaging/build
  invariant.

## Validation results

Executed from the repository with the locked graph:

| Command | Result |
|---|---|
| `pkg-config --modversion opus` | **PASS** — `1.6.1` |
| `cargo test --locked -p relay-opus-sys -p relay-opus --all-targets --all-features` | **PASS** — 18 passed (15 safe + 3 sys) |
| `cargo test --locked --release -p relay-opus-sys -p relay-opus --all-targets --all-features` | **PASS** — 18 passed; one release-only ignored gate |
| `cargo test --locked --release -p relay-opus --lib tests::release_steady_state_codec_gate -- --ignored --exact` | **PASS** — 1 passed in 1.23 s |
| `cargo clippy --locked -p relay-opus-sys -p relay-opus --all-targets --all-features -- -D warnings` | **PASS** — no warnings |
| C probe calling linked `opus_get_version_string()` | **PASS** — `libopus 1.6.1` |
| `cargo check --locked -p relay-audio --all-targets --all-features` | **PASS** — migrated in-tree consumer compiles |

## Corrections recommended, in order

1. Define and implement the constrained/unconstrained VBR decision (M1).
2. Make any failed reset/reapplication leave a type-enforced unusable/poisoned
   state, with failure injection tests (M2).
3. Reconcile forced fullband with the bitrate domain using max-bandwidth/auto or
   a measured higher V1 minimum bitrate (M3).
4. Enforce the libopus version contract in distribution/build integration and
   relocate exact-version smoke semantics appropriately (M4).
5. Close the application getter/reset-observability and RT/trait/test gaps.

## Overall disposition

**Changes requested before calling the V1 profile completely explicit and
failure-safe.** The implemented CTL numbers, integer values, getter pointer
shapes, primary ranges, FEC value `2`, DTX-off behavior, Music hint, and successful
reset path are correct against libopus 1.6.1. The main blockers are the implicit
constrained-VBR default and the usable partial state after reset-reapplication
failure. Forced fullband across the entire libopus bitrate range and unenforced
1.6.1 linkage also require an explicit product/distribution decision. All required
local validation passes against the linked libopus 1.6.1.
