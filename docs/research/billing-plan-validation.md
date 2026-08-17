# Billing Plan Validation

## Scope

Validate `docs/plans/2026-08-15-relay-billing-plan.md` against a small set of current, official primary sources. This review is limited to billing-provider responsibilities and Cloudflare Durable Objects/D1 persistence semantics. It does not edit the master plan or application code.

## Validation criteria

- Provider claims distinguish checkout, subscription lifecycle, webhook delivery, and tax/compliance responsibilities.
- Webhook handling accounts for authenticity, retries, duplicate delivery, and event ordering where the provider documents them.
- Durable Objects and D1 are assigned roles consistent with their documented consistency, transaction, and storage behavior.
- Any recommended correction is explicit, bounded, and traceable to primary evidence.

## Source table

| # | Primary source | Targeted question | Evidence captured | Plan impact |
|---|---|---|---|---|
| 1 | [Paddle: `transaction.completed` webhook](https://developer.paddle.com/webhooks/transactions/transaction-completed) (accessed 2026-08-15) | When is purchase credit safe to post? | Paddle says this notification occurs when a transaction changes to `completed`; for automatic transactions it is sent after payment for the transaction is fully processed. It also says this event includes the related transaction and lists `status` as `completed`. | Supports using the verified transaction ID/status as the purchase-credit fact. This source does **not** establish adjustment/refund/chargeback semantics. |
| 2 | [Stripe: Webhooks](https://docs.stripe.com/webhooks) (accessed 2026-08-15) | What retry, duplication, ordering, and signature rules must the fallback adapter honor? | Stripe requires signature verification against the raw body, recommends returning `2xx` quickly, retries live deliveries for up to three days with exponential backoff, does not guarantee event ordering, and says duplicate Events can occur. It recommends logging processed event IDs; in some duplicate cases, use `data.object.id` together with `event.type`. Its libraries default to a five-minute timestamp tolerance. | Supports raw-body verification, asynchronous processing, idempotency, out-of-order convergence, and the stated Stripe tolerance. The plan should avoid calling `event.type + data.object.id` **the** documented duplicate key for all duplicates. |
| 3 | [Cloudflare: Durable Objects SQLite storage API](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/) (accessed 2026-08-15) | Are the planned account-local atomic writes valid? | Cloudflare documents strongly consistent private storage, atomic/isolated storage operations, event-loop input/output gates, automatic commit for SQL in one event-loop turn, and a synchronous-only `transactionSync()` callback. It says `allowConcurrency: true` opts out of the normal pause in event delivery during asynchronous KV storage operations. | Supports await-free account-local ledger/snapshot/reservation writes, the warning against an async transaction callback, and the conservative no-`allowConcurrency` rule. It does not validate the plan's alarm-delay wording. |
| 4 | [Cloudflare: D1 Database API](https://developers.cloudflare.com/d1/worker-api/d1-database/) (accessed 2026-08-15) | If D1 is used, what atomicity/consistency boundary is documented? | Cloudflare says `batch()` sends statements in one call, executes them sequentially as a SQL transaction, and rolls back the sequence if a statement fails. It also says read-replication sessions provide sequential consistency and may be initialized with `first-primary` or `first-unconstrained`. | Supports transactional batches inside one D1 database and explicit read-consistency sessions. It does not provide an atomic transaction spanning a D1 database and a Durable Object's private SQLite storage. The plan should name the authoritative store and any projection/outbox boundary explicitly. |

## Findings

### 1. Paddle completed-transaction fact

The plan's narrow Paddle launch rule is supported: `transaction.completed` represents a transaction whose status is `completed`, and automatic-collection payment has finished processing. Using the transaction ID—not a redirect or a subscription-created observation—as the economic identity is therefore well founded. The plan is also appropriately cautious that this event page proves nothing about adjustment, refund, or chargeback terminal states.

This single Paddle source does not validate the plan's signature-tolerance, webhook retry, or delivery-order statements; those claims must remain implementation-time contract checks unless covered by another source in this bounded review.

### 2. Stripe webhook delivery

Stripe's webhook guide supports the plan's raw-body verification boundary, prompt `2xx`, asynchronous processing, retry-safe inbox, out-of-order convergence, and five-minute default signature tolerance. One phrasing is too absolute: Stripe first recommends deduplicating retried deliveries by Event ID and then notes that, in some cases, two separate Event objects represent the same underlying object; only for that latter case does it recommend `data.object.id` together with `event.type`. The semantic economic key is a sound core design, but it should not be described as Stripe's universal "documented duplicate key."

### 3. Durable Object SQLite atomicity

Cloudflare's SQLite storage reference supports the plan's use of an await-free `transactionSync()` boundary for ledger, snapshot, reservation, and dedupe/result writes. It also confirms that the transaction callback cannot be asynchronous. This makes the plan's caution about arbitrary asynchronous handler code useful rather than redundant.

The source additionally says SQLite statements in the same event-loop turn are automatically committed together. Therefore `transactionSync()` is defensible for an explicit monetary critical section, but the plan should not imply it is the only valid atomic mechanism. This page also documents that `allowConcurrency: true` opts out of the normal pause in event delivery during asynchronous key-value storage operations. That supports the plan's conservative instruction not to opt into it for monetary operations. This bounded source does not prove the separate alarm-delay statement; keep that as an implementation-time platform check.

### 4. D1 boundary and reads

D1 can atomically execute a `batch()` within one D1 database, and read-replication sessions can provide sequentially consistent reads. Neither facility creates a transaction that spans D1 and a Durable Object's private SQLite database. The plan currently says “durable SQL storage” and later specifies SQLite-backed DO transactions, but it never says whether cross-account audit/reconciliation data lives only in per-account DO SQLite, in D1, or in a projected D1 copy. Before implementation, it should explicitly name the ledger authority and, if D1 is a projection, define the idempotent outbox/checkpoint and accepted projection lag.

## Explicit potential corrections to the master plan

These are proposed corrections only; this task does not edit the plan.

1. **Correct the Stripe deduplication wording (section 6, item 7).** Replace the assertion that Stripe's documented duplicate key *is* `event.type + data.object.id` with two cases:
   - deduplicate repeat delivery of the same Event by `event.id`; and
   - when separate Event objects may represent the same underlying object, use `event.type + data.object.id` as Stripe recommends, while retaining the core's stable economic idempotency key.

2. **Name the storage authority and D1 relationship (sections 3, 5, and 8).** State explicitly whether each account's SQLite-backed Durable Object is the authoritative ledger store. If D1 is used for cross-account reporting/reconciliation, describe it as an asynchronous projection and require an idempotent outbox/checkpoint, replay, lag monitoring, and a declared read-consistency policy. Do not imply an atomic commit across DO SQLite and D1.

3. **Qualify two unverified platform/provider specifics (sections 4 and 10).** This four-source review did not establish the claim that Durable Object alarms may be delayed specifically “during maintenance/failover,” nor the claimed Paddle SDK default tolerance of five seconds. Either add current first-party citations/contract fixtures before implementation or restate these as conservative design assumptions without attributing the exact detail to Cloudflare/Paddle. The Stripe five-minute default is supported.

4. **Do not overstate provider validation.** Keep Paddle adjustment/refund/chargeback normalization gated on separate current schemas and fixtures. Also keep tax, receipts, country/currency coverage, refund policy, and production-account readiness as GA approval items; none was established by the single Paddle event source used here.

No correction is indicated for integer arithmetic, append-only compensation, reservation fencing, or provider-neutral ledger idempotency. Those are system design decisions rather than facts proved by these four product-documentation pages, and the consulted sources do not contradict them.

## Decisions reflected in the plan

| Plan decision | Validation outcome |
|---|---|
| Paddle primary; Stripe disabled fallback; no mixed writes | Coherent architectural decision, but this bounded evidence set does not compare provider suitability or prove operational readiness. Keep the plan's “pending validation” status. |
| Credit only on verified Paddle `transaction.completed` with `status = completed` | **Supported** by the Paddle event reference. |
| Adjustment/refund/chargeback handling requires separate fixtures | **Supported as a necessary limitation** because the completed-transaction source does not define those lifecycles. |
| Raw-body verification, prompt acknowledgement, asynchronous processing, retry-safe dedupe, and order-independent convergence | **Supported for Stripe** by its webhook guide. Only the raw verified-body/economic-fact architecture—not Paddle's exact delivery rules—was validated for Paddle here. |
| Inbox delivery identity is separate from economic idempotency | **Supported**, with the Stripe wording correction above. |
| One SQLite-backed `AccountMeterDO` is the account-local monetary serialization boundary | **Supported** by Cloudflare's private, strongly consistent DO storage and synchronous transaction semantics. |
| Avoid asynchronous work within `transactionSync()` and avoid `allowConcurrency` for monetary storage operations | **Supported** as the conservative implementation rule. |
| D1 as ledger authority or reporting projection | **Not decided in the plan.** Must be made explicit before schema/repository implementation. |
| One-time top-ups versus subscription-granted credits | Correctly remains an open product decision; no source in this review closes it. |
| Taxes, receipts, refund policy, supported geographies/currencies, and support readiness are GA gates | Correctly treated as approvals rather than assumed provider behavior; not substantively validated here. |

## Validation proof

- The evidence file was created as the first tool action, before the plan was read.
- Exactly four targeted primary sources were consulted: one Paddle page, one Stripe page, one Durable Objects page, and one D1 page. No secondary sources or broad search were used.
- This file was updated immediately after each source was retrieved.
- No plan or code was edited. `git diff -- docs/plans/2026-08-15-relay-billing-plan.md` was empty at validation time.
- Observed plan SHA-256 after validation: `f36cce12233a0c93e5589356c3422823f8b4753e8db720f73acde2e7b6290463`.
- Review disposition: **valid foundation with three required clarifications before implementation**—Stripe duplicate semantics, authoritative DO/D1 storage topology, and citation/qualification of exact alarm and Paddle-tolerance claims.
