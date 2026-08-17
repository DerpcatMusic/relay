# Transport candidate comparison

**Scope.** Independent normalization of the three checked-in dossiers only:
[`libdatachannel`](transport-candidate-libdatachannel.md),
[`webrtc-rs`](transport-candidate-webrtc-rs.md), and
[`Shiguredo webrtc-rs`](transport-candidate-shiguredo.md). No new upstream or web
research was performed. This is a probe-planning comparison, not a selection.

## Reading the labels

- **PASS** — the pinned dossier directly establishes the stated paper/API/artifact fact. It does **not** mean that a T0 hard gate has passed.
- **EVIDENCE GAP** — the surface or claim is plausible/documented, but the checked-in evidence lacks a reproducible RELAY build or live result.
- **HARD BLOCKER** — selection is prohibited until the named condition is cleared by reproducible evidence (or a narrow, pre-scoring architecture-owner exception allowed by the T0 rubric).

Every candidate is currently ineligible for weighted comparison. All three remain
under consideration, but can become eligible only after reproducible builds and
live probes fill candidate-specific copies of the T0 manifests. A paper **PASS**
never changes `decision.eligibleForWeightedComparison` from `false`.

## Normalized dossier comparison

| Criterion | libdatachannel `v0.24.5` | webrtc-rs `v0.20.2` | Shiguredo `0.151.0` |
|---|---|---|---|
| **Pinned identity and artifact** | **PASS (identity):** tag object `61204eb…`, commit `443f693…`; source-only release, observed generated tarball hash is not a signed upstream asset. Five direct gitlinks are pinned, but OpenSSL/libnice/system closure is not. **EVIDENCE GAP:** no RELAY-built artifact or lock record. [Dossier §§ Artifact, gitlinks, Packaging](transport-candidate-libdatachannel.md#artifact-revision-and-maintenance-status) | **PASS (identity):** user API is `webrtc` `0.20.2` at `38e02d8…`, with `rtc` `0.20.2`/gitlink `efad79d…`; source distribution, exact crate pin plus committed consumer lockfile required. **EVIDENCE GAP:** upstream has no root lockfile or RELAY-built artifact. [Dossier §§1–3, 9](transport-candidate-webrtc-rs.md#1-identity-webrtc-not-rtc) | **PASS (identity):** annotated unsigned tag `9720e08…` peels to `7052a80…`; crate/native `0.151.0`, builder `m151.7922.0.0`/`0d51a21…`, libwebrtc `f20ebb8…`; target asset SHA-256 values recorded. **EVIDENCE GAP:** no signed provenance/SBOM or bit-reproducibility claim. [Dossier §§ Exact identity, Artifact construction](transport-candidate-shiguredo.md#exact-identity-at-the-cutoff) |
| **Required targets and packaging** | **EVIDENCE GAP:** claims Windows/macOS/Linux support and CI covers three OS families, but Windows x86-64, macOS arm64, macOS x86-64, and Linux x86-64 have not all been reproduced in RELAY; native/system dependencies must be frozen per image. | **EVIDENCE GAP:** upstream CI uses moving Windows/macOS/Linux images and has no exact architecture matrix or MSRV contract (edition floor only); `ring` adds native C/assembly. Four RELAY target lanes remain unbuilt. | **HARD BLOCKER (current artifact matrix):** Windows x86-64, macOS arm64, and Linux x86-64 assets exist, but no macOS x86-64/universal asset exists. Forced target names do not prove ABI support. Clean default builds are network-dependent; offline vendoring must be proven. |
| **Runtime / ownership** | **EVIDENCE GAP:** C/C++ library with C API but no first-party Rust binding; global worker pool/SCTP settings/cleanup, backend threads, and possible detached resolver thread. Callbacks arrive on library threads. | **PASS (model):** async `webrtc` injects a runtime; Tokio default uses ambient spawning and one long-lived driver task per peer. Dedicated reactor mode creates a first-config-wins process-global thread pool. **EVIDENCE GAP:** explicit close/join and host-runtime coexistence remain unproven. | **PASS (model):** caller owns started network/worker/signaling threads; they must outlive factory/context/peers and be explicitly stopped/joined. Proxied objects are `Send + Sync`. **HARD BLOCKER:** callback thread, overlap/reentrancy, panic containment, and native lifetime ordering are unproven across FFI. |
| **Offer / answer / glare** | **PASS (surface):** offer/answer/trickle plus `pranswer`/`rollback`; manual negotiation is available and preferable. **EVIDENCE GAP:** deterministic polite/impolite glare recovery has no pinned browser result. | **PASS (surface):** normal create/set offer/answer and separate trickle API. **HARD BLOCKER for renegotiation:** rollback state transitions and deterministic glare policy are not proven; may not block a deliberately one-shot session. | **PASS (surface):** async offer/answer/set-description and public rollback representation. **HARD BLOCKER:** no test proves empty-SDP rollback/state recovery or deterministic glare handling. |
| **End-of-candidates and restart** | **PASS (local):** gathering complete can map to one V1 empty-candidate end marker. **HARD BLOCKER:** no remote end call; no-op policy is unproven. Default libjuice rejects changed ICE credentials and same-peer restart; replacement counts only if predeclared and proven against both frozen T0 restart directions, continuity, epoch, and deadline rules. | **HARD BLOCKER:** exact local completion and remote empty/`None` mapping remain unverified. Both restart directions must prove new credential generations, rejection/delay of stale candidates, per-generation end markers, and selected-pair migration; “another offer” is insufficient. | **PASS (request surface):** ICE-restart offer option exists. **HARD BLOCKER:** no nullable remote-end API; local-complete mapping/no-op remote handling, credential rotation, restart continuity, and both directions remain live-probe obligations. |
| **TURN UDP / TCP / TLS and certificate policy** | **PASS (UDP surface):** relay policy and TURN/UDP work through default libjuice. **HARD BLOCKER:** libjuice rejects TURN control over TCP/TLS; libnice is required for those lanes, while the wrapper exposes no CA/hostname/pin policy. Valid host plus wrong-host, expired, private/untrusted CA cases must prove fail-closed behavior. | **PASS (UDP-only surface):** credentials and relay-only policy reach the pinned relayer. **HARD BLOCKER (source-established):** v0.20.2 explicitly skips secure TURN and every non-UDP TURN URL, so required TURN/TCP and TURN/TLS—including certificate/hostname verification—are absent, not merely untested. A later immutable version/patch identity must expose and pass those lanes before eligibility. | **PASS (surface):** TURN URL/auth, relay policy, `Secure`/`InsecureNoCheck`, and certificate callback surfaces exist. **HARD BLOCKER:** no pinned TCP/TLS connection evidence; SNI, roots, hostname checking, failure codes, refresh, and rejection are unknown. Shipping must use `Secure`; bypass is negative-fixture-only. |
| **Reliable ordered data and bounded backpressure** | **PASS (data semantics):** default channel is reliable/ordered; buffered amount, low threshold, and strict crossing callback exist. **HARD BLOCKER:** native SCTP send queue is unlimited and `send=false` means buffered, not rejected. A serialized adapter must prevent native admission above its byte budget and must not resend. | **PASS (source surface):** reliable/ordered defaults and opt-in bounded sending expose `outstanding_bytes`, low/high poll events, and an exact-fit `try_send`; ordinary `send` may soft-overshoot and rejection consumes its buffer. **HARD BLOCKER until adapter proof:** RELAY must use the exact-fit path behind one owner, preserve retry ownership, and prove no overshoot/lost wakeup/unbounded waiter behavior with a stalled reader. | **HARD BLOCKER (surface):** wrapper exposes neither buffered amount nor low threshold/event. Boolean send alone cannot implement byte-bounded event-driven admission. A maintained/pinned wrapper surface change or eligible release is required before this lane can run. |
| **Stats** | **EVIDENCE GAP:** C++ exposes selected pair/address, SCTP bytes and RTT; C API omits byte/RTT methods and there is no standardized report or many required fields. Adapter-owned observations and unavailable-field markers are required. | **PASS (surface):** async `get_stats` aggregates peer/ICE/DTLS/SCTP/data-channel reports. **EVIDENCE GAP:** populated fields, identifiers, and semantics per browser/TURN/restart state are unverified. | **PASS (snapshot surface):** report-to-JSON callback exists. **HARD BLOCKER:** required JSON keys/units are unknown and an outstanding callback has no visible cancel/destruction path, so stats-vs-close allocation reclamation must be proven. |
| **Teardown / quiescence** | **HARD BLOCKER:** peer teardown is asynchronous/process-global; Closed callbacks have different dispatch behavior; no per-peer quiescence token or bounded deadline. Must prove callback gating, off-callback global cleanup, and no UAF/deadlock/leak. | **HARD BLOCKER:** `close().await` and driver abort are not proof that every callback/task is quiescent; drop can detach. Explicitly owned Relay pumps/runtime ordering and 10,000-cycle state coverage are required. | **HARD BLOCKER:** observer/data-channel/stats destruction timing and synchronous quiescence are unknown. Required order is gate → unregister/close → settle → drop peer/factory → stop/join three threads, with no panic across FFI or post-gate entry. |
| **Browser evidence** | **EVIDENCE GAP:** upstream claims Firefox/Chromium/Safari and ships a browser example, but has no pinned-browser/coturn/adverse-network CI; example omits explicit end and restart. | **EVIDENCE GAP:** standards intent/API and examples exist, but no RELAY evidence for pinned Chromium, Firefox, Safari, coturn routes, glare/end/restart, or close. | **EVIDENCE GAP:** WHIP/WHEP examples and multi-OS native CI do not test native↔browser data channels; no pinned Chrome/Firefox/Safari/coturn job exists. |
| **Licensing and integrity** | **PASS (declared):** MPL-2.0 candidate/libjuice plus BSD/MIT dependencies; covered-source and notices apply. **EVIDENCE GAP:** no attached signed artifact; system crypto/libnice versions, hashes, licenses, source offer, SBOM, and final notice bundle depend on the chosen build image. | **PASS (declared):** `webrtc`/`rtc` dual MIT/Apache-2.0. **EVIDENCE GAP:** final locked transitive SBOM/notices are absent; `ring` brings Apache-2.0 AND ISC/BoringSSL/other notice material. | **PASS (declared):** wrapper Apache-2.0 and libwebrtc BSD terms; final asset sidecars recorded. **HARD BLOCKER:** sidecar and asset share a mutable `--clobber` release channel, upstream builder input lacks a published digest, and wrapper artifacts omit the comprehensive target `NOTICE`; independent hash lock and complete notice/SBOM recovery are mandatory. |

## Common prerequisite and evidence discipline

**T1b Gate 0 is a prerequisite, not candidate work.** The repository records Phase-2
as “Gate 0 T1b in progress.” No candidate-specific adapter, build, or live probe
may begin until T1b completes the provider-neutral contract
for send/backpressure, TURN/TLS configuration, stats, failure injection,
dropped-driver/teardown behavior, and the architecture owner approves Gate 0.

After Gate 0:

1. Reconfirm all 15 files under `tests/fixtures/transport/v1` against
   `SHA256SUMS`; preserve canonical empty-candidate end markers and both changed-
   `ufrag` restart directions. Any schema/generated-tree change, checksum drift,
   provider import into T0, or reinterpretation of `PeerUpdate(LEFT)` is **STOP**.
2. Copy, never edit, `environment-manifest-v1.template.json` and
   `scorecard-v1.template.json` for each candidate, run, target, and materially
   different backend/profile. Empty strings and `null` mean incomplete evidence.
3. Complete immutable source/submodule/features/native-dependency identity,
   build image/toolchains, all four frozen target entries, exact Chromium,
   Firefox and manual Safari identities, isolated coturn configuration/TLS chain,
   impairment profiles/seeds, one-attempt-default retry policy, UTC bounds, and
   every required observation. Public STUN/TURN is **STOP**.
4. Keep every scorecard gate `not_run`, every rating `null`, total `null`, and
   eligibility `false` until raw evidence exists. Open weighted scores only after
   `adapter_fit`, `browser_interop`, `relay_security`, `recovery_lifecycle`,
   `licensing`, `packaging`, and `maintenance` all pass (or have a pre-approved,
   unexpired narrow exception). A weighted total cannot override a failed gate.

## Risk-adjusted probe order (scheduling, not ranking)

The order minimizes sunk integration work and exercises the shared harness early;
it is not a preference or winner declaration.

1. **Cheap preflight on all three.** Freeze candidate-specific environment
   manifests and resolve only known admission blockers: Shiguredo must identify
   an eligible byte-accurate buffered-low surface and a macOS x86-64 strategy,
   libdatachannel must predeclare the libnice profile and whether peer replacement
   can satisfy the frozen restart contract, and webrtc-rs must lock the complete
   `0.20.2` graph with exactly one runtime. **STOP** a candidate lane on a blank
   manifest field, mutable dependency/artifact, unavailable required target, or
   unapproved semantic workaround; record a reproducible blocker, not a score.
2. **First shared harness shakeout: webrtc-rs, negative-gate first.** Its
   source/Cargo shape and backpressure/stats surfaces make it the lowest-cost harness
   validation, but pinned v0.20.2 must record source-established TURN/TCP/TLS hard
   failures and stop before the expensive live matrix. It may advance only under a
   separately pinned upgrade/patch identity that restores those mandatory routes;
   this ordering is not evidence of superiority.
3. **Second: libdatachannel, as separate JUICE-MIN and NICE-MIN identities.** Run
   restart/end-marker kill cases before the full browser/impairment matrix; only
   NICE-MIN can attempt TURN/TCP/TLS. Do not merge results across binaries or
   relabel peer replacement as same-peer restart.
4. **Third: Shiguredo.** Proceed beyond artifact/offline/NOTICE checks only after
   its missing buffered-low surface and required macOS target are resolved under
   an immutable eligible identity; this avoids paying the largest static-artifact
   and FFI lifecycle cost before known admission blockers clear.

## Exact stop/go criteria

### GO from reproducible build to live probes

A candidate advances only when its copied environment manifest is complete and
names the exact dossier identity, lock/hashes, feature/backend profile, linked
native closure, toolchains/image, and frozen target; clean build/link/smoke results
are reproducible for Windows x86-64, macOS arm64, macOS x86-64, and Linux x86-64
(or the architecture owner has approved a narrow exception before scoring).
Licenses/notices and binary hashes/sizes must already be archived. Shiguredo must
also prove clean offline consumption from independently locked assets.

### STOP during live probes

Stop that candidate's expensive matrix, preserve raw evidence, and mark the
relevant hard gate `fail` if any required condition is reproducibly false:

- either frozen offer/answer role, canonical end marker, glare policy, or either
  restart direction cannot preserve V1 fixture meaning and per-generation identity;
- selected pairs fail to prove forced relay on UDP, TCP, or TLS, or valid-host TLS
  fails / wrong-host, expired, or untrusted TLS succeeds;
- reliable ordered payload hashes show loss/duplicate/reorder, native/application
  admission exceeds the declared byte/message cap, memory/tasks are unbounded, or
  low-water recovery loses a wakeup;
- required state/pair/protocol/message/error/resource observations cannot be
  emitted honestly (missing fields must be recorded, never synthesized);
- close from any partial/connected/congested state exceeds its declared deadline,
  enters a retired callback gate, leaks tasks/threads/FDs/RSS, panics across FFI,
  deadlocks, crashes, or triggers sanitizer findings;
- the four-target package, license/source-offer/notice/SBOM, or immutable integrity
  record cannot be shipped under the declared candidate identity.

### GO to normalized weighted comparison

Only identical pinned browser/coturn/network fixtures, first-attempt reporting, and
complete evidence paths may be compared. Each of the seven hard gates must be
`pass` (or carry the allowed pre-scoring exception); every 0–5 rating must cite raw
measurements and confidence for the frozen 25/20/15/15/10/10/5 dimensions. Until
then each candidate remains `not_evaluated`, with no winner selected here.
