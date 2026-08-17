# Protocol golden fixtures

`hello-resume-v1.bin` is the canonical deterministic `relay.v1.Envelope`
fixture for cross-language compatibility. It contains a `Hello` payload with a
`ResumeRequest`; its token is an inert, explicitly fake test value.

Regenerate generated consumers first, then regenerate the binary through the
Rust generated consumer:

```sh
cd proto
npx --yes @bufbuild/buf@1.57.2 generate
cd ..
cargo run --manifest-path crates/relay-protocol/Cargo.toml   --example regenerate_golden --locked
```

The fixture deliberately contains no Protobuf map fields, whose serialization
order can otherwise vary. Both the Rust and TypeScript tests decode the same
bytes, assert the Hello/Resume discriminants and key values, and require a
byte-identical re-encode. Never edit the binary or generated source by hand.
