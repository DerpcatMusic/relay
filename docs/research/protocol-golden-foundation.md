# Protocol generated consumers and golden compatibility foundation

**Date:** 2026-08-15  
**Status:** Complete

## Scope and acceptance criteria

This Phase-0 task seeds checked-in Rust and TypeScript consumers for the
existing `relay.v1` schemas and one cross-language Hello/Resume wire fixture.
It intentionally does not add either consumer to a root build command.

- [x] `crates/relay-protocol` exposes Buf-generated prost messages and tests the
  golden fixture.
- [x] `packages/protocol` exposes Buf-generated protobuf-es schemas and tests
  the same fixture.
- [x] `tests/fixtures/protocol/hello-resume-v1.bin` has documented,
  deterministic regeneration.
- [x] Generator and runtime versions are pinned; generated files are checked in
  and explicitly marked as generated.
- [x] Root `Cargo.toml`, `package.json`, and `pnpm-workspace.yaml` remain
  byte-for-byte unchanged.

## Sources consulted

Exactly four upstream primary sources were consulted on 2026-08-15:

1. [Buf `buf.gen.yaml` v2 reference](https://buf.build/docs/configuration/v2/buf-gen-yaml/)
   defines `clean`, remote plugins, output directories, and plugin options.
2. [`prost-build` 0.14.4 documentation](https://docs.rs/prost-build/0.14.4/prost_build/)
   documents the alternative build-script/`OUT_DIR` generation model and the
   generated module inclusion pattern.
3. [protobuf-es repository README](https://github.com/bufbuild/protobuf-es/blob/main/README.md)
   documents `@bufbuild/protobuf`, `protoc-gen-es`, `target=ts`, and the
   `create`/`fromBinary`/`toBinary` schema API.
4. [Protocol Buffers encoding guide](https://protobuf.dev/programming-guides/encoding/)
   states that Protobuf serialization is not generally canonical and that
   deterministic output is not guaranteed across different binaries or
   versions.

## Findings

- Buf v2 can send each plugin directly into its consumer directory. The
  existing remote plugin versions remain pinned, while the template now emits
  Rust to `crates/relay-protocol/src/generated` and TypeScript to
  `packages/protocol/src/generated`.
- The TypeScript generator needs `import_extension=js`. This keeps checked-in
  TypeScript resolvable under `NodeNext` and makes the emitted ESM imports valid
  in Node.
- Although `prost-build` supports build-time generation, that would make every
  Rust consumer build depend on a compiler/toolchain setup. Checked-in Buf
  output is the narrower Phase-0 policy: consumers build offline, and one
  explicit command regenerates both languages.
- A byte fixture must not be presented as a universally canonical Protobuf
  representation. This fixture contains no map fields, uses fixed field values
  and pinned runtimes, and is guarded by decode plus byte-identical re-encode
  tests in both languages.
- The pnpm lockfile needs a `packages/protocol` importer even though no root
  JavaScript manifest changes are required. It was refreshed with the pinned
  root package-manager version.

## Decisions applied

- Keep generated sources in version control; never hand-edit them. Both
  consumer READMEs point to the single pinned Buf command:

  ```sh
  cd proto
  npx --yes @bufbuild/buf@1.57.2 generate
  ```

- Pin the generated-code runtimes to `prost = 0.14.4` and
  `@bufbuild/protobuf = 2.2.3`; retain generator pins
  `neoeinstein-prost:v0.4.0` and `bufbuild/es:v2.2.3`.
- Make `relay-protocol` a temporary nested standalone Cargo workspace so its
  manifest can be validated without changing the root workspace membership.
- Generate the 163-byte binary only through the Rust generated consumer. Its
  SHA-256 is
  `b3c492eba4760e099987ddc0ebaf9fde267c170cfce76b87c9d55a7c7add486d`.
- Skip rustfmt traversal only for the generated Rust module. Handwritten Rust
  remains formatted, while a regeneration remains byte-for-byte identical to
  the pinned plugin output.

## Potential corrections to the master plan

1. **Cargo integration needs a separately owned root change.** The root
   workspace currently lists only `relay-domain`, and this task was forbidden
   from editing it. `relay-protocol` therefore contains a temporary
   `[workspace]` table for isolated Cargo use. When the root manifest is
   intentionally updated, add `crates/relay-protocol` as a member and remove
   that nested table in the same change.
2. **Golden bytes are compatibility sentinels, not canonical serialization.**
   The plan should not infer that arbitrary messages (especially messages with
   maps) will serialize identically across all runtimes and versions. Golden
   coverage should use intentionally deterministic fixtures and pinned
   toolchains.
3. **Generated Rust is not rustfmt-normalized by the selected remote plugin.**
   Running rustfmt over it would create an unreproducible post-generation diff.
   The checked-in module is excluded from rustfmt instead; handwritten files
   still pass format checks.

No schema or wire-contract correction was required.

## Validation evidence

All commands ran from the repository root unless shown otherwise.

```text
$ cd proto && npx --yes @bufbuild/buf@1.57.2 lint
exit 0

$ cd proto && npx --yes @bufbuild/buf@1.57.2 build
exit 0

$ cd proto && npx --yes @bufbuild/buf@1.57.2 generate
exit 0
$ diff generated-checksums-before generated-checksums-after
exit 0; all five generated files were byte-identical
```

```text
$ cargo fmt --manifest-path crates/relay-protocol/Cargo.toml -- --check
exit 0

$ cargo test --manifest-path crates/relay-protocol/Cargo.toml --locked
exit 0; 1 golden integration test passed

$ cargo clippy --manifest-path crates/relay-protocol/Cargo.toml \
    --all-targets --locked -- -D warnings
exit 0
```

```text
$ npx --yes pnpm@11.22.0 install --lockfile-only
exit 0; lockfile gained the packages/protocol importer

$ npx --yes pnpm@11.22.0 install --filter @relay/protocol --frozen-lockfile
exit 0

$ npx --yes pnpm@11.22.0 --filter @relay/protocol typecheck
exit 0

$ npx --yes pnpm@11.22.0 --filter @relay/protocol test
exit 0; 1 Node golden compatibility test passed
```

```text
$ cargo run --manifest-path crates/relay-protocol/Cargo.toml \
    --example regenerate_golden --locked
exit 0
$ sha256sum tests/fixtures/protocol/hello-resume-v1.bin
b3c492eba4760e099987ddc0ebaf9fde267c170cfce76b87c9d55a7c7add486d
```

The fixture checksum was unchanged before and after regeneration. A direct
in-memory comparison also confirmed that the three prohibited root manifests
were byte-for-byte unchanged from their pre-task contents.


## Parent integration amendment

After the scoped task, the parent integration added `relay-protocol` to the root workspace and removed its temporary nested `[workspace]` and crate-local lockfile as required above. It also corrected the fixture to advertise the highest supported minor first and to use the product V1 duration set `{5000, 10000, 20000}` from `docs/protocols/signaling-v1.md`. The fixture was intentionally regenerated; its integrated SHA-256 is:

```text
8ed8b1cd5901a987cbf2c45c2c04cac7135ef53e24bea2e00c4882619869e8d0
```

The original scoped-task checksum remains above as historical evidence, not the current baseline.
