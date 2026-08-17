# RELAY Billing and Credits Plan

**Date:** 2026-08-15  
**Status:** Executable implementation plan; provider details pending validation  
**Scope:** Prepaid credits and usage charging for RELAY. No implementation is included here.

## 1. Goals and invariants

RELAY will charge usage against prepaid credits without allowing retries, concurrent sessions, delayed provider events, or worker failures to double-credit or double-debit an account.

Hard invariants:

1. Money and credits are represented as integers in explicitly named minor units; floating point is forbidden in billing state and arithmetic.
2. The credit ledger is append-only. Posted entries are never updated or deleted; mistakes are corrected by compensating entries linked to the original entry.
3. Each economic fact has one stable idempotency key and can affect the ledger at most once.
4. An account's spendable balance changes only through its `AccountMeterDO`, which serializes admission, reservation, capture, release, and provider-credit posting.
5. Usage must be reserved before work starts. Captured usage cannot exceed the live reservation unless a narrowly bounded, explicitly audited overdraft policy is enabled.
6. Provider webhooks are evidence, not mutable account state. Their raw verified envelope and processing result are retained for replay and audit.
7. The system can reconstruct balances from the ledger and reconcile them with provider transactions and operational usage records.
8. Provider-specific concepts remain behind a billing-provider seam. Paddle is the planned primary provider; Stripe remains a documented fallback rather than a mixed runtime integration.

## 2. Units and account model

Choose one immutable credit scale before implementation, for example `1 credit = 1_000_000 microcredits`. Store:

- provider money as integer minor units plus ISO currency (`amount_minor`, `currency`);
- RELAY value as signed `amount_microcredits`;
- metered usage in integer native units (bytes, milliseconds, messages) before deterministic conversion;
- conversion/pricing versions on every quote, reservation, and capture.

Never infer currency scale from decimal formatting. The provider adapter must normalize provider amounts into integer minor units and reject unsupported currencies or fractional conversions. Pricing conversion uses checked integer arithmetic with an explicit rounding rule (normally round usage charge up at final aggregation) and overflow failure.

An account has an immutable `account_id`. Provider customer/subscription/transaction IDs are mappings, never primary account identity. A payer may fund one account in V1; expanding payer-to-account cardinality requires a later model change.

## 3. Append-only ledger

Use durable SQL storage as the audit source of truth, partitioned logically by account and written only through the account's Durable Object. Suggested records:

### `ledger_entries`

- `entry_id` (ULID/UUID; primary key)
- `account_id`
- `sequence` (monotonic per account; unique with `account_id`)
- `kind`: `purchase_credit`, `usage_capture`, `refund_debit`, `chargeback_debit`, `manual_adjustment`, `expiry_debit`, `correction`
- `amount_microcredits` (signed, non-zero)
- `currency` and `provider_amount_minor` when backed by money
- `provider`, `provider_object_id`, `provider_event_id` when applicable
- `reservation_id`, `usage_record_id` when applicable
- `idempotency_key` (unique within account; namespaced, e.g. `paddle:transaction:<id>:credit`)
- `pricing_version`
- `corrects_entry_id` (nullable)
- `reason_code`; do not store sensitive free-form provider payloads here
- `occurred_at`, `recorded_at`
- `integrity_version` (and optional hash-chain fields if threat modeling justifies them)

A synchronous SQLite storage transaction inserts one ledger entry and updates a derived account snapshot (`posted_balance_microcredits`, next sequence). The snapshot is a cache checked against ledger replay, not an independent authority. Enforce uniqueness of economic identity using database constraints, not only application checks.

Reversals:

- provider refund/chargeback creates a new negative entry with its own provider-scoped idempotency key;
- a correction references the faulty entry and states the reason;
- never mutate an amount, provider reference, or posted timestamp;
- if a reversal makes the posted balance negative, block new reservations and expose account debt; do not erase previously captured usage.

## 4. Reservations and leases

A reservation prevents concurrent work from spending the same balance.

### Reservation record

- `reservation_id`, `account_id`, `work_id`
- `maximum_microcredits`
- `captured_microcredits` (monotonic)
- `state`: `active`, `closed`, `expired`, `cancelled`
- `pricing_version`
- `lease_generation` (monotonic fencing token)
- `lease_expires_at`
- `created_at`, `updated_at`, final reason
- unique idempotency key for open and each capture/finalize operation

Available balance is `posted_balance - sum(active reservation remaining amounts)`. The DO performs the availability check and reservation insert atomically. It returns a reservation ID plus lease generation. Workers must present both for heartbeats, captures, and close, preventing stale owners from acting after a lease is renewed or reassigned.

Rules:

1. Estimate a conservative maximum and reserve it before starting billable work.
2. Heartbeat/extend using an idempotent operation and bounded maximum lifetime; never silently increase the maximum.
3. For streaming work, incrementally capture deterministic usage checkpoints. Each checkpoint has a stable `usage_record_id` and cumulative-or-delta contract chosen once; cumulative checkpoints are preferred because retries are naturally deduplicated.
4. Finalization atomically posts the remaining usage capture and releases unused capacity.
5. Expiry releases only the uncaptured remainder. Already posted captures remain ledger facts.
6. An alarm scans expired leases and closes them idempotently. Alarm time is not an authorization boundary: every command compares `lease_expires_at` with current time because Cloudflare documents that alarms can be delayed during maintenance/failover. A later completion using an expired generation is rejected and routed for reconciliation, not blindly charged.
7. Reservation records remain as audit records after closure; they are not reused.

## 5. `AccountMeterDO` serialization boundary

One Durable Object instance per stable `account_id` is the sole command handler for account monetary state. Its public commands are intentionally narrow:

- `quoteAndReserve(request)`
- `heartbeatReservation(request)`
- `captureUsage(request)`
- `finalizeReservation(request)`
- `cancelReservation(request)`
- `postProviderCredit(command)`
- `postProviderReversal(command)`
- `getBalance()` and restricted audit/reconciliation reads

Every mutating request supplies an operation idempotency key and expected account/pricing context. On SQLite-backed Durable Objects, use `ctx.storage.transactionSync()` with a synchronous callback (or an equivalently await-free atomic SQL write sequence) to check deduplication, validate state/fencing tokens, append ledger entries if needed, update reservation/snapshot state, record the response, and commit. Retried calls return the previously committed semantic response. Do not put an `async` callback or a promise inside `transactionSync()`, and do not opt into `allowConcurrency` for monetary operations.

The Durable Object is the account coordination atom, but “one DO per account” is not by itself a proof that arbitrary asynchronous handler code is serializable. Keep the monetary read/validate/write sequence inside the synchronous SQLite transaction, and test handler interleavings.

Do not call external provider APIs from a DO storage transaction. Webhook ingress verifies and durably records events first; asynchronous processing sends normalized commands to the DO. Provider API retrieval, when required, happens outside account transactions.

Use DO alarms for reservation expiry and repair wakeups, but make handlers re-entrant and idempotent because delivery can be retried. Maintain bounded operation-result retention sufficient for provider replay horizons; permanent economic keys must retain a compact uniqueness tombstone indefinitely or for the legal ledger lifetime.

## 6. Idempotent webhook pipeline

Webhook ingress must:

1. Accept only HTTPS `POST`, impose strict body-size/time limits, and read the raw request body exactly once.
2. Verify the provider signature over the required raw bytes using the configured webhook secret and provider timestamp/replay rules before parsing or acknowledging success.
3. Parse only after verification; validate event schema and supported environment/account.
4. Persist a webhook inbox row keyed by `(provider, provider_event_id)` containing a payload digest, minimal routing metadata, received time, verification result/key version, processing status, and encrypted/restricted raw payload or a retention-controlled object reference.
5. If the same event ID and digest is already present, return success without a second economic effect. If the event ID is reused with a different digest, quarantine and alert.
6. Acknowledge only after the small durable inbox insertion, then return `2xx` promptly. Process asynchronously so provider retry timing is decoupled from the account mutation.
7. Normalize the event into a provider-independent command. The ledger command uses a stable semantic economic key — provider namespace, effect type, economic object ID, and any provider-defined sub-effect discriminator — rather than webhook delivery ID alone. For Stripe specifically, the documented duplicate key is `event.type` plus `data.object.id`; the inbox separately deduplicates `event.id`.
8. Mark inbox processing outcome and retry safely with bounded exponential backoff/dead-letter visibility. Unknown event types are recorded and acknowledged as ignored; malformed or unverifiable events fail according to provider retry guidance without leaking details.

Never grant credits from a client redirect, checkout success page, unverified client claim, or subscription-created event. For Paddle V1, grant only from a verified `transaction.completed` fact whose transaction `status` is `completed`, after validating the correct environment/currency/product/price mapping and non-negative amount. Paddle documents that the event occurs only after paid-transaction processing completes; do not treat an earlier generic “paid” observation as equivalent.

Out-of-order events are normal. A refund may be recorded before a delayed purchase event; economic IDs and compensating entries must converge regardless of delivery order. If required information is absent, retain the event pending provider retrieval/reconciliation rather than guessing.

## 7. Provider seam: Paddle primary, Stripe fallback

Define a small adapter contract:

- create checkout/top-up request from a server-created, short-lived quote;
- verify webhook signature from raw request data;
- parse/deduplicate provider event envelope;
- normalize paid transaction, refund, adjustment/chargeback, and cancellation facts;
- fetch canonical transaction state for reconciliation;
- expose provider request idempotency support and normalized error classes.

The core owns account IDs, quotes, pricing versions, ledger semantics, credits, reservation rules, and reconciliation. The adapter owns provider signatures, event names/statuses, IDs, monetary fields, and API calls.

### Paddle launch path

- Use Paddle-hosted checkout/client token only as documented; secrets remain server-side.
- Put an opaque quote/reference ID in supported custom data, then resolve it server-side. Do not trust a client-supplied account or credit amount.
- Map configured Paddle price IDs to server-side packages and environments.
- Post credit exactly once from verified `transaction.completed` with transaction `status = completed`; retain the transaction ID as the economic object identity.
- Translate adjustments/refunds/chargebacks to compensating ledger commands only after their current Paddle schemas and terminal-state semantics receive separate contract fixtures; the `transaction.completed` source does not prove adjustment semantics.

### Stripe fallback

Keep a second adapter design and contract tests for Stripe Checkout/PaymentIntents without enabling dual-provider writes. Stripe-specific event objects, signatures, and idempotency headers stay inside the adapter. Activation requires operational configuration, webhook replay tests, product/price mapping, reconciliation support, and a migration decision for existing Paddle customers. One economic purchase must have exactly one provider namespace.

## 8. Reconciliation and repair

Run four independent reconciliations:

1. **Ledger replay:** recompute each account's posted balance and sequence from append-only entries; compare to cached snapshots and repair cache drift only after alerting.
2. **Provider-to-ledger:** page provider transactions/adjustments over overlapping time windows; assert every canonical paid/reversed fact has exactly one matching ledger effect with equal currency/minor amount and correct package conversion.
3. **Ledger-to-provider:** verify every provider-backed ledger entry resolves to a canonical provider fact and seller/environment; quarantine orphaned or mismatched entries.
4. **Usage-to-ledger/reservations:** assert every accepted usage checkpoint has one capture, captures do not exceed reservation maxima, terminal work has terminal reservation state, and expired active reservations are released.

Persist reconciliation run IDs, cursors, source windows, counts, mismatches, decisions, and repair command IDs. Windows overlap so late events are found; dedupe makes repeats harmless. Automatic repair may replay a missing already-proven fact through the same DO command. Amount disagreements, cross-account mappings, negative-balance reversals, or suspicious volume require human review. Never directly patch ledger rows.

Daily automated runs plus near-real-time event processing are required for GA. Alert on inbox age, dead letters, negative accounts, stale reservations, mismatch rate, webhook signature failures, and provider API lag/rate limits.

## 9. Security, fraud, privacy, and abuse controls

- Authenticate/authorize all balance and reservation APIs; derive `account_id` from server identity, never request body alone.
- Separate webhook secrets, API secrets, client tokens, and per-environment product mappings; rotate keys with an explicit overlap procedure.
- Constant-time signature verification via provider SDK/recipe; raw-body preservation; timestamp/replay validation where supported.
- Rate-limit checkout creation, reservation attempts, and webhook ingress independently. Apply per-account/device/IP velocity signals without making IP a durable identity.
- Server-side package allowlist, bounds on top-up and reservation size, checked arithmetic, currency/environment/seller verification.
- Treat custom data and webhook fields as hostile. Escape logs, minimize PII, encrypt restricted payloads, redact secrets, and enforce retention/access policies.
- Require privileged, reason-coded, dual-controlled manual adjustments; all operator reads/writes are audited.
- Detect purchase-refund-spend abuse: refunds debit the account even after credits were spent, block further spending at negative balance, and flag linked identities/payment instruments using provider-supported risk signals.
- Do not store card data. Use hosted provider surfaces to keep payment details outside RELAY systems.

## 10. Required tests

### Arithmetic/property tests

- integer conversion/rounding at zero, minimum, maximum, and overflow boundaries;
- balance invariant under randomized sequences of reserve/capture/release/credit/reversal;
- ledger replay equals snapshot;
- compensation never mutates originals.

### Concurrency/failure tests

- hundreds of simultaneous reservations cannot exceed available balance;
- duplicate capture/finalize/webhook deliveries return the committed result once;
- crash before/after inbox insert, DO transaction commit, response send, alarm execution, and reconciliation repair;
- stale lease generation cannot heartbeat/capture/finalize;
- expiry racing with capture yields one valid serialized outcome;
- out-of-order purchase/refund events converge;
- DO eviction/restart preserves dedupe and balance.

### Provider contract/security tests

- official signed fixture succeeds; altered body/signature/timestamp fails (Paddle SDK default timestamp tolerance is five seconds; Stripe library default is five minutes);
- body reserialization cannot accidentally pass/fail verification path;
- same event ID/different digest quarantines; replay processing trusts the stored verification result and digest rather than attempting to reverify an expired provider timestamp;
- test/live environment, seller, currency, product/price, amount, and account-reference mismatches reject;
- checkout redirect/client claim cannot grant credit;
- SSRF-resistant canonical retrieval, payload limits, malformed JSON, unknown event types, log injection, secret rotation;
- provider retry storm and rate limiting do not lose a valid event;
- refunds, partial refunds, multiple adjustments, chargebacks, disputed facts, and duplicate provider objects post exactly once.

### Reconciliation/e2e tests

- seeded provider/ledger/usage mismatches are detected with correct classification;
- replay repair uses normal idempotent commands and cannot double-post;
- cursor overlap handles late facts;
- Paddle sandbox purchase through spend and refund; equivalent disabled Stripe adapter contract suite.

## 11. Executable task slices

Each slice is independently reviewable and must land with tests, schema/ADR updates, observability, and rollback notes.

1. **Decide units and semantics** — ADR for microcredit scale, pricing/rounding, expiry, refund debt, account ownership, and supported currencies. Golden conversion vectors.
2. **Define provider-neutral contracts** — quote, normalized economic facts, adapter errors, idempotency namespaces, test fixtures; fake adapter contract suite.
3. **Create ledger schema/repository** — append-only entries, account sequence/snapshot, constraints, replay verifier, compensating entries. No provider code.
4. **Create `AccountMeterDO` command surface** — serialized credit/reversal commands, persisted operation results, deterministic responses, authorization boundary.
5. **Add reservations/leases** — admission, fenced heartbeats, cumulative capture, finalization, alarms/expiry, race/property tests.
6. **Build webhook inbox** — raw-body verification boundary, durable inbox/digest collision behavior, async retry/dead-letter processing, metrics. Start with signed fake adapter fixtures.
7. **Implement Paddle adapter** — checkout quote linkage, current raw-body signature verification with timestamp handling, canonical `transaction.completed` normalization, separately validated adjustment normalization, sandbox fixtures, environment mappings.
8. **Integrate end-to-end Paddle flow** — top-up, completed-event credit, usage reserve/capture, refund debt; browser/API sandbox test and operator runbook.
9. **Build reconciliation jobs/tools** — four comparisons, cursors/overlap, report and repair commands, dashboards/alerts, dry-run default.
10. **Fraud/security hardening** — threat model, velocity/limits, privileged adjustment workflow, key rotation, privacy/retention, penetration/abuse test cases.
11. **Stripe fallback proof** — implement or spike adapter behind disabled configuration; pass common contract fixtures; document cutover/migration and explicit non-dual-write guard.
12. **GA soak and launch** — sandbox fault injection, production shadow reconciliation, limited-account canary, on-call drills, finance sign-off, staged limits.

## 12. GA gates

Do not declare GA until all are evidenced:

- unit/pricing/refund semantics approved by product, finance, and engineering;
- ledger append-only constraints, replay, backup/restore, and operator access audited;
- no double-spend or double-post in concurrency/property/fault-injection suites;
- Paddle signature verification and sandbox lifecycle fixtures match current primary documentation;
- reconciliation runs daily for at least 30 days in shadow/canary with zero unexplained material mismatches and measured late-event coverage;
- reservation expiry and DO restart/eviction soak passes at projected peak plus safety margin;
- alerting/on-call runbooks cover webhook backlog, provider outage, mismatch, negative balance, stale lease, secret compromise, and provider migration;
- manual adjustment and reconciliation repair require audited authorization and never mutate ledger history;
- privacy/security review completed; secrets rotated in a drill; no payment-card data enters RELAY;
- provider production account, tax/receipts/refund policy, terms, supported countries/currencies, and customer support process approved;
- load/cost model and per-account/global circuit breakers are configured;
- canary limits and rollback (disable new checkouts/reservations while preserving captures/reconciliation) tested;
- Stripe fallback contract suite/cutover document exists, without claiming production readiness unless separately certified.

## 13. Open decisions to close before Slice 1 exits

- Exact credit-to-currency/package policy and whether credits expire.
- Whether purchases are one-time top-ups only or subscriptions also grant periodic credits.
- Refund allocation when only part of a multi-package transaction is adjusted.
- Reservation estimate bounds, heartbeat cadence, maximum lease lifetime, and policy for meter failure.
- Durable storage placement/retention and jurisdiction requirements.
- Whether webhook processing should use Queues/Workflows or a simpler retry mechanism while retaining the same inbox semantics.
- Provider risk/chargeback signals RELAY may lawfully retain and act on.

## 14. Validation note

Provider- and platform-specific claims in this draft must be checked against no more than four current primary sources. Corrections and proof are recorded in `docs/research/billing-plan-validation.md`.
