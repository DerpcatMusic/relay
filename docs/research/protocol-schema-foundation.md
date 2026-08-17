# Protocol schema foundation

This document records the initial `relay.v1` schema decisions and the narrow
source validation performed for the protocol skeleton.

## Scope

The foundation consists of:

- `proto/buf.yaml`: one Buf v2 module rooted at `proto/`, with `STANDARD` lint
  and `FILE` breaking-change policy.
- `proto/buf.gen.yaml`: Buf v2 remote generation for Rust (`prost`) and
  TypeScript (`protobuf-es`).
- `common.proto`: protocol versions, endpoints, transports, and safe trace
  correlation.
- `capabilities.proto`: negotiated audio and signaling capabilities.
- `signaling.proto`: a versioned envelope and typed signaling payloads.
- `telemetry.proto`: bounded, typed operational measurements.

## Envelope and evolution

`Envelope` assigns stable fields 1 through 5 to protocol version, message ID,
session ID, peer ID, and monotonic state revision. Its payload is a `oneof`
containing hello, welcome, offer, answer, ICE candidate, peer update, route
update, and error messages. Payload tags begin at 16 so the compact 1–15 range
remains available for future frequently-present envelope metadata.

Messages and enums reserve unused tag ranges. Once published, an existing tag
must not be repurposed; removed tags and names should be added to `reserved`
declarations. The revision is an unsigned 64-bit value so implementations can
reject stale state updates without coupling revision order to wall-clock time.

## Explicit corrections

1. **Buf v2 generation syntax.** The generator uses `version: v2`, `plugins`,
   and each BSR plugin's `remote`, `out`, and optional `opt` keys. It does not
   use legacy `plugin:` declarations. `clean: true` is intentional: generated
   output directories are removed before regeneration.
2. **Fractional Opus frame durations.** Frame duration fields are integer
   microseconds (`frame_durations_us` and `frame_duration_us`), not integer
   milliseconds. An Opus duration of 2.5 ms is represented exactly as `2500`;
   it is not truncated or rounded to `2` or `3` ms.
3. **Evolution space.** Common, capability, signaling, and telemetry messages
   reserve blocks of unused field numbers. Envelope metadata stays in fields
   1–5 while payload alternatives occupy 16–23.
4. **Telemetry secrecy.** Telemetry exposes only reviewed, typed measurements
   and coarse error classes. It intentionally has no arbitrary attributes,
   detailed error text, endpoint metadata, SDP, ICE candidate text, media,
   cookies, authentication tokens, or credentials. Trace fields are opaque
   correlation identifiers only.
5. **Typed payloads.** Signaling variants are distinct messages inside a
   `oneof`, rather than a string kind plus an unvalidated byte or JSON body.
   This makes invalid simultaneous payload variants unrepresentable on the
   wire.

## Official source checks

Exactly two official references were checked on 2026-08-15:

1. [Buf `buf.gen.yaml` v2 reference](https://buf.build/docs/configuration/v2/buf-gen-yaml/)
   confirms that the current configuration version is `v2`, `clean: true`
   deletes each plugin output before generation, and a public BSR remote plugin
   uses `remote: buf.build/<owner>/<plugin>:<version>` with `out` and `opt`.
2. [Protocol Buffers proto3 language guide](https://protobuf.dev/programming-guides/proto3/)
   confirms that field numbers are stable wire identifiers, fields 1–15 have
   the compact one-byte tag encoding, and field numbers must never be reused.
   Those rules motivate the stable envelope core and reserved ranges.

No additional documentation sources were consulted for this foundation.

## Validation

Run from `proto/`:

```console
$ npx --yes @bufbuild/buf lint
# passed (exit 0)

$ npx --yes @bufbuild/buf build
# passed (exit 0)
```

These commands validate Buf configuration discovery, imports, proto3 syntax,
package/file layout, enum naming, field naming, and descriptor construction.
Remote code generation was not run, so this change does not add generated
artifacts.
