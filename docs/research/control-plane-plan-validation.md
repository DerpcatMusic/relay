# Control-Plane Plan Validation

## Scope

Validate `docs/plans/2026-08-15-relay-control-plane-plan.md` against a tightly bounded set of official Cloudflare primary sources covering Workers, Durable Objects, WebSocket hibernation, and D1. This record does not edit the plan or code.

## Validation criteria

- Proposed APIs and configuration are supported by current official documentation.
- Runtime ownership and consistency boundaries are correctly assigned.
- WebSocket lifecycle assumptions account for hibernation and reconnection behavior.
- D1 usage respects its consistency, transaction, and access model.
- Any plan claim that is unsupported, ambiguous, or outdated is identified as a potential correction.

## Source table

| # | Area | Official primary source | Consulted | Evidence applied |
|---|---|---|---|---|
| 1 | Workers | [How Workers works](https://developers.cloudflare.com/workers/reference/how-workers-works/) | Yes | Workers isolates may be evicted; requests are not guaranteed to reach the same instance; Cloudflare recommends not mutating global state. |
| 2 | Durable Objects | [What are Durable Objects?](https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/) | Yes | Each instance is addressable by an identifier, single-threaded/cooperatively multitasked, and has private durable transactional strongly consistent storage. |
| 3 | WebSocket hibernation | [Use WebSockets](https://developers.cloudflare.com/durable-objects/best-practices/websockets/) | Yes | Proxy upgrade Worker→DO; use `ctx.acceptWebSocket`, event handlers, `getWebSockets`, and small serialized attachments; constructor reruns after hibernation. |
| 4 | D1 | [D1 read replication](https://developers.cloudflare.com/d1/best-practices/read-replication/) | Yes | Without Sessions API queries use primary; replicas update asynchronously; Sessions provide sequential consistency, with `first-primary`/bookmarks for freshness. |

## Findings

### 1. Worker statelessness boundary is correct

Cloudflare states that isolates may be evicted and that no two user requests are guaranteed to reach the same Worker instance; it therefore recommends not using or mutating global state ([How Workers works](https://developers.cloudflare.com/workers/reference/how-workers-works/), accessed 2026-08-16). This supports the plan’s Worker-as-stateless-gateway boundary, its rejection of cross-request in-memory coordination, and routing authoritative per-session state to a Durable Object. The source does **not** by itself validate authentication, origin, header-size, or forwarding details; those remain implementation requirements rather than platform guarantees.

### 2. One authoritative Durable Object per canonical session is platform-aligned

Cloudflare documents that every Durable Object instance has an identifier by which it is globally addressed, is single-threaded and cooperatively multitasked, and owns storage that is durable, transactional, strongly consistent, and accessible only within that object ([What are Durable Objects?](https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/), updated 2026-07-15; accessed 2026-08-16). This supports CP-002/CP-003/CP-017 and the placement of ordering, revision allocation, replay, idempotency, and recovery-critical state inside one `SessionDO` selected from the canonical session ID. It also supports keeping another database out of the live ordering path.

“Single-threaded” must not be read as “all multi-step async logic is automatically atomic”: the plan correctly requires an explicit concurrency-safe initialization barrier and a single atomic storage commit for each externally visible event.

### 3. The hibernation design is correct, with an attachment-lifetime constraint to make explicit

Cloudflare recommends the Hibernation WebSocket API. Its documented flow is: proxy the upgrade request from the Worker to the Durable Object, create the WebSocket pair in the object, call `ctx.acceptWebSocket(server)`, and implement `webSocketMessage`/close/error handlers. When an event wakes a hibernated object, its constructor runs again; `ctx.getWebSockets()` and `serializeAttachment()`/`deserializeAttachment()` support reconstructing connection indexes ([Use WebSockets](https://developers.cloudflare.com/durable-objects/best-practices/websockets/), updated 2026-06-19; accessed 2026-08-16). This directly validates CP-018, CP-033, CP-042, CP-043, and the hibernation tests.

The same source says attachments survive hibernation only while the WebSocket remains healthy, are lost when either side closes, and are limited to 16,384 serialized bytes. Thus attachments are appropriate for the plan’s **minimal per-live-connection metadata**, but not for recovery-critical session/replay state. The plan already places that durable state in object storage; it should state the attachment size/lifetime constraint explicitly in CP-033 or CP-043.

### 4. The D1 boundary is sound, but its consistency wording and admission-read policy need correction

Cloudflare documents that without read replication/Sessions API, all D1 queries run on the primary. With read replication, copies receive updates asynchronously and may be arbitrarily out of date; the Sessions API provides sequential consistency. `withSession("first-primary")` starts from the latest primary version, while a bookmark guarantees a new session begins at least as up to date as the session that produced it ([D1 read replication](https://developers.cloudflare.com/d1/best-practices/read-replication/), accessed 2026-08-16).

This validates keeping D1 out of per-frame ordering and treating D1-backed catalog views as potentially stale **when read replicas are enabled without an appropriate session constraint**. However, CP-074’s unqualified phrase “D1 is eventually consistent” is inaccurate: asynchronous **closure propagation** can be eventually consistent, and read replicas can lag, but ordinary primary access is not described that way and Sessions provide defined sequential guarantees. CP-023 must also specify the freshness mode for security-sensitive grant/revocation admission reads (for example, `first-primary`, or a bookmark/other policy whose maximum staleness meets the revocation target); an unconstrained replica session is not sufficient for a “latest grant state” requirement.

## Potential corrections to the master plan

The plan should remain draft until the following are resolved. These are proposed corrections only; this validation did not edit the plan.

1. **P0 — Freeze one signaling representation and align the plan with the existing V1 contract.** The proposed boundaries say clients use the existing Protobuf `relay.v1.Envelope`, while the protocol sketch is JSON and CP-006 calls for JSON Schemas. The checked-in `proto/relay/v1/signaling.proto` uses `message_id`, `uint64 revision`, `Welcome`, `ResumeAccepted`, and `FullRenegotiationRequired`; it does not currently define the sketched `commandId`, `expectedRevision`, `resume.complete`, `resync.required`, or snapshot messages. Either:
   - keep Protobuf as normative, express sketches as explicitly non-wire pseudocode, change CP-006 to Protobuf/Buf plus semantic fixtures, and amend the `.proto`/`docs/protocols/signaling-v1.md` through the normal protocol decision process for any new messages; or
   - make an explicit architecture decision to replace/encapsulate the existing Protobuf contract and update every dependent artifact.
   CP-050’s JSON-number-versus-decimal-string decision also belongs only to a JSON encoding; it must not silently redefine the existing Protobuf `uint64` field.
2. **P0 — Specify consistency for security-sensitive D1 admission reads (CP-023/CP-027).** State whether read replication is enabled and require a freshness mechanism appropriate to revocation policy. For reads that must observe latest grant state, use a primary-constrained session such as `withSession("first-primary")`, or document an equivalent bookmark/epoch design and its maximum revocation latency. Do not allow an unconstrained, potentially stale replica read to make a “current authorization” claim.
3. **P1 — Replace the blanket “D1 is eventually consistent” wording in CP-074.** Say that the **SessionDO→D1 closure-summary workflow** is asynchronous/eventually propagated and does not block signaling. Separately document D1 access semantics: primary-only access when Sessions/read replication are not used; possibly lagging asynchronous replicas when enabled; sequential consistency within Sessions; and explicit primary/bookmark constraints when freshness matters.
4. **P1 — Add the WebSocket attachment boundary to CP-033/CP-043.** Serialized attachments survive hibernation only while the socket remains healthy, disappear on connection close, and are capped at 16,384 bytes. Keep only bounded per-connection reconstruction metadata there; all recovery-critical membership/revision/replay/idempotency state stays in Durable Object storage.
5. **P1 — Narrow open decision 3.** Current official documentation already identifies the Hibernation API shape (`ctx.acceptWebSocket`, `ctx.getWebSockets`, WebSocket event-handler methods, serialized attachments). The remaining G0 decision is to pin the project compatibility date/types and verify this exact API in the chosen local/test/deploy toolchain, including close-frame behavior for that compatibility date—not to re-decide whether a hibernation API exists.
6. **P2 — Qualify platform-derived versus product-derived limits.** CP-007/CP-035/CP-040 correctly demand limits, but the four sources reviewed do not establish the plan’s intended header, frame, replay, socket, rate, or backpressure ceilings. Record them as product budgets validated by tests and current platform limits rather than implying they follow from these sources.

## Decisions reflected in the plan

| Plan decision | Validation result | Basis / condition |
|---|---|---|
| Worker is a stateless ingress gateway, not the live coordinator | **Validated** | Workers instances are distributed/evictable and global mutable state is not reliable across requests. |
| One canonically addressed `SessionDO` owns live session ordering and revisions | **Validated at architecture level** | A Durable Object is globally addressable, single-threaded/cooperatively multitasked, and has private strongly consistent transactional storage. Exact ID canonicalization remains a product invariant with vector tests. |
| Recovery-critical replay/idempotency/revision state belongs in Durable Object storage | **Validated** | Object storage is durable, transactional, strongly consistent, and object-private. Atomic implementation details still require tests. |
| Worker proxies the WebSocket upgrade; the Durable Object accepts the server endpoint | **Validated** | Matches the official hibernation flow and supports CP-018/CP-033. |
| Rebuild ephemeral connection indexes after hibernation from live sockets/attachments | **Validated with constraint** | Constructor re-executes and `getWebSockets`/attachments are available, but attachments are bounded and socket-lifetime-only. |
| D1 stores catalog/account/grant/audit metadata and does not order signaling frames | **Validated** | It avoids putting live ordering behind a separately accessed relational database and permits an explicitly stale/asynchronous product view where acceptable. |
| D1 is categorically “eventually consistent” | **Correction required** | This conflates async replica/closure propagation with all D1 access. Sessions expose sequential guarantees; primary access and constrained sessions have different freshness properties. |
| Existing Protobuf V1 plus the proposed JSON command/event contract | **Correction required** | The plan presently names both as normative-looking wire contracts without a mapping or schema migration. |
| Auth provider, revocation target, TURN provider, replay budgets, and retention | **Still open by design** | These are product/security decisions; the bounded Cloudflare source review does not resolve them. |

**Validation verdict:** the control-plane ownership model is sound and aligned with Cloudflare’s execution and storage primitives. The plan is **conditionally validated**, not yet execution-ready: resolve the Protobuf/JSON wire-contract conflict and the D1 admission consistency policy before G0, then incorporate the attachment and wording clarifications.

## Validation proof

- Evidence file created before inspecting the plan or consulting sources, as required.
- Plan/code edits: none. The plan was read only; its post-validation SHA-256 is `91c2f5db1edb13269b8c02a0a42fb2d74017a25d96ec3cb48c318b9f9c95ae37`.
- Primary sources consulted: **4 of at most 4**. Each was an official `developers.cloudflare.com` page and returned HTTP 200 during validation.
- Source 1 captured immediately after consultation: official Workers execution-model documentation.
- Source 2 captured immediately after consultation: official Durable Objects concepts documentation.
- Source 3 captured immediately after consultation: official Durable Objects WebSocket hibernation documentation.
- Source 4 captured immediately after consultation: official D1 read-replication and Sessions documentation.
- Local contract cross-check (not an additional external source): `proto/relay/v1/signaling.proto` and `docs/protocols/signaling-v1.md` confirm that the repository’s current V1 contract is Protobuf and expose the wire-shape conflict recorded above.
- Required sections present: scope, criteria, source table, findings, explicit corrections, decisions reflected in the plan, and validation proof. No pending placeholders remain.

| Criterion | Result |
|---|---|
| Runtime ownership and consistency boundary | Pass |
| Durable Object authority/storage placement | Pass |
| Hibernating WebSocket lifecycle | Pass with attachment constraint |
| D1 access/consistency model | Conditional; corrections required |
| Protocol/API internal coherence | Fail until Protobuf/JSON contract is reconciled |
| Overall plan | Conditionally validated |
