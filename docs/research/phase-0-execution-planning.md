# Phase 0 execution planning — Research and implementation evidence

**Date:** 2026-08-15  
**Task owner:** Prime Agent (parent)  
**Status:** Complete

## Scope

Turn the master specification into a bounded Phase 0 execution plan and a mandatory evidence format. This task does not implement product behavior or choose provisional providers.

## Acceptance criteria

- [x] Preserve the supplied master plan at its requested path.
- [x] Define focused, file-owned Phase 0 tasks and an integration gate.
- [x] Require task-local research, potential corrections, and exact validation evidence.
- [x] Verify that the selected workspace/contract tools have official mechanisms matching the plan.

## Sources consulted

| Source | Why it is authoritative | Accessed |
|---|---|---|
| https://doc.rust-lang.org/cargo/reference/workspaces.html | Official Cargo workspace reference | 2026-08-15 |
| https://pnpm.io/workspaces | Official pnpm workspace documentation | 2026-08-15 |
| https://buf.build/docs/breaking/ | Official Buf breaking-change detection documentation | 2026-08-15 |
| `docs/plans/2026-08-15-relay-master-plan.md` | User-supplied product and architecture authority | 2026-08-15 |

All three external documents were retrieved successfully over HTTPS during this task. Serper web search was unavailable because this Prime Agent installation has no configured Serper key; direct primary-source retrieval was used instead.

## Findings

- Cargo workspaces provide a shared lockfile/output directory and support workspace-inherited package/lint/dependency configuration, matching the monorepo foundation goal.
- pnpm requires a root `pnpm-workspace.yaml`; a root `package.json` alone is not the workspace definition.
- Buf breaking detection is a separate compatibility gate from linting, so the eventual contract CI must run both rather than treating lint as schema compatibility proof.
- The master document is an architecture specification, not an executable ticket. File-owned, dependency-ordered tasks reduce collision risk and keep acceptance evidence local.

## Potential corrections to the master plan

1. **Make research evidence a task acceptance criterion, not merely documentation guidance.** Applied in the Phase 0 plan and template.
2. **Separate Protobuf lint and breaking checks explicitly.** The master CI section already names both conceptually; the executable plan makes the distinction a validation gate.
3. **Do not require unavailable tools to be silently installed globally.** Tasks must record tool gaps and use the repository's declared environment once bootstrapped.

## Decisions applied

- Preserved the supplied architecture verbatim in the requested master-plan file.
- Added a separate Phase 0 plan instead of mutating the master specification.
- Established one Markdown evidence record per focused task with a compulsory corrections section.
- Partitioned concurrent tasks by file ownership, followed by one integration task.

## Validation evidence

```text
GET https://doc.rust-lang.org/cargo/reference/workspaces.html -> HTTP 200
GET https://pnpm.io/workspaces -> HTTP 200
GET https://buf.build/docs/breaking/ -> HTTP 200
master plan -> 4,167 lines at requested path
```

## Deferred follow-ups

- Integrated validation after concurrent foundation tasks finish.
- Separate executable plans for audio, transport, control plane, browser, plugin, billing, and release.
