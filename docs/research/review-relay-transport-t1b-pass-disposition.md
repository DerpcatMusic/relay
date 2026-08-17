# Relay Transport T1b Pass Disposition

**Review mode:** independent, read-only verification of the sole residual finding in `docs/research/review-relay-transport-t1b-final-disposition.md`. No production source or checked-in test was edited; this report is the only repository deliverable. Independent public-API probes live under `/tmp/relay-t1b-pass-probe`.

**Reviewed implementation:** `crates/relay-transport/src/lib.rs`  
**Focused evidence:** `crates/relay-transport/tests/t1b_blocker_regressions.rs` and the in-crate `x509_name_tests`  
**Prior disposition:** `docs/research/review-relay-transport-t1b-final-disposition.md`

## Disposition

**T1b/fake Gate0: PASS — 0 critical / 0 high / 0 medium.**

The prior final report's sole residual M1 is closed. The exact byte-offset-52 `commonName` INTEGER mutation is rejected by the public constructor in locked debug and release, while the unchanged checked-in Ed25519 certificate remains accepted by both RELAY and OpenSSL. The revised Name validator is deliberately conservative and OID-specific: it admits bounded, well-formed supported string encodings and fails closed on mismatched tags, malformed encodings, unsupported/unknown attribute syntax, and over-limit values. No replacement C/H/M finding was found.

**Native provider probes and provider selection remain OPEN.** Fake Gate0 approval is not provider selection and supplies no live provider, TURN/TLS backend, browser-interoperability, or packaging evidence.

## Sole prior finding re-verification

### M1 — Custom trust accepted a DER-shaped non-X.509 Name: CLOSED

The former permissive `take_any_der_value` behavior is now followed by an OID-specific syntax gate (`src/lib.rs:563-702`). `valid_name` requires both a syntactically valid OID and `valid_name_attribute_value`, rejects trailing attribute fields, and retains DER SET ordering checks (`704-741`). In particular:

- supported X.520 attributes map to bounded `DirectoryString` or exact `PrintableString` rules;
- `domainComponent` and `emailAddress` map to bounded printable-ASCII IA5 values;
- `DirectoryString` accepts only checked UTF8String, PrintableString, BMPString, or UniversalString primitives (`627-683`);
- malformed UTF-8, BMP surrogates/odd lengths, invalid Unicode scalar values, empty strings, disallowed PrintableString characters, mismatched tags, unsupported TeletexString, unknown OIDs, and configured character-limit overruns fail closed;
- countryName is exactly a two-character PrintableString; and
- unknown attribute syntax returns `None` and is rejected rather than guessed.

This is conservative: it may reject valid but unsupported Name encodings or attributes, but it does not broaden trust-anchor acceptance. That is appropriate for this bounded validation boundary.

The checked-in regression now asserts the original byte at exact DER offset 52 is UTF8String tag `0x0c`, changes it to INTEGER tag `0x02`, and requires `InvalidTlsTrust` (`t1b_blocker_regressions.rs:267-305`). The internal focused tests exercise supported primitive encodings, malformed/unsupported tags and values, OID-specific country/domain/email rules, unknown syntax, and the 64/65-character commonName edge (`src/lib.rs:2846-2938`).

## Independent public-API reproduction

The throwaway crate `/tmp/relay-t1b-pass-probe` depends only on the reviewed path crate and calls `TurnTlsConfig::new` through the public API. Its two tests passed in locked debug and release:

1. `minimal-ed25519-cert.der` is accepted; changing exact offset 52 from `0x0c` to `0x02` returns `Err(InvalidTlsTrust)`.
2. Retagging the printable issuer CN as PrintableString remains accepted, while TeletexString, malformed UTF-8, a ten-character value retagged as countryName, and an unknown X.520 attribute are rejected.

External parser cross-check:

| Input | SHA-256 | RELAY | OpenSSL `x509 -inform DER -noout -subject` |
|---|---|---:|---:|
| checked-in `minimal-ed25519-cert.der` | `fa8447cc84fb2228b47e694430dda11a1519a28932a527eda837e897ee057b70` | accept | accept (`CN=relay.test`) |
| exact offset-52 INTEGER mutation | `912dee9d2a74a597dc1a76e80e0aeacbd7bd1caef4411ae7030fd196dbd94769` | reject | reject |
| exact offset-52 PrintableString positive | local throwaway mutation | accept | accept (`CN=relay.test`) |

## Validation

All repository commands ran from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`.

| Exact command | Result |
|---|---|
| `cargo test --locked -p relay-transport --all-targets --all-features` | PASS — **43/43** (3 unit + 18 fake contract + 10 blocker regressions + 12 T1b) |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | PASS — **43/43** |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `cargo deny check` | PASS — advisories, bans, licenses, and sources; only the existing unmatched BSD-2-Clause/BSD-3-Clause/ISC allowance warnings |
| `cargo test --locked` in `/tmp/relay-t1b-pass-probe` | PASS — 2/2 public probes |
| `cargo test --locked --release` in `/tmp/relay-t1b-pass-probe` | PASS — 2/2 public probes |
| OpenSSL parse of the real certificate and exact offset-52 mutation | positive accepted; mutation rejected |

## Final statement

**Fake Gate0 PASS is granted only because the residual count is 0C / 0H / 0M.** The prior final report's sole M1 is closed, and this focused independent verification found no replacement C/H/M defect.

**Provider selection remains OPEN** and must be decided only by the later native-provider and interoperability evidence.
