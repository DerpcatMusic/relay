# Relay Transport T1b Fix Disposition

**Review mode:** independent read-only review of the current tree. No production source or checked-in test was edited; this report is the only repository deliverable. Public adversarial probes live in `/tmp/relay-t1b-fix-disposition-adversarial`.

**Reviewed implementation:** `crates/relay-transport/src/lib.rs`  
**Focused evidence:** `crates/relay-transport/tests/fake_contract.rs`, `t1b_contract.rs`, and `t1b_blocker_regressions.rs`  
**Normative inputs:** `docs/design/transport-interface-synthesis.md`, `docs/research/review-relay-transport-t1b.md`, `docs/research/transport-t0-fixtures-rubric.md`, and `docs/plans/2026-08-15-relay-transport-plan.md`.

## Scope and verdict

**T1b fix disposition: FAIL. Full fake Gate 0: FAIL.**

Residual findings are **0 critical, 1 high, 1 medium**. H1-H3 are closed, the certificate-policy/shutdown-timeout/active-drop portions of H4 are closed, strict host validation is closed, and the expanded checked-in matrices are materially adequate. However, the portable operation deadline is not enforced for every accepted operation and an elapsed timeout can be overwritten by a later fatal error (H4). The custom-anchor validator also accepts a short non-certificate DER shell (M1). The requested rule permits a full fake Gate-0 PASS only with no C/H/M, so Gate 0 cannot pass.

This disposition does **not** select or reject a live provider. Provider selection remains open for T2-T4; the reviewed normal dependency tree is still provider-free.

## Findings

### H4 — Accepted-operation hard deadlines are neither universal nor preserved

**Disposition: unresolved / Gate-0 blocker.**

The new portable configuration and validation are real: `operation_timeout_ms`, `shutdown_timeout_ms`, and the mandatory peer-certificate policy are present (`src/lib.rs:1010-1045`), and zero/over-limit timeouts fail construction (`1121-1127`). Each admitted command receives an absolute `deadline_ms` (`1365-1369`, `2230-2244`). The shutdown fault waits until its deadline, tears provider resources down before its timeout terminal, and then closes (`2280-2288`, `2146-2160`; `t1b_contract.rs:479-520`). Active `Drop` also tears down provider-owned resources (`src/lib.rs:2310-2317`). Those portions of the prior H4 are closed.

The operation deadline itself is still conditional rather than hard. `poll_event` consults `deadline_ms` only when `queued.stalled` is true (or when the separately injected shutdown-timeout flag is true), then processes every non-stalled queued command regardless of whether its deadline has elapsed (`src/lib.rs:2280-2303`). A public probe admitted two operations at time 0, stalled the first, advanced exactly to both five-millisecond deadlines, and polled: operation 1 timed out, but operation 2 emitted `Stats` and later would complete instead of timing out. Thus queue residence behind an accepted stalled operation can exceed the documented “hard deadline for every accepted non-shutdown operation” (`PeerConfig`, `1036-1037`).

Fatal ordering also overwrites an already elapsed timeout. The pending-fatal branch runs before deadline evaluation and fails queued operations with the fatal classification unconditionally (`2256-2279`). A public probe advanced a stalled operation exactly to its deadline, injected provider loss, and received `OperationFailed(ProviderFailure)` rather than `OperationFailed(OperationTimeout)`.

Both probes reproduced in locked debug and release. The checked-in regression covers only one explicitly stalled operation and polls it before any fatal (`t1b_blocker_regressions.rs:217-245`), so it misses both cases.

**Required disposition:** evaluate every front command's absolute deadline independently of the fault flag, including commands delayed behind other accepted work; define and enforce precedence so an already elapsed operation timeout is not replaced by a later provider fatal; add queued-deadline and elapsed-timeout-versus-fatal tests with exact-one-terminal/no-later-terminal assertions.

### M1 — Custom trust accepts DER-shaped bytes that are not an X.509 certificate

**Disposition: unresolved validation defect. Host half closed.**

Host validation is now strict enough for the reviewed boundary: it accepts only `IpAddr` or bounded ASCII DNS labels and rejects controls, backslashes, whitespace, underscores, empty labels, bracketed addresses, and scoped-address text (`src/lib.rs:441-461`). Checked-in and external bad-host matrices passed.

Custom trust is still only superficially parsed. `valid_der_certificate` checks an outer SEQUENCE followed by three tagged values, but treats the TBS portion as valid merely when non-empty and accepts an algorithm whenever `der_value(algorithm, 0x06)` returns anything; it does not validate a `TBSCertificate`, a non-empty/valid OID, algorithm consistency, or complete certificate semantics (`src/lib.rs:463-512`). The value

```text
30 0b 30 01 00 30 02 06 00 03 02 00 00
```

is accepted by public `TurnTlsConfig::new`, although it contains a non-certificate TBS body and an empty OID; `openssl x509 -inform DER` rejects it. The new blocker test rejects only obvious outer-length/tag failures (`t1b_blocker_regressions.rs:267-279`), while both the focused test and allocation unit test deliberately accept similarly tiny “structural” shells (`t1b_contract.rs:125-132`; `src/lib.rs:2375-2388`).

This remains medium rather than high because the portable API still has no insecure verification bypass and a real TLS backend should reject the bytes; the defect is that the RELAY boundary falsely claims validated DER certificate/trust-anchor input and postpones failure.

**Required disposition:** use a complete, bounded X.509 DER parser/validator at this boundary, or rename the carrier as bounded opaque certificate bytes and specify a stable fail-closed provider-parse error. Add adversarial DER with valid outer lengths but invalid TBS/OID/algorithm/signature structure, plus a real certificate fixture.

## Re-audit of prior 4H / 2M findings

### H1 — Injection normalization gate and fatal ordering: CLOSED

`inject_provider_event` now routes `FatalError` through `inject_fatal`/`schedule_fatal`, accepts only semantically checked `Message` and truthful `SendCapacity`, and rejects all already-normalized operation terminals, lifecycle states, shutdown markers, stats, descriptions, and candidates (`src/lib.rs:1478-1577`). Pending/provider-failed and shutdown gates reject later progress (`1483-1487`). The fatal drain produces one failure for each accepted queued command, then `Failed`, then `FatalError` (`2256-2279`); shutdown remains separately available. Overflow is now induced with legal bounded messages (`t1b_contract.rs:383-477`), not duplicate forged states. Checked-in and external duplicate-terminal, false-shutdown, and post-fatal probes passed.

### H2 — Real changed ICE generation, both roles, and stale handling: CLOSED

A restart enters `Restarting`, produces an epoch-specific `native-restart-v{epoch}` username fragment, changes the SDP session version/text, and changes candidate generation/ufrag consistently (`src/lib.rs:1820-1931`, `2050-2079`). Offerer and answerer paths are covered (`t1b_contract.rs:339-379`, `922-1000`), and `fake_contract.rs:984-1198` checks both roles against restart fixtures while rejecting stale descriptions, candidates, and end markers. Same-epoch conflicts remain correlated. The fixture hashes remain unchanged.

### H3 — Full lifecycle: disconnect, recovery, restart, and closing: CLOSED

`PeerState` now includes `Restarting`, `Disconnected`, `Closing`, and `Closed` (`src/lib.rs:754-781`). Completed description installation emits `Connecting` before `Connected` (`1997-2019`); restart emits `Restarting`; recoverable loss permits only `Connected -> Disconnected -> Connecting -> Connected` (`1596-1635`); shutdown emits `Closing -> terminal -> Closed -> ShutdownComplete` (`2146-2160`). Arbitrary public state injection is rejected. Checked-in lifecycle and external transition-skip probes passed.

### H4 — Portable bounded timeouts, certificate policy, and active drop: PARTIAL / HIGH REMAINS

Certificate policy, configuration bounds, deterministic shutdown deadline, teardown-before-timeout-terminal, and active drop are implemented and tested. The per-operation hard deadline is not applied universally and is not preserved against a later fatal; see the finding above.

### M1 — Strict host and DER validation: PARTIAL / MEDIUM REMAINS

DNS/IP validation is closed. X.509 DER validation remains superficial; see the finding above.

### M2 — Expanded matrices: CLOSED as a separate evidence finding

The focused T1b suite grew from 6 to 12 tests and gained 7 blocker regressions, on top of 18 fake-contract tests and 1 allocation unit test. It now covers all STUN/TURN transport capability gaps, required-feature gaps, exact returned allocation pointers for representative pre-admission failures, distinct byte/message backpressure, FIFO and low-water re-arm, two truthful stats samples, ordered inbound messages and legal callback saturation, channel idempotence/rejection, both restart roles, full stale-input matrices, disconnect/recovery, exact shutdown timeout ordering, active drop, count/credential/redaction boundaries, and reserved terminal operation-ID behavior. The two H4 deadline gaps and the M1 DER gap are specific missing adversarial cases, not a remaining general M2 matrix finding.

## Adversarial public probes

Throwaway crate: `/tmp/relay-t1b-fix-disposition-adversarial` (path dependency on the reviewed crate; no repository source edit).

| Probe | Debug / release result | Meaning |
|---|---|---|
| forged terminal, false `Closed`, post-fatal message | PASS | all rejected; accepted op retained exactly one terminal |
| exact single stalled-operation timeout | PASS | pending before deadline, one timeout at deadline, no immediate duplicate |
| public transition skips / observable `Connecting` | PASS | illegal skips rejected; legal path emits intermediate state |
| bad DNS/host matrix | PASS | controls, slash, ambiguous labels, brackets, and scope text rejected |
| non-certificate DER shell | **FAIL as intended** | constructor incorrectly returned `Ok` |
| second queued operation reaches deadline behind stalled first | **FAIL as intended** | overdue operation emitted `Stats` instead of timeout |
| elapsed timeout followed by provider fatal | **FAIL as intended** | terminal error changed from timeout to provider failure |

Locked debug and release produced the same **4 pass / 3 fail** result. “Fail as intended” means the probe exposed a reviewed contract defect.

## Validation commands and results

All repository commands ran from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`.

| Exact command | Result |
|---|---|
| `cargo fmt -p relay-transport -- --check` | PASS |
| `cargo check --locked -p relay-transport --all-targets --all-features` | PASS |
| `cargo check --locked --release -p relay-transport --all-targets --all-features` | PASS |
| `cargo test --locked -p relay-transport --all-targets --all-features` | PASS — 38 tests: 1 unit + 18 fake contract + 7 blocker regressions + 12 T1b |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | PASS — same 38 tests |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-Dwarnings" cargo doc --locked -p relay-transport --all-features --no-deps` | PASS |
| `cargo test --locked -p relay-transport --doc` | PASS — 0 doctests |
| `cargo tree --locked -p relay-transport --edges normal` | PASS — root only |
| `cargo tree --locked -p relay-transport --edges dev` | PASS — direct `prost` and path `relay-protocol` only |
| `cargo deny check` | PASS — advisories/bans/licenses/sources all OK; unmatched license allowances are warnings |
| `(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)` | PASS — all 15 frozen fixtures |
| candidate/runtime leakage grep over reviewed source and T1b tests | PASS — no matches |
| `cargo test --manifest-path /tmp/relay-t1b-fix-disposition-adversarial/Cargo.toml --locked` | 4 PASS / 3 defect-exposing FAIL |
| same command with `--release` | 4 PASS / 3 defect-exposing FAIL |

## Gate 0 disposition

**FAIL.** The fake is substantially improved and closes H1-H3, strict host handling, certificate-policy shape, deterministic shutdown teardown, active drop, and the broad M2 evidence gap. It still cannot receive full fake Gate-0 approval with an unresolved high deadline guarantee and a medium DER-validation defect.

Re-review entry criteria:

1. enforce every accepted operation's absolute deadline, including time spent queued behind other work;
2. preserve an already elapsed timeout against later fatal injection and prove exact-one terminal behavior;
3. reject DER-shaped non-certificates (or explicitly redefine the carrier and provider-parse contract); and
4. add the three failing public probes to checked-in regression coverage.

Live provider selection remains open and must not proceed as though this report selected a candidate.
