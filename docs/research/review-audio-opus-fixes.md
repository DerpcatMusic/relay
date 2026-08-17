# Opus O1/O2/T1 Fix Evidence

## Scope and disposition

This change addresses the Opus findings in `review-audio-codecs.md` without changing the resampler or workspace configuration.

| Finding | Disposition | Fix / evidence |
| --- | --- | --- |
| O1 | Fixed | Ordinary packet decode now inspects packet duration before touching decoder state or caller output and returns `Error::UnexpectedDecodedDuration { expected, actual }` unless it exactly equals the negotiated duration. Cross-duration 5 ms → 20 ms and 10 ms → 20 ms tests assert the dedicated error and unchanged output. PLC and successful FEC/ordinary paths also require exactly one configured frame. |
| O2 | Fixed | `Decoder::decode_fec` documents the mandatory one-packet-late sequence: call `decode_fec(following)` for the lost frame, then `decode(following)` for the current frame. A two-frame, non-silent Voice-mode test enables FEC, supplies a 35% loss hint, drops frame one, proves non-silent recovery from frame two, then normally decodes that same packet. A separate no-FEC test proves `decode_fec` equals explicit PLC from equivalent fresh decoder state. |
| T1 | Partially fixed; allocation instrumentation explicitly deferred | Numeric/state-order regression tests were strengthened and a safe optimized steady-state throughput gate was added. A direct allocation counter is not added because workspace lint forbids unsafe code, while a Rust global allocator implementation necessarily requires `unsafe` (`GlobalAlloc` implementation and allocator calls). Weakening the lint or placing an unsafe exception outside the FFI quarantine would violate the review boundary. The processing source remains caller-buffer based and allocation-free at the Rust facade. |

## O1 duration and state-order details

`relay-opus-sys::packet_samples_per_channel` is a narrow safe wrapper around libopus's stateless packet-duration query. It validates the nonempty slice and checked `i32` length before the quarantined FFI call. `Decoder::decode` uses it before the stateful decode call. Therefore a negotiated-duration mismatch:

1. returns the explicit expected and actual samples per channel;
2. does not advance libopus decoder state; and
3. does not alter caller-owned output.

The decoded length is checked again after every ordinary, FEC, and PLC call. No processing allocation, lock, logging, or owned output buffer was introduced.

## O2 FEC call sequence

For missing packet **N** when packet **N+1** arrives:

1. call `decode_fec(packet_n_plus_1, lost_output)` to recover **N** (or receive libopus PLC fallback when no usable FEC is present);
2. call `decode(packet_n_plus_1, current_output)` to decode **N+1** normally;
3. do not discard **N+1** after the FEC call, because FEC decode only emits the previous frame.

The regression uses two distinguishable sine-like voiced frames, Voice application mode, enabled in-band FEC, and a nonzero packet-loss hint. It asserts exact duration, finite samples, nontrivial recovered/current energy, and distinct outputs. The fallback regression disables FEC and compares the FEC-request path with explicit PLC under equivalent initial decoder state.

## T1 safe regression gate and instrumentation deferral

The ignored `release_steady_state_codec_gate` performs 10,000 encode/decode iterations after construction using fixed caller-owned stack buffers. It asserts finite output and a deliberately broad 10-second release budget. Run it explicitly as a required optimized gate:

```sh
cargo test --locked --release -p relay-opus -- --ignored
```

This is a practical throughput/regression sentinel, not proof of zero allocation. Exact allocation instrumentation is deferred until the repository provides a lint-compatible allocation observer (for example an external profiler/test harness that requires no unsafe code in the workspace) or explicitly approves an unsafe test-only allocator outside production crates. The unsafe-code prohibition was not weakened, and FFI remains quarantined in `relay-opus-sys`.

## Validation

Commands and results from this environment:

```text
cargo fmt -p relay-opus -p relay-opus-sys -- --check
  passed

cargo test --locked -p relay-opus-sys -p relay-opus
  relay-opus: 10 passed; relay-opus-sys/doc tests passed

cargo check --locked --workspace --all-targets --all-features
  passed

cargo test --locked --workspace --all-targets --all-features
  passed (workspace; release-only gate excluded by cfg)

cargo test --locked --release --workspace --all-targets --all-features
  passed (workspace; ignored release gate not selected)

cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo clippy --locked --release --workspace --all-targets --all-features -- -D warnings
  both passed, no diagnostics

cargo test --locked --release -p relay-opus -- --ignored
  release steady-state gate: 1 passed (10,000 iterations in about 1.04 s)
```

Formatting was deliberately scoped to the two permitted crates; no unrelated workspace source was reformatted.
