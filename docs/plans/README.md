# RELAY implementation plans

The [master architecture/specification](2026-08-15-relay-master-plan.md) is intentionally not an executable mega-ticket. Work proceeds through the focused plans below.

Every plan is paired with a task-local research record. Before implementation, the assigned agent must read that record and explicitly disposition every listed potential correction in its task evidence. A plan marked “validated” is not permission to skip its unresolved gates.

| Area | Executable plan | Research/correction evidence | Execution state |
|---|---|---|---|
| Foundation | [Phase 0](2026-08-15-relay-phase-0-foundation-plan.md) | [Integration evidence](../research/phase-0-integration.md) | First slice locally validated; exit pending CI/testkit/contracts/bootstrap |
| Audio engine/lab | [Phase 1 audio](2026-08-15-relay-audio-plan.md) | [Audio validation](../research/audio-plan-validation.md) | Next implementation phase; research corrections must be applied first |
| Native transport | [Phase 2 bake-off](2026-08-15-relay-transport-plan.md) | [Transport validation](../research/transport-plan-validation.md) | Blocked on Phase 1 exit |
| Control plane/signaling | [Control-plane plan](2026-08-15-relay-control-plane-plan.md) | [Control-plane validation](../research/control-plane-plan-validation.md) | Blocked on protocol G0 corrections |
| Browser | [Web plan](2026-08-15-relay-web-plan.md) | [Web validation](../research/web-plan-validation.md) | Blocked on standalone transport/signaling seams |
| Plugin shell | [Plugin-shell plan](2026-08-15-relay-plugin-shell-plan.md) | [Plugin validation](../research/plugin-shell-plan-validation.md) | Blocked on standalone Connect and Truce P1 spike |
| Billing/credits | [Billing plan](2026-08-15-relay-billing-plan.md) | [Billing validation](../research/billing-plan-validation.md) | Blocked until paid routes work in beta |
| CI/hardening/release | [Release plan](2026-08-15-relay-release-plan.md) | [Release validation](../research/release-plan-validation.md) | Foundation policy only; release execution remains blocked |

## Required execution sequence

```text
Phase 0 foundation (in progress)
  → Phase 1 deterministic audio-lab
  → Phase 2 transport bake-off/probe
  → standalone Connect
  → browser Link + control-plane signaling
  → plugin shell
  → TURN and Stream providers
  → authentication and billing
  → hardening and desktop release
```

Control-plane and web scaffolding may be explored in parallel only when their contracts do not claim completion ahead of the transport gates.
