# relay-opus implementation research

## Scope and decision

This Phase-1 seed implements only `crates/relay-opus-sys` and `crates/relay-opus`; this scoped work did not edit the root manifest. The current shared tree lists both crates as workspace members as part of separate concurrent integration.

The spike links the system `libopus` shared library (`#[link(name = "opus")]`). `relay-opus-sys` owns every FFI declaration, unsafe block, raw state pointer, CTL varargs call, and destructor. `relay-opus` has `#![forbid(unsafe_code)]` and exposes only a fixed RELAY format: stereo interleaved `f32`, 48 kHz, and 5/10/20 ms frames.

## Official API findings

- Encoder and decoder states are created once and kept across frames. The create calls allocate codec state; the corresponding destroy calls free it. An individual state must not be used for multiple streams concurrently. [1][2]
- `opus_encode_float` consumes exactly one frame, with `frame_size` expressed in samples **per channel**. Upstream supports more durations, but this boundary admits only 5, 10, and 20 ms: 240, 480, and 960 samples per channel at 48 kHz. The encoder API recommends a 4000-byte packet output buffer. [1]
- `opus_decode_float` returns samples per channel. Its `frame_size` is the maximum output duration that fits in the caller's PCM buffer. Packet loss concealment (PLC) is requested with a null packet pointer and zero length. Packets for one state must be decoded serially and in stream order. [2]
- In-band FEC is an encoder control. Recovery is requested by decoding the **following** packet with `decode_fec = 1`; when usable FEC is absent, libopus may perform PLC. Enabling the control does not guarantee that every packet/frame mode carries recovery data. [1][2]
- The 1.6.1 source README documents the distribution build as `./configure && make`; a git checkout additionally needs `./autogen.sh` first. It describes a shared library for raw Opus bitstreams. [3]
- Upstream redistribution terms permit source and binary redistribution with conditions equivalent to a three-clause BSD license, and the COPYING file also identifies royalty-free patent license grants. A vendored release must preserve the copyright, conditions, disclaimer, and relevant notices. [4]

## Safe API and real-time boundary

### Construction (non-real-time)

- `EncoderConfig::try_new` and `DecoderConfig::try_new` reject anything other than 48,000 Hz and two channels.
- `FrameDuration` is a closed enum (`Ms5`, `Ms10`, `Ms20`); `TryFrom<u16>` rejects other millisecond values.
- `Encoder::new` and `Decoder::new` are fallible. They are the only codec paths that allocate libopus state.
- Encoder configuration includes application, in-band FEC, and expected packet-loss percentage (validated to 0..=100).

### Streaming (real-time-conscious)

- `Encoder::encode` requires one exact interleaved frame and a caller-owned byte slice. It exposes at most 4000 bytes of that slice to libopus and returns the initialized prefix length.
- `Decoder::decode`, `decode_fec`, and `decode_plc` require caller-owned PCM output sized for the configured frame and return both per-channel and interleaved initialized-prefix lengths.
- Empty packets are rejected by normal decode so loss cannot be confused with a malformed/empty packet; PLC is an explicit method.
- Packet inputs above 4000 bytes are rejected at this intentionally narrow boundary.
- After successful construction, the Rust encode/decode/PLC/FEC paths contain no heap allocation, logging, locks, file/network I/O, or dynamic resizing. They perform constant-space validation and one libopus call. Time is linear in the configured frame size; memory is the pre-created codec state plus caller buffers.
- Each operation needs `&mut self`; the sys owners are `Send` but deliberately not `Sync`. This permits moving a prepared codec to its owning stream thread while preventing concurrent safe access to one state.

## FFI safety notes

`relay-opus-sys` does not expose raw pointers or raw extern functions. Its safe narrow owners enforce the preconditions before entering small documented unsafe blocks:

- successful create results are checked for both status and non-null state;
- each state has exactly one owner and exactly one matching destructor;
- PCM lengths are checked as `frame_size * channels` with checked arithmetic;
- output slices are checked before passing writable pointers;
- packet lengths are converted to `i32` without truncation;
- PLC alone passes null data, always with length zero;
- encoder CTL wrappers fix both request number and promoted C `int` argument type;
- all stateful calls require a unique mutable borrow.

The safe crate maps all negative libopus statuses and caller validation failures to non-allocating error enums. Recoverable caller input never uses `unwrap`, `expect`, indexing, or assertions in production code.

## Tests implemented

The safe crate covers:

1. silence encode/decode with caller-owned buffers;
2. impulse encode/decode with finite, non-zero output;
3. invalid sample rate, channel count, and frame duration;
4. too-small encode and decode outputs;
5. explicit PLC;
6. in-band FEC controls and FEC decode entry point;
7. malformed/empty input inside `catch_unwind`, demonstrating errors rather than panics;
8. linked library version reporting.

## Exact local validation

Validated on the current development image against both pkg-config metadata and the loaded library API:

```text
$ pkg-config --modversion opus
1.6.1

$ cc /tmp/relay-opus-version.c $(pkg-config --cflags --libs opus) -o /tmp/relay-opus-version
$ /tmp/relay-opus-version
libopus 1.6.1
```

The temporary C probe called `opus_get_version_string()` from `<opus/opus.h>`.

Rust validation:

```text
$ cargo test --manifest-path crates/relay-opus/Cargo.toml
running 8 tests
........
test result: ok. 8 passed; 0 failed

$ cargo clippy --manifest-path crates/relay-opus/Cargo.toml --all-targets -- -D warnings
Finished `dev` profile

$ cargo test --manifest-path crates/relay-opus-sys/Cargo.toml
test result: ok. 0 passed; 0 failed
```

The manifest-targeted commands exercise the two crates while retaining the repository's shared workspace edition, Rust-version, and lint policy. `relay-opus-sys` explicitly permits unsafe Rust; the workspace forbids it everywhere else, and `relay-opus` additionally has `#![forbid(unsafe_code)]`.

## Potential corrections and release gaps

1. **System linking is only a development spike.** It assumes headers/runtime provisioning happened outside Cargo and that the linker can resolve `-lopus`. There is no minimum-version build check, target-specific discovery, static-link option, cross-compilation story, or Windows/macOS packaging yet.
2. **Portable releases need a pinned vendored source.** Pin an audited libopus 1.6.x release (the validated candidate is 1.6.1), verify its archive/hash, build it reproducibly for every supported target, preserve upstream license/notices, and run target artifact smoke tests. Decide static versus dynamic distribution before shipping.
3. **The 4000-byte limit is an API policy, not a claim about the absolute Opus format maximum.** It follows the upstream encoder recommendation and gives this boundary a small, fixed transport allocation target. Revisit it alongside the wire protocol/MTU decision.
4. **FEC quality needs a network-loss test.** This seed proves control wiring and safe recovery entry points, not recovery quality or that every selected mode emits LBRR. Add deterministic packet-loss sequences, packet inspection, and quality/latency criteria when transport policy is fixed.
5. **Real-time behavior needs release-mode measurement.** The wrapper has no streaming-path allocation/locks, but a shipping gate should add an allocation-counting harness and worst-case encode/decode deadline benchmarks on supported CPUs and on the exact vendored build.
6. **Root integration is separately coordinated.** This scoped implementation did not edit the root manifest. The current shared tree already includes these crates as workspace members, so root lockfile/lint validation belongs to the coordinating agent's complete Phase-1 pass.

## Primary sources (four total)

1. Xiph.Org, libopus 1.6 encoder API: <https://opus-codec.org/docs/opus_api-1.6/group__opus__encoder.html>
2. Xiph.Org, libopus 1.6 decoder API: <https://opus-codec.org/docs/opus_api-1.6/group__opus__decoder.html>
3. Xiph.Org, libopus 1.6.1 README/build instructions: <https://gitlab.xiph.org/xiph/opus/-/raw/v1.6.1/README>
4. Xiph.Org, libopus 1.6.1 COPYING/license and patent notice: <https://gitlab.xiph.org/xiph/opus/-/raw/v1.6.1/COPYING>
