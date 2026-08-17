# Relay Opus canonical V1 profile controls

## Scope

Close relay-audio foundation audit F3 at the codec boundary by making the Relay Opus V1 encoder policy explicit, typed, versioned, and independent of implicit libopus defaults. Changes are restricted to `relay-opus`, `relay-opus-sys`, and only if required `relay-domain`.

## Official sources (maximum three)

1. [libopus 1.6.1 `opus_defines.h`](https://github.com/xiph/opus/blob/v1.6.1/include/opus_defines.h) — exact CTL request values, enum values, ranges, and encoder reset contract.
2. [libopus 1.6.1 encoder API](https://opus-codec.org/docs/opus_api-1.6/group__opus__encoder.html) — encoder construction, supported frame sizes, CTL semantics, and application guidance.
3. [RFC 6716](https://www.rfc-editor.org/rfc/rfc6716) — Opus interoperability and packet/frame domain constraints.

## Product and domain constraints

- V1 media format remains 48 kHz stereo.
- Negotiated packet durations remain 5, 10, or 20 ms.
- Bitrate and FEC/loss-hint behavior remain negotiated/domain-driven.
- DTX is forced off because continuous master/program audio must not disappear during quiet passages.
- No DRED is introduced.
- Streaming encode operations must not allocate, lock, or log.
- All raw variadic CTL calls remain quarantined in `relay-opus-sys`; the safe crate exposes typed enums, checked ranges, configuration, and getters.

## Canonical V1 encoder decisions

- Application: `OPUS_APPLICATION_AUDIO` (2049), appropriate for music/mixed program material rather than VoIP speech optimization or restricted-low-delay tradeoffs.
- Complexity: 10, fixed deliberately for maximum encoder analysis quality on the master path.
- VBR: enabled, allowing the codec to spend bits according to program complexity while the negotiated target bitrate remains authoritative.
- Bandwidth: `OPUS_BANDWIDTH_FULLBAND` (1105), matching the 48 kHz master/product profile.
- Signal: `OPUS_SIGNAL_MUSIC` (3002), matching the product's music/master program constraint rather than leaving content classification implicit.
- DTX: disabled explicitly.
- In-band FEC: explicit from the negotiated policy; the packet-loss percentage hint is explicit and compatible with FEC state.
- Bitrate: explicit from the negotiated/domain policy and checked against libopus 1.6 bounds.
- Reset: `OPUS_RESET_STATE` (4028), followed by reapplication of every canonical and negotiated setting so reset cannot restore hidden defaults.

## Potential corrections to verify during implementation

- Confirm existing bitrate domain bounds are within libopus's accepted `OPUS_SET_BITRATE` range (500..512000 bits/s for the encoder as a whole, plus sentinel values only if intentionally supported; V1 uses concrete values).
- Confirm the loss hint is checked as 0..=100 and never inferred merely from FEC enablement.
- Confirm boolean CTLs use libopus integer 0/1 values through non-variadic wrappers rather than passing Rust `bool` through C variadics.
- Confirm getter CTLs use the exact paired request constants and output pointer types.
- Confirm encoder reset is not treated as preserving CTL configuration; reapply the complete typed profile after reset.
- Confirm linked runtime reports libopus 1.6.1 in the environment smoke test without over-constraining portable consumers.

## Validation plan

- Exact-constant and ABI-oriented `relay-opus-sys` tests/checks for bitrate, complexity, VBR, bandwidth, signal, DTX, FEC, loss hint, getters, and reset wrappers.
- Negative safe-range tests for bitrate, complexity, and loss percentage.
- Safe configuration getter tests proving each V1 field is explicit.
- Encoder CTL getter roundtrip tests before and after reset.
- FEC compatibility tests across enabled/disabled policies and loss hints.
- Cross-duration invariants for 5/10/20 ms at 48 kHz stereo.
- Linked libopus 1.6.1 smoke evidence.
- Locked formatting, package/workspace check and test, release build/check, and strict Clippy (`-D warnings`).

## Implementation evidence

### Implemented boundary

- `relay-opus-sys` now owns the exact libopus 1.6 request constants and the only variadic FFI calls. Its non-variadic Rust methods check concrete bitrate (500..=512000), complexity (0..=10), bandwidth, signal, FEC (0/1/2), and loss hint (0..=100) before crossing FFI. Boolean VBR/DTX values are converted to C `int`; paired getters write through C `int` pointers; reset takes no variadic argument.
- `relay-opus` now exposes checked `Bitrate`, `Complexity`, and `PacketLossPercent` value types plus typed `Application`, `VbrMode`, `Bandwidth`, `Signal`, `DtxMode`, and `InbandFec` enums.
- `EncoderPolicyV1` requires negotiated bitrate, FEC mode, and loss hint. Its fixed getters expose application Audio, complexity 10, VBR enabled, Fullband, Music signal, and DTX disabled. `EncoderConfigV1` couples that policy only to 48 kHz stereo and 5/10/20 ms.
- Encoder construction passes Audio to `opus_encoder_create` and then explicitly applies bitrate, complexity, VBR, bandwidth, signal, DTX, FEC, and loss hint. `Encoder::reset` invokes `OPUS_RESET_STATE` and reapplies the complete current policy. Runtime getters query the linked codec rather than merely echoing configuration.
- Steady-state encode, setters/getters, and reset use stack values and caller-owned buffers only: no Rust allocation, locks, or logging. No DRED surface was added.

### Corrections resolved

- The concrete bitrate range is checked against libopus's total encoder bitrate range; sentinel values are deliberately excluded from V1.
- Packet loss is an independent checked 0..=100 hint and is not inferred from FEC.
- libopus 1.6's three FEC values are represented, including value 2 (`EnabledWithoutSilkSwitch`).
- Reset is never assumed to retain settings: all eight controls are reapplied.
- The environment smoke test proves the dynamically linked implementation reports exactly `libopus 1.6.1`.

### Tests added

- Exact CTL/enum constant assertions and sys-layer rejection tests.
- Safe negative/out-of-range tests for bitrate, complexity, and loss hint.
- V1 policy/config getter assertions for every fixed and negotiated field.
- Runtime setter/getter roundtrip plus reset/reapply verification.
- All three libopus 1.6 FEC modes with an explicit loss hint.
- Packet sample-count and fixed-control invariants across 5/10/20 ms.
- Existing FEC recovery/PLC compatibility and cross-duration rejection remain green.
- Exact linked `libopus 1.6.1` smoke.

### Validation completed

All commands used the locked dependency graph and passed:

```text
cargo fmt --all -- --check
cargo check --locked -p relay-opus-sys -p relay-opus --all-targets --all-features
cargo test --locked -p relay-opus-sys -p relay-opus --all-targets
cargo clippy --locked -p relay-opus-sys -p relay-opus --all-targets --all-features -- -D warnings
cargo check --locked --release -p relay-opus-sys -p relay-opus --all-targets --all-features
cargo test --locked --release -p relay-opus-sys -p relay-opus --all-targets --all-features
cargo test --locked --release -p relay-opus --lib tests::release_steady_state_codec_gate -- --ignored --exact
cargo check --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo check --locked --release --workspace --all-targets --all-features
cargo test --locked --release --workspace --all-targets --all-features
```

Package debug result: 18 tests passed (15 safe-boundary plus 3 sys-quarantine). Package release result: the same 18 tests passed, and the explicitly invoked steady-state gate passed in 1.21 seconds. Workspace debug and release suites passed; strict workspace Clippy produced no warnings.
