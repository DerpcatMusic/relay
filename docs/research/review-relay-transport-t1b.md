# Relay Transport T1b Adversarial Review

**Review mode:** independent, read-only source review; only this report and throwaway files under `/tmp` were written.  
**Reviewed implementation:** `crates/relay-transport/src/lib.rs`  
**Primary focused test:** `crates/relay-transport/tests/t1b_contract.rs`  
**Normative inputs:** `docs/design/transport-interface-synthesis.md`, `docs/research/review-relay-transport-gate0-fix-disposition.md` (accepted T1a boundary), `docs/research/transport-t0-fixtures-rubric.md`, and `docs/plans/2026-08-15-relay-transport-plan.md`.

## Scope and disposition

**T1b disposition: FAIL. Full Gate 0: FAIL.**

Residual findings are **0 critical, 4 high, 2 medium**. The high findings are contract blockers: the fake's documented provider-callback path can violate exact terminal and lifecycle invariants, explicit restart does not create new ICE credentials, the selected lifecycle transition table is not implemented, and construction-time timeout/certificate-policy behavior remains absent. Gate 0 therefore cannot receive a full PASS and no real candidate may be admitted against this contract.

This report does **not** count the absence of candidate adapters or live browser interoperability as a T1b/Gate-0 defect. The transport plan places libdatachannel/Shiguredo/webrtc-rs adapters in T2-T4 and browser interoperability in T5, after Gate 0 (`docs/plans/2026-08-15-relay-transport-plan.md:85-118`). Those later probes must prove that each provider faithfully maps the approved fail-closed trust, callback, teardown, and backpressure rules; they cannot repair a deficient portable contract.

## Findings

### H1 — The “real bounded event path” accepts forged/duplicate terminals and progress after fatal

**Disposition: unresolved / Gate-0 blocker.**

`FakePeer::inject_provider_event` is documented as injecting a provider callback “through the real bounded event path,” but it accepts almost every public `Event` verbatim (`src/lib.rs:1343-1373`). `injected_event_error` performs only a few size/counter checks (`1376-1422`); it does not validate operation correlation, exact-one terminal identity, legal state transitions, or the post-fatal event boundary. After `poll_event` publishes `StateChanged(Failed)` and `FatalError` (`2001-2023`), the injection path still accepts further progress events because it checks `shutdown_complete`, not `provider_failed`. It also accepts a caller-supplied `OperationCompleted` for an operation still queued, after which normal command processing emits the real terminal too.

Throwaway public-API probes demonstrated all of the following in both debug and release:

1. accept `RequestStats(op=1)`, inject `OperationCompleted(op=1)`, then observe a second `OperationCompleted(op=1)` from normal processing;
2. inject `StateChanged(Shutdown)`, observe it, and then successfully accept and complete new work;
3. inject provider loss, observe `Failed` then `FatalError`, inject `StateChanged(Connected)`, and observe post-fatal progress.

This violates the selected exact-one terminal rule, RELAY-owned transition validation, fatal ordering, and the claim that callback adaptation is proven by the fake. The T1b overflow test actually fills the callback queue with five duplicate `StateChanged(New)` events (`tests/t1b_contract.rs:382-395`), so it relies on the invariant bypass rather than testing overflow with legal provider input.

**Required disposition:** accept raw provider callback facts, not already-normalized arbitrary `Event`s, and validate/correlate them before publication; alternatively sharply restrict the fake injection API so operation terminals, lifecycle events, fatal/terminal markers, and post-fatal progress cannot be forged. Add exact duplicate, illegal transition, stale event, and post-fatal/post-shutdown adversarial tests.

### H2 — `RestartIce` changes only the RELAY tag, not the ICE generation

**Disposition: unresolved / Gate-0 blocker.**

Both baseline and restart generation use the same static SDP, candidate text, and `native-base-v1` username fragment (`src/lib.rs:38-41`, `1662-1668`, `1809-1839`). The only changed value is `NegotiationEpoch`. The T1b test asserts only that the emitted description tag is epoch 2 (`tests/t1b_contract.rs:327-368`); it never compares the opaque SDP or candidate username fragment.

A throwaway probe confirmed byte-identical baseline/restart SDP, candidate text, and username fragment. This does not model an ICE restart. T0 deliberately freezes restart carriers whose SDP session/ICE generation changes and warns that an epoch must not be mistaken for a V1 wire identity (`transport-t0-fixtures-rubric.md`, “V1 representability decision” and “Potential corrections”). The selected Gate-0 rubric requires both-sided restart and stale/out-of-order rejection, not merely relabeling the old credentials.

**Required disposition:** the deterministic fake must generate distinct, deterministic ICE credentials/session versions for each epoch and prove both offerer- and answerer-initiated restart flows, while retaining epoch correlation and stale-input rejection.

### H3 — The selected lifecycle transition table is not implemented

**Disposition: unresolved / Gate-0 blocker.**

The selected synthesis requires `New -> Negotiating -> Connecting -> Connected -> Restarting/Connecting -> Closing -> Closed`. `PeerState` has `Connecting` but no `Restarting` (`src/lib.rs:705-725`). `maybe_connect` transitions directly from `Negotiating` to `Connected` (`1757-1778`), and restart transitions back to `Negotiating`, not a restart/connecting phase. `Disconnected` is vocabulary-only; the fake has no validated disconnect/recovery behavior. The T1b helper explicitly expects direct `Connected` (`tests/t1b_contract.rs:69-78`) and the restart test expects `Negotiating` (`334-339`), thereby enshrining the divergence.

The T1a disposition accepted only the vocabulary and explicitly deferred real transition evidence to T1b. That deferral has not been closed.

**Required disposition:** implement and test the approved transition table (or obtain an explicit design amendment), including offerer and answerer restart, connecting, disconnect/recovery, fatal, close, and invalid provider transition rejection.

### H4 — Portable bounded timeout/certificate-policy configuration and hard-timeout proof remain absent

**Disposition: unresolved / Gate-0 blocker.**

The selected synthesis says validated configuration includes certificate policy and timeouts. `PeerConfig` contains queue/message/send/ICE/capability fields only (`src/lib.rs:944-967`), and validation adds no timeout, monotonic deadline/clock, or peer-certificate policy (`998-1065`). `inject_shutdown_timeout` merely flips a Boolean (`1439-1445`); `shutdown` immediately emits `OperationFailed(ShutdownTimeout)`, `StateChanged(Shutdown)`, and `ShutdownComplete` (`1906-1919`). It does not prove that a configured hard bound elapsed or that provider-owned endpoints/workers were force-torn down before the completion marker. The test uses wildcard state assertions and checks the peer drop probe only after the caller manually drops the peer (`tests/t1b_contract.rs:447-470`). Active-driver drop is not tested.

TURN TLS itself is structurally fail-closed: TLS requires `TurnTlsConfig`, verification has no insecure bypass, and platform/custom trust is explicit. This finding does **not** claim an insecure flag exists. It is the still-missing portable timeout/certificate-policy portion and the absence of a deterministic hard-timeout/owner-thread teardown proof.

**Required disposition:** add bounded portable timeout/deadline policy and deterministic fake-clock evidence; make the timeout path prove forced provider-resource teardown before its terminal completion; specify/test active drop. If `TlsTrust` is intended to be the complete selected certificate policy, amend the design to say so and add black-box mapping requirements for later providers.

### M1 — ICE host and custom-DER validation is syntactically incomplete

**Disposition: unresolved.**

`valid_ice_host` rejects only empty/long/non-ASCII values and a small delimiter/whitespace set (`src/lib.rs:437-444`). It accepts embedded NUL and backslash; public probes confirmed both `IceServer::stun("bad\0host", ...)` and `IceServer::stun(r"bad\host", ...)` validate. An embedded NUL is especially unsafe at a future C/C++ FFI boundary because truncation can change the endpoint the provider sees.

`normalize_trust` bounds anchor count/bytes but only rejects empty byte strings (`460-482`). It does not validate that entries are DER certificate/trust-anchor values. The checked-in T1b test treats `[1, 2, 3]` as a valid custom root (`tests/t1b_contract.rs:119-128`). Arbitrary bytes should fail closed in a TLS backend, so this is not evidence of an insecure trust bypass, but it makes the “validated custom DER trust anchors” claim false and postpones a predictable configuration error into provider-specific code.

**Required disposition:** validate host as a precise DNS/IP literal value with no controls or ambiguous FFI characters, and either validate DER at the RELAY boundary or rename the value as bounded opaque certificate bytes and require stable provider-parse failure semantics.

### M2 — T1b’s six tests are too weak to substantiate several implemented claims

**Disposition: unresolved evidence gap; some source behavior independently passed.**

The focused suite does not isolate or prove:

- STUN UDP/TCP and TURN UDP/TCP/TLS positive/negative capability matrices; it tests only missing `custom_tls_trust`;
- required restart/channel/stats capability rejection;
- ICE count/host/credential/anchor exact maxima and redaction through `IceServer`, `PeerConfig`, and `ValidatedPeerConfig` debug output;
- exact allocation identity for rejected `Send` (it clones and compares equality), `Send` under `QueueFull`, separate byte-vs-message `WouldBlock`, nonzero low-water crossing/re-arm, multiple-message FIFO, or capacity-event overflow;
- send/buffer/drain statistics and a second monotonic stats sample;
- multiple ordered inbound messages, inbound event saturation, open idempotence/different-channel rejection, answerer restart, disconnect/recovery, active drop, or exact timeout-state/no-later-event suffix.

Independent throwaway tests did prove that the current source returns the exact `Send` payload allocation for `QueueFull`, `WouldBlock`, and configured-cap rejection and releases an unprocessed send reservation exactly once on fatal. Code inspection also found sound checked byte/message reservation, full-message admission, fatal release, and shutdown clear paths (`src/lib.rs:1869-1904`, `1955-1983`, `2006-2013`). Those are implementation positives, but they do not cure the checked-in evidence gap or H1-H4.

## Contract matrix

| Area | Disposition | Evidence |
|---|---|---|
| STUN/TURN UDP/TCP/TLS vocabulary | Partial PASS | Structured variants and per-transport capability checks exist (`265-271`, `371-434`, `1105-1153`). |
| No insecure TLS trust bypass | PASS at portable API shape | TLS requires config; platform/custom trust only; no disable-verification option (`276-315`, `415-426`). Backend enforcement is later provider-probe evidence. |
| Normalized retained secrets/payloads | PASS | Strings, credentials, payloads, anchors, and config server vector are normalized; allocation unit test covers retained capacities (`2046-2118`). |
| Capability rejection | Source PASS / tests partial | `validate_for` covers configured transports and required features (`1003-1065`), but T1b tests only one capability gap. |
| One reliable ordered channel | Partial PASS | One channel identity/lifetime is enforced (`1842-1867`); no multi-message FIFO/close-with-buffer evidence. |
| Atomic owned send and caps | PASS in source + throwaway probe | Exact owned return for QueueFull/WouldBlock/config cap, checked byte/message reservation, no partial admission. |
| Low-water/no stranded retry/double release | Source appears sound / evidence partial | Validation guarantees a future max-message byte edge and message-slot edge (`1023-1037`, `1466-1496`); fatal release probe passed. Checked-in test covers only threshold 0/message capacity 1. |
| Inbound and event bounds | FAIL overall | Payload/count caps exist, but arbitrary normalized events bypass semantic bounds and terminal/state validation (H1). |
| Stats truth/bounds | Partial PASS | Fake-owned request reports fixed-size counters after ordered processing (`1883-1899`); coverage is narrow and arbitrary injected stats are not normalized. |
| Restart epochs | FAIL | Tags/stale checks exist, but explicit restart reuses old ICE credentials (H2). |
| Exact terminal per accepted op | FAIL through callback path | Normal command path is correlated; callback injection can duplicate terminals (H1). |
| Overflow/fatal ordering | FAIL | Bounded overflow and accepted-command fatal drain exist, but illegal/post-fatal events remain possible (H1). |
| Shutdown terminal boundary | Partial PASS | `ShutdownComplete -> Ready(None)` and post-complete injection rejection work; bounded timeout/drop proof is absent (H4). |
| Hidden panic/unbounded allocation | PASS with caveat | `#![forbid(unsafe_code)]`, all retained public carriers have absolute/configured caps, strict Clippy/doc pass, and no production `unwrap`/panic was found. Internal event batching relies on a `debug_assert` but current legal batches fit the five-event minimum. H1 permits semantically illegal events, not unbounded storage. |
| Dependencies/provider leakage | PASS | Normal dependency tree is empty; dev tree is only `prost` and path `relay-protocol`; no reviewed candidate/runtime identifiers. |

## Adversarial tests

Throwaway crate: `/tmp/relay-t1b-adversarial` (not a repository source edit), path-dependent on the reviewed crate. Final inventory: seven public-API tests.

1. duplicate an accepted operation terminal through the callback path;
2. publish a false `Shutdown` state then continue work;
3. publish progress after `FatalError`;
4. prove restart SDP/candidate/ufrag are unchanged;
5. prove embedded-NUL/backslash ICE hosts validate;
6. prove exact send allocation ownership for QueueFull/WouldBlock/config cap;
7. prove fatal releases an unprocessed reservation exactly once.

Final debug and release runs both passed all seven; “pass” here means the probes successfully demonstrated both defects and positive properties as asserted.

The first exploratory invocation attempted `--locked` before the throwaway lockfile existed, generated the lockfile with an unlocked retry, and then exposed one harness-only expectation error (the probe assumed an extra repeated `Negotiating` event). The probe was corrected to accept the actual event position; the final locked runs below are authoritative.

## Validation commands

All repository commands ran from `/mnt/Windows11/DEV_PROJECTS/Repos/relay`.

| Exact command | Result |
|---|---|
| `cargo fmt -p relay-transport -- --check` | PASS |
| `cargo check --locked -p relay-transport --all-targets --all-features` | PASS |
| `cargo check --locked --release -p relay-transport --all-targets --all-features` | PASS |
| `cargo test --locked -p relay-transport --all-targets --all-features` | PASS — 1 unit + 18 T1a integration + 6 T1b integration |
| `cargo test --locked --release -p relay-transport --all-targets --all-features` | PASS — same 25 tests |
| `cargo clippy --locked -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `cargo clippy --locked --release -p relay-transport --all-targets --all-features -- -D warnings` | PASS |
| `env RUSTDOCFLAGS=-Dwarnings cargo doc --locked -p relay-transport --all-features --no-deps` | PASS |
| `cargo test --locked -p relay-transport --doc` | PASS — 0 doctests |
| `cargo tree --locked -p relay-transport --edges normal` | PASS — root only |
| `cargo tree --locked -p relay-transport --edges dev` | PASS — direct `prost` and path `relay-protocol` only |
| `(cd tests/fixtures/transport && sha256sum --check SHA256SUMS)` | PASS — all 15 frozen fixtures |
| `grep -RniE 'libdatachannel|webrtc[-_ ]?rs|shiguredo|tokio|async_std' crates/relay-transport/src crates/relay-transport/tests/t1b_contract.rs` (success expected only on no matches) | PASS — no candidate/runtime identifiers |
| `(cd /tmp/relay-t1b-adversarial && cargo test --locked)` | PASS — 7/7 |
| `(cd /tmp/relay-t1b-adversarial && cargo test --locked --release)` | PASS — 7/7 |

## Gate 0 disposition

**FAIL.** There are no critical findings, but Gate 0 cannot pass with unresolved high-severity contract defects. The ordinary send reservation/ownership implementation is materially stronger than the focused tests show, TURN TLS has no insecure portable bypass, retained allocations are bounded/normalized, the build is clean in locked debug/release, and the dependency boundary remains excellent. Those positives do not offset H1-H4.

Required re-review entry criteria:

1. close arbitrary callback-event injection and prove exact terminals/legal transition/post-fatal rules;
2. emit deterministic fresh ICE credentials for both restart roles;
3. implement or formally amend the selected lifecycle table;
4. add portable bounded timeout/teardown policy and clarify complete certificate policy;
5. harden ICE host/custom-anchor validation; and
6. expand the checked-in T1b suite across capability, send-edge, stats, inbound, restart, fatal, timeout, and drop matrices.
