# Relay Transport T1b Final Disposition

**Review mode:** independent, read-only review of the current tree. No production source or checked-in test was edited; this report is the only repository deliverable. Final public-API probes live under `/tmp/relay-t1b-final-probe`, and one DER mutation lives under `/tmp/relay-t1b-final-mut`.

**Reviewed implementation:** `crates/relay-transport/src/lib.rs`  
**Focused evidence:** `crates/relay-transport/tests/fake_contract.rs`, `t1b_contract.rs`, and `t1b_blocker_regressions.rs`  
**Prior dispositions:** `docs/research/review-relay-transport-t1b.md` and `docs/research/review-relay-transport-t1b-fix-disposition.md`.

## Scope and disposition

**T1b/fake Gate0 disposition: FAIL.** Residual findings are **0 critical, 0 high, 1 medium**.

The two residual deadline defects are closed: every queued operation retains its own absolute admission deadline, and an operation whose deadline was reached before a later provider-fatal observation terminates as `OperationTimeout`. The supplied malformed-DER fixtures are rejected and the checked-in real Ed25519 certificate is accepted. However, custom trust still accepts an OpenSSL-rejected X.509 mutation whose `commonName` value is an INTEGER rather than a permitted `DirectoryString`. This is the same prior M1 category, so the required zero-C/H/M rule prevents a fake Gate0 PASS.

**Native provider probes and provider selection remain OPEN and separate.** This report neither selects a provider nor treats portable fake approval as live TURN/TLS, browser-interoperability, packaging, or candidate-adapter evidence.

## Finding

### M1 — Custom trust still accepts a DER-shaped non-X.509 Name

**Disposition: unresolved; fake Gate0 blocker under the requested zero-C/H/M rule.**

`valid_name` parses the attribute OID, then accepts the value through `take_any_der_value`; it checks only that the OID is syntactically valid, the value is non-empty, and no fields trail (`src/lib.rs:559-594`, especially `582-589`). It never constrains the value type according to the attribute OID. Consequently a `commonName` (`2.5.4.3`) can carry an INTEGER even though X.509 requires a `DirectoryString` choice.

The independent mutation changes byte offset 52 of the checked-in positive certificate from UTF8String tag `0x0c` to INTEGER tag `0x02`, leaving all DER lengths intact. Its SHA-256 is `912dee9d2a74a597dc1a76e80e0aeacbd7bd1caef4411ae7030fd196dbd94769`.

- `openssl x509 -inform DER -in /tmp/relay-t1b-final-mut/name_value_integer.der -noout -subject` rejects the value (exit 1).
- `TurnTlsConfig::new("turn.example", TlsTrust::Custom(...))` returns `Ok` in both locked debug and release public probes.

This is medium rather than high: the portable API still exposes no trust-all/disable-verification option, and a conforming native TLS backend should fail closed when it parses the anchor. The defect is that the RELAY construction boundary claims DER-encoded trust anchors yet accepts a value a real X.509 parser rejects, deferring a deterministic configuration error into provider-specific behavior.

**Required disposition:** validate `Name` attribute values according to their attribute syntax (at minimum enforce the RFC 5280 `DirectoryString` choice for `commonName`) and expand adversarial comparison against a conforming X.509 parser, or use a vetted bounded X.509 DER parser. Check in this exact mutation as a rejection regression.

## Required residual reproductions

### Queued operation uses its own deadline — PASS

The public probe admits operation 1 at time 0 with a 5 ms deadline, admits operation 2 at time 1 with its own 6 ms deadline, advances once to time 6 without polling, and then observes exactly:

1. `OperationFailed { op: 1, OperationTimeout }`;
2. `OperationFailed { op: 2, OperationTimeout }`; and
3. `Pending`, proving no duplicate terminal.

This passes in locked debug and release. The checked-in tests also cover equal-deadline operations and four operations expired by one clock advance (`t1b_blocker_regressions.rs:313-414`). Source assigns the absolute deadline at admission (`src/lib.rs:2506-2524`) and evaluates the queue front before normal execution (`2536-2551`). Admission deadlines are monotonic because the fake clock is monotonic and the timeout is fixed per peer, so acceptance-order front evaluation cannot hide an earlier deadline behind a later one.

An exploratory first probe incorrectly expected an un-stalled second operation to remain pending before its own deadline after the first operation was removed. That expectation contradicted the contract: unblocked work may execute before its deadline. The final probe instead advances to the second operation's own deadline before polling; the final locked runs above are authoritative.

### Elapsed timeout versus later provider fatal — PASS

The public probe stalls operation 1, advances exactly to its 5 ms deadline, then injects provider loss. It observes exactly:

1. `OperationFailed { op: 1, OperationTimeout }`;
2. `StateChanged(Failed)`;
3. `FatalError(ProviderFailure)`; and
4. `Pending`, with no second operation terminal.

This passes in locked debug and release and matches the checked-in regression (`t1b_blocker_regressions.rs:339-376`). `PendingFatal` retains its observation time (`src/lib.rs:1642-1646`, `1980-1987`); a due operation wins when `deadline_ms <= fatal.observed_at_ms` (`2536-2543`). A fatal observed before the operation deadline still correctly wins, rather than allowing a later poll time to rewrite historical precedence.

### Bad DER rejection and real certificate positive — PARTIAL PASS / M1 remains

The independent debug/release probe confirms:

- the four checked-in `openssl-rejected-*.der` fixtures return `InvalidTlsTrust`;
- obvious non-DER shells exercised by the checked-in regression return `InvalidTlsTrust`;
- `minimal-ed25519-cert.der` returns `Ok` and `openssl x509` parses it successfully; but
- the OpenSSL-rejected `commonName` INTEGER mutation returns `Ok` (M1).

Checked-in fixture hashes observed during review:

| Fixture | SHA-256 | RELAY / OpenSSL |
|---|---|---|
| `minimal-ed25519-cert.der` | `fa8447cc84fb2228b47e694430dda11a1519a28932a527eda837e897ee057b70` | accept / accept |
| `openssl-rejected-empty-oid.der` | `b1ac5ce126655fe84e7a79ea326c895ed3c75bb61ffe3d56c279e53792a10419` | reject / reject |
| `openssl-rejected-indefinite-length.der` | `0257adfff386004cc4ce954caf1336e1e55a000dfda35d6050c888fac9871364` | reject / reject |
| `openssl-rejected-malformed-length.der` | `150b878c171f02d21ef452d2e5ca685c0204b1f340501675401586c47afe7a46` | reject / reject |
| `openssl-rejected-trailing-data.der` | `8c10abb2327f21dfcc34de0c5041ffe747dbba409045b5dcb068583e2869aca0` | reject / reject |

## Re-audit of prior 4H / 2M

| Prior finding | Final disposition | Evidence |
|---|---|---|
| H1 — callback path could forge terminals/lifecycle/post-fatal progress | **CLOSED.** | Injection admits only validated `Message`/truthful `SendCapacity`, routes fatal facts through the fatal scheduler, and rejects normalized terminals/states/stats/negotiation markers (`src/lib.rs:1755-1853`). Exact terminal and post-fatal regressions pass (`t1b_blocker_regressions.rs:71-113`; `t1b_contract.rs:382-477`). |
| H2 — restart changed only the RELAY epoch | **CLOSED.** | Restart emits a distinct deterministic SDP session version and `native-restart-v{epoch}` credential shared by description/candidate/end carriers (`src/lib.rs:2125-2200`). Both roles and stale generation matrices pass. |
| H3 — selected lifecycle absent | **CLOSED.** | Portable states include `Connecting`, `Restarting`, `Disconnected`, `Closing`, `Closed`, and `Failed` (`src/lib.rs:1025-1052`); checked tests prove connecting, both restart roles, disconnect/recovery, fatal, orderly close, and invalid public state injection. |
| H4 — portable timeouts/certificate policy/teardown incomplete | **CLOSED as a high finding.** | Construction bounds both timeout values and requires the sole fail-closed peer-certificate policy (`src/lib.rs:1280-1425`). Own-deadline and fatal precedence now pass; shutdown timeout tears resources down before its terminal, and active drop releases resources. The remaining anchor grammar defect is retained as M1, not H4, because there is still no insecure bypass. |
| M1 — host and custom-DER validation incomplete | **PARTIAL: host CLOSED; DER remains OPEN.** | DNS/IP validation is strict (`src/lib.rs:441-461`), and the supplied DER defects are closed, but the `commonName` INTEGER mutation is accepted (finding above). |
| M2 — checked evidence too weak | **CLOSED as a general evidence finding.** | The suite is now 41 tests per profile: 1 unit + 18 fake-contract + 10 blocker regressions + 12 T1b. It covers the requested deadline/precedence/DER paths in addition to the previously expanded capability, send, stats, inbound, restart, lifecycle, timeout, drop, bounds, redaction, and reserved-ID matrices. |

Thus **all four prior high findings and M2 are closed; prior M1 is not fully closed**.

## Contract preservation

- **Exact terminals:** PASS. Admission remains strictly monotonic with `u64::MAX` reserved for shutdown (`src/lib.rs:2444-2524`). Due commands are removed once and produce one timeout; fatal drain removes each queued non-shutdown command once before `Failed` and `FatalError` (`2536-2575`). Checked and external probes end in `Pending`/`Ready(None)` as applicable and observe no later terminal.
- **Lifecycle/teardown:** PASS. Legal progress remains `New -> Negotiating -> Connecting -> Connected`, with `Restarting`, `Disconnected -> Connecting -> Connected`, terminal fatal, and `Closing -> terminal -> Closed -> ShutdownComplete`. Shutdown tears down before its terminal (`src/lib.rs:2426-2440`); active `Drop` clears provider sends/reservations and marks teardown (`2606-2613`).
- **Backpressure/ownership:** PASS. Pre-admission `QueueFull`, configured-size, and `WouldBlock` paths return the owned command; send byte/message reservations are checked before transfer (`src/lib.rs:2470-2504`). Timeout/fatal paths release a queued send reservation exactly once (`2543-2564`), while successful sends preserve whole-message FIFO. Existing exact-allocation, byte/message limit, low-water re-arm, FIFO, and truthfully normalized stats tests all pass.
- **Bounds:** PASS for queues, message/send budgets, ICE counts/text, timeouts, retained allocations, and event capacity. Strict Clippy/docs and the dependency inspection found no hidden provider/runtime dependency or unsafe boundary. The M1 defect is semantic X.509 validation, not unbounded retention.

## Validation evidence

All repository commands ran from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`.

| Exact command | Result |
|---|---|
| `cargo fmt -p relay-transport -- --check` | PASS |
| `cargo check --locked -p relay-transport --all-targets --all-features` | PASS |
| `cargo check --locked --release -p relay-transport --all-targets --all-features` | PASS |
| `cargo test --locked -p relay-transport --all-targets --all-features` | PASS — **41/41** (1 + 18 + 10 + 12) |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | PASS — **41/41** |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-Dwarnings" cargo doc --locked -p relay-transport --all-features --no-deps` | PASS |
| `cargo test --locked -p relay-transport --doc` | PASS — 0 doctests |
| `cargo deny check` | PASS — advisories/bans/licenses/sources; only the existing unmatched BSD-2-Clause/BSD-3-Clause/ISC allowance warnings |
| `cargo tree --locked -p relay-transport --edges normal` | PASS — root only; no normal dependency |
| `cargo tree --locked -p relay-transport --edges dev` | PASS — direct `prost 0.14.4` and exact path `relay-protocol 0.1.0` only |
| `(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)` | PASS — all 15 frozen fixtures |
| provider/runtime leakage grep over `crates/relay-transport/src` and `tests` | PASS — no libdatachannel, Shiguredo, webrtc-rs, Tokio, or async-std identifiers |
| `cargo test --locked` in `/tmp/relay-t1b-final-probe` | PASS — final 3/3 public probes |
| `cargo test --locked --release` in that probe | PASS — final 3/3 |
| OpenSSL parse over the five checked-in certificate fixtures and mutation | Positive fixture accepted; four supplied negatives and the novel mutation rejected as expected by OpenSSL |

## Final disposition

**Fake Gate0: FAIL (0C / 0H / 1M).** Do not state PASS while M1 remains. The deadline/fatal residuals and all four prior high findings are closed, exact terminal/lifecycle/backpressure/bound guarantees remain intact, and the full locked 41-test/strict-tooling matrix is green. Custom trust validation nevertheless still accepts one reproducible OpenSSL-rejected X.509 structure, so the requested zero-C/H/M threshold is not met.

**Native provider probes and selection: OPEN, independently of this fake disposition.** Even after M1 is fixed, T2-T5 must still prove provider-side trust mapping, real callback normalization, teardown, TURN/TLS, browser interoperability, build/packaging, and candidate selection.
