# RELAY Control-Plane and Signaling Execution Plan

**Date:** 2026-08-15  
**Status:** Draft for validation  
**Scope:** Cloudflare Worker control plane, one Durable Object per session, authenticated WebSocket signaling, reconnect/resume, D1 persistence boundary, provider seams, security, observability, tests, and delivery gates. No media bytes traverse this control plane.

## Outcome

Ship an executable, testable signaling plane in which an edge Worker validates external ingress and account/session admission material before routing to the authoritative `SessionDO`; the object atomically consumes join credentials or validates/rotates resume credentials before accepting a `Hello`, binds the socket to the authorized peer, serializes membership and signaling revisions, and retains a bounded replay window. D1 stores durable product/account metadata but is not the live-session coordinator, and provider-specific logic stays behind narrow adapters.

## Non-goals

- Media relay, TURN data-plane implementation, or codec negotiation policy.
- UI implementation.
- Durable Object acting as an identity provider or secret vault.
- D1 polling or cross-request in-memory state as a coordination mechanism.
- Exactly-once delivery across disconnects; the protocol provides ordered, replayable-at-least-once delivery with client deduplication.

## Proposed boundaries

1. **Worker (stateless edge gateway):** validate method/origin/size, authenticate account credentials when present, resolve or mint narrow session admission material, apply coarse abuse controls, derive the stable session-object name, and forward the upgrade through the Durable Object binding. An anonymous product flow still receives an authenticated, short-lived peer/join credential; it simply has no account principal.
2. **SessionDO (single-session authority):** own live membership, peer roles, current revision, bounded replay/idempotency state, signaling fan-out, resume-token rotation, expiry, and session closure. Persist recovery-critical session state in its private SQLite-backed Durable Object storage. A newly upgraded socket is pending until the object validates `Hello` admission; it cannot signal or observe membership before binding.
3. **D1 (durable business metadata):** accounts, session catalog/lifecycle summaries, grants/invitations, audit summaries, and provider configuration references. D1 is never consulted for each signaling frame and never determines frame order. Live session state is object-authoritative; catalog/account state is D1-authoritative.
4. **Providers:** identity verification, TURN credential minting, fan-out routing, and future notifications sit behind injected interfaces. Provider responses are normalized before entering the core.
5. **Clients:** use the existing Protobuf `relay.v1.Envelope`, retain the last fully applied server revision and rotated resume token, submit unique `message_id` values, apply replay idempotently by revision, detect gaps, and reconnect with `ResumeRequest`.

## Protocol sketch to freeze before implementation

### Upgrade

`GET /v1/sessions/{sessionId}/socket` with a WebSocket upgrade and an authenticated principal. The Worker resolves an authorization snapshot and forwards only trusted internal headers/arguments to `SessionDO` (principal ID, role/capabilities, authorization epoch/expiry, trace ID). Client-supplied identity headers are removed.

### Client envelope

```json
{"v":1,"type":"signal.send","commandId":"uuid","expectedRevision":41,"payload":{}}
```

### Server envelope

```json
{"v":1,"type":"signal.received","revision":42,"eventId":"sessionId:42","payload":{}}
```

### Resume

Client reconnects with `lastAppliedRevision`. The object either:

- accepts and replays events `(lastAppliedRevision, currentRevision]`, followed by `resume.complete`; or
- returns `resync.required` when the cursor is ahead, too old, invalid for the authorization epoch, or outside the retained log.

Replayed and live events share the same monotonically increasing per-session revision space. A command retry with the same `(principalId, commandId)` returns/replays the prior result and does not allocate a second revision. Revisions are allocated only for committed, externally visible session events.

## Tiny-task execution sequence

Each checkbox is intended to be independently reviewable and leave the tree buildable.

### Phase 0 — Decision records and contracts

- [ ] **CP-001:** Add a control-plane context diagram showing client → Worker → SessionDO and Worker → D1/provider calls; explicitly draw media traffic outside it.
- [ ] **CP-002:** Record the invariant “one canonical Durable Object ID/name per session ID.”
- [ ] **CP-003:** Record the invariant “only SessionDO allocates session event revisions.”
- [ ] **CP-004:** Record delivery semantics: ordered per session, replayable at least once, client-deduplicated by `eventId`.
- [ ] **CP-005:** Define canonical opaque IDs and reject alternate textual forms before object lookup.
- [ ] **CP-006:** Publish protocol-v1 JSON Schemas for upgrade metadata, client commands, server events, errors, replay completion, and resync-required.
- [ ] **CP-007:** Define hard limits: upgrade URL/header size, frame bytes, decoded payload bytes, replay count/bytes, members/session, sockets/principal, command rate, and idle lifetime.
- [ ] **CP-008:** Define close codes and stable public error codes without leaking authorization or provider details.

**Gate G0:** Architecture review approves boundaries, ordering/delivery semantics, schemas, and limits.

### Phase 1 — Worker ingress shell

- [ ] **CP-010:** Add a single route table for health, session creation/read, grants, provider credentials, and the socket upgrade.
- [ ] **CP-011:** Reject unsupported methods and malformed content types before reading bodies.
- [ ] **CP-012:** Enforce request/body/header limits and a strict WebSocket `Upgrade` check.
- [ ] **CP-013:** Add an exact production-origin allowlist; define explicit development origins without reflecting arbitrary origins.
- [ ] **CP-014:** Generate or validate a bounded trace/correlation ID; never accept it as an authorization fact.
- [ ] **CP-015:** Normalize external errors at the Worker boundary and attach safe cache/security headers.
- [ ] **CP-016:** Bind environment resources through typed `Env`; fail closed when a required binding or secret is absent.
- [ ] **CP-017:** Derive `idFromName(canonicalSessionId)` in one helper and cover it with vector tests.
- [ ] **CP-018:** Forward socket upgrades to the object without buffering or creating a second WebSocket pair in the Worker.

**Gate G1:** Worker unit tests prove malformed/oversize/origin-invalid/unauthenticated requests cannot reach a SessionDO stub.

### Phase 2 — Authentication and authorization binding

- [ ] **CP-020:** Define an `Authenticator` interface returning a normalized principal, credential ID, authentication time, and expiry.
- [ ] **CP-021:** Verify issuer, audience, signature, expiry/not-before, and allowed algorithm for bearer credentials; reject query-string bearer tokens.
- [ ] **CP-022:** Define an `Authorizer` interface returning session-scoped capabilities and an authorization epoch/expiry.
- [ ] **CP-023:** Query D1 once at connection admission for membership/grant state; do not pass raw tokens into SessionDO.
- [ ] **CP-024:** Strip all external `x-relay-*` identity headers and construct trusted internal admission data in code.
- [ ] **CP-025:** Bind every accepted socket to immutable `{sessionId, principalId, capabilities, authEpoch, expiresAt}` attachment/state.
- [ ] **CP-026:** Re-check capability on every command; never trust a client-declared sender, role, target session, or revision owner.
- [ ] **CP-027:** Define revocation propagation: increment authorization epoch/close active sockets through the session object, with a bounded maximum credential lifetime as backstop.
- [ ] **CP-028:** Close expired connections and require fresh authentication rather than silently extending credentials.
- [ ] **CP-029:** Make not-found/forbidden behavior indistinguishable where session enumeration is a risk.

**Gate G2:** Adversarial tests cover forged headers, wrong audience, expired token, revoked grant, cross-session access, role escalation, origin mismatch, and expiry during a connection.

### Phase 3 — SessionDO state machine and WebSockets

- [ ] **CP-030:** Define explicit states: `active`, `closing`, `closed`; enumerate legal transitions.
- [ ] **CP-031:** Define persisted recovery header: schema version, session revision, lifecycle state, auth epoch, replay-window bounds, and expiry.
- [ ] **CP-032:** Initialize and migrate object storage inside a concurrency-safe initialization barrier.
- [ ] **CP-033:** Accept sockets through the Durable Objects WebSocket API and persist only minimal serializable attachment metadata needed after hibernation.
- [ ] **CP-034:** Validate subprotocol/version before acceptance and return a clean rejection for unsupported protocol versions.
- [ ] **CP-035:** Centralize message parsing; reject binary/unexpected/oversize/invalid-schema frames before command dispatch.
- [ ] **CP-036:** Implement command dispatch as a total allowlist keyed by protocol version and command type.
- [ ] **CP-037:** Verify socket attachment identity/capabilities and command ID on every frame.
- [ ] **CP-038:** Commit state mutation, revision allocation, replay record, and idempotency result atomically in object storage before fan-out.
- [ ] **CP-039:** Broadcast from the committed event; isolate and close failed/slow recipients without rolling back the commit.
- [ ] **CP-040:** Enforce member/socket/rate/backpressure limits with deterministic close/error behavior.
- [ ] **CP-041:** Use alarms for session expiry/cleanup; make alarm handlers idempotent and re-schedule when work remains.
- [ ] **CP-042:** Handle close/error callbacks by releasing ephemeral presence without fabricating a durable leave event unless protocol policy requires one.
- [ ] **CP-043:** Ensure hibernation/restart rebuilds ephemeral indexes from socket attachments plus stored recovery state.
- [ ] **CP-044:** Add schema-versioned migrations and reject/alert on unknown future state versions.

**Gate G3:** State-machine tests prove serialized revisions, atomic recovery after injected failures, safe hibernation/restart, bounded resources, and idempotent alarms.

### Phase 4 — Resume, replay, and revision protocol

- [ ] **CP-050:** Specify revision as an unsigned safe JSON integer or decimal string and freeze overflow behavior.
- [ ] **CP-051:** Require `commandId` for mutating commands and define its format/maximum length.
- [ ] **CP-052:** Store a bounded `(principalId, commandId) → outcome/revision` idempotency index with the replay window.
- [ ] **CP-053:** Store schema-versioned replay entries by revision and prune by both count/bytes and age.
- [ ] **CP-054:** Make resume admission capture a consistent committed revision and replay-window lower bound.
- [ ] **CP-055:** Reject cursors greater than current revision and cursors below the retained lower bound with `resync.required`.
- [ ] **CP-056:** Replay authorized events in ascending revision order, filtering payload fields by the reconnecting principal’s current capabilities.
- [ ] **CP-057:** Queue or sequence live delivery behind replay so the client never observes revision `N+1` before replayed `N`.
- [ ] **CP-058:** Send `resume.complete` with the captured/live boundary; specify client handling when events arrive immediately afterward.
- [ ] **CP-059:** Specify client deduplication by `eventId` and gap detection; any unexplained gap triggers resync.
- [ ] **CP-060:** Specify full-resync snapshot semantics, revision watermark, authorization filtering, and atomic client replacement.
- [ ] **CP-061:** Test duplicate commands before/after reconnect and ensure one externally visible event/revision.
- [ ] **CP-062:** Test reconnect races, replay pruning, stale authorization epoch, object restart during resume, and snapshot-to-live handoff.

**Gate G4:** A deterministic model/property test finds no duplicate state transition, revision regression, unauthorized replay, or replay/live reordering under generated disconnect/retry schedules.

### Phase 5 — D1 boundary and durable workflows

- [ ] **CP-070:** Write the D1 schema for accounts, session catalog/lifecycle summary, grants/invites, provider references, and append-only audit summaries.
- [ ] **CP-071:** Add D1 migrations with forward-only IDs and local/preview migration tests.
- [ ] **CP-072:** Keep live membership, presence, replay records, and next revision out of D1.
- [ ] **CP-073:** Define a session-creation saga: create authoritative catalog row, derive object ID, initialize object idempotently, and compensate/mark failed on partial error.
- [ ] **CP-074:** Define closure propagation from SessionDO to D1 as an idempotent outbox/task; D1 is eventually consistent and does not block signaling.
- [ ] **CP-075:** Use explicit column lists and parameterized statements; enforce tenant/principal predicates in repository methods.
- [ ] **CP-076:** Make retries idempotent with unique operation keys and conditional state transitions.
- [ ] **CP-077:** Define retention/deletion workflows spanning D1, SessionDO storage, provider resources, and audit tombstones.
- [ ] **CP-078:** Document consistency shown to APIs: object-authoritative live state versus D1-authoritative catalog/account state.

**Gate G5:** Migration, retry, partial-failure, deletion, and stale-read tests demonstrate the written consistency contract.

### Phase 6 — Provider seams

- [ ] **CP-080:** Define narrow ports for `IdentityVerifier`, `RelayCredentialIssuer`, `Clock`, and `AuditSink`.
- [ ] **CP-081:** Keep provider SDK types/errors out of protocol, domain, D1 repository, and SessionDO state.
- [ ] **CP-082:** Normalize provider timeouts, retryability, expiry, quotas, and public error mapping.
- [ ] **CP-083:** Mint least-privilege, short-lived TURN/relay credentials only after session authorization; never broadcast them through signaling events.
- [ ] **CP-084:** Bound provider calls with timeouts and explicit retry budgets; never call a provider synchronously from a SessionDO frame broadcast path.
- [ ] **CP-085:** Add deterministic fakes and provider contract tests including malformed, slow, denied, and rate-limited responses.
- [ ] **CP-086:** Define provider failover policy without changing the v1 client envelope.

**Gate G6:** Core protocol/state tests run entirely with fakes; provider contract suites prove normalization and secret isolation.

### Phase 7 — Security, privacy, and redaction

- [ ] **CP-090:** Create a threat model covering token theft, cross-session confused deputy, replay, enumeration, signaling injection, resource exhaustion, origin abuse, and log leakage.
- [ ] **CP-091:** Classify fields as public, internal, personal, credential, or secret and derive a logging allowlist.
- [ ] **CP-092:** Implement structural redaction before serialization; never log Authorization/Cookie, full SDP, ICE candidates, provider credentials, socket URLs, or raw frames.
- [ ] **CP-093:** Log stable IDs only when needed; hash/pseudonymize principal and IP identifiers with a rotating keyed scheme.
- [ ] **CP-094:** Validate all protocol payloads and constrain nested depth, string lengths, candidate counts, and target membership.
- [ ] **CP-095:** Apply layered abuse limits: edge/admission, per principal, per session object, per socket, and provider issuance.
- [ ] **CP-096:** Define CSP/CORS/origin policy separately; do not treat CORS as WebSocket authentication.
- [ ] **CP-097:** Store secrets only in secret bindings/provider secret stores; document rotation and break-glass procedures.
- [ ] **CP-098:** Add dependency/config scanning and a test that serialized logs contain no seeded canary secrets.
- [ ] **CP-099:** Define data retention, deletion, and audit access controls.

**Gate G7:** Security review and automated negative tests close all high-severity threat-model paths; canary-secret redaction test passes.

### Phase 8 — Observability and operations

- [ ] **CP-100:** Define low-cardinality structured log events for admission, rejection class, connect/disconnect, command outcome, resume outcome, alarm, provider call, and internal failure.
- [ ] **CP-101:** Propagate a trace ID Worker → SessionDO → async D1/provider work without using user payload as labels.
- [ ] **CP-102:** Define metrics: active sockets/sessions, admission outcomes, command latency/errors, close-code counts, replay size/latency, resync rate, storage failures, alarm lag, provider latency/errors, and redaction drops.
- [ ] **CP-103:** Avoid session/principal/command IDs as metric dimensions; retain them only in access-controlled sampled logs/traces.
- [ ] **CP-104:** Add health/readiness checks that validate code path/config without exposing bindings, object contents, or provider secrets.
- [ ] **CP-105:** Define SLOs and alerts for successful admission, signaling command latency, unexpected disconnects, resume success, and provider issuance.
- [ ] **CP-106:** Write runbooks for elevated resyncs, object/storage failures, provider outage, abusive session, auth compromise, and schema rollback/forward-fix.
- [ ] **CP-107:** Add deployment version/schema/protocol version to safe logs and diagnostics.

**Gate G8:** Staging fault injection produces actionable alerts/traces and runbooks identify the failing boundary without inspecting sensitive payloads.

### Phase 9 — Test matrix and release gates

- [ ] **CP-110:** Add pure schema/envelope/idempotency/revision state-machine unit tests.
- [ ] **CP-111:** Add Worker integration tests with real local bindings for D1 and Durable Objects.
- [ ] **CP-112:** Add multi-client WebSocket tests for join, directed signal, broadcast policy, close, and role enforcement.
- [ ] **CP-113:** Add hibernation/restart tests with socket attachments and persisted replay state.
- [ ] **CP-114:** Add D1 migration/repository/tenant-isolation tests.
- [ ] **CP-115:** Add fuzz/property tests for envelopes, state transitions, retry/reconnect schedules, and revision gaps.
- [ ] **CP-116:** Add load tests at documented limits, including slow consumers and reconnect storms; assert bounded CPU/storage/queue growth.
- [ ] **CP-117:** Add fault injection for storage/provider failures before commit, after commit/before send, during replay, and during alarm cleanup.
- [ ] **CP-118:** Add security tests for every threat-model case and redaction canaries.
- [ ] **CP-119:** Add compatibility fixtures proving old v1 clients tolerate newly added optional fields and unknown event handling follows policy.
- [ ] **CP-120:** Deploy to an isolated staging namespace with separate D1/DO/provider credentials and synthetic clients.
- [ ] **CP-121:** Run a rollback/forward-fix rehearsal; never deploy code that cannot read the currently stored schema.
- [ ] **CP-122:** Canary by traffic/tenant cohort, monitor SLO/error/resync/storage metrics, then expand gradually.

**Release Gate R1 (code complete):** G0–G7 pass; protocol fixtures and migrations are frozen for the candidate.  
**Release Gate R2 (staging):** G8 plus integration, hibernation, load, fault, and security suites pass.  
**Release Gate R3 (canary):** no SLO burn, anomalous close/resync rate, schema errors, or secret leakage for the observation window.  
**Release Gate R4 (general availability):** runbooks staffed, rollback/forward-fix rehearsed, retention jobs verified, and dashboards/alerts owned.

## Required proof artifacts

- Versioned protocol schemas and compatibility fixtures.
- State-transition and authorization matrix.
- D1 and Durable Object storage schemas plus migration policy.
- Threat model and field classification/redaction allowlist.
- Test report including property/load/fault/hibernation results.
- Staging/canary dashboard links and operational runbooks.

## Open decisions to resolve at G0

1. Maximum replay age/count/bytes and whether a resumable snapshot is sufficient for every event class.
2. JSON numeric versus decimal-string revision encoding.
3. Exact Durable Objects WebSocket hibernation API available in the selected compatibility date.
4. Authentication provider and revocation-latency target.
5. Authorization source and whether the admission snapshot must be refreshed during long-lived connections.
6. Session expiry and audit/metadata retention periods.
7. Provider(s) for TURN credential issuance and whether failover is required in v1.
