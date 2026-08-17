# RELAY Browser/Web Execution Plan

**Date:** 2026-08-15  
**Status:** Validated execution plan  
**Scope:** Browser receive/share experience only. This plan does not implement UI.

## Outcome

Extend the existing Astro 7 shell and `@relay/web-rtc` placeholder into a browser client whose media and realtime logic lives in the framework-independent package. The client must make browser lifecycle transitions explicit, play remote media only after user/browser permission allows it, expose actionable connection statistics, recover predictably, provide a shareable receive page, and pass automated and real-device release gates.

## Guardrails

- Keep Astro responsible for routes, document structure, build output, and hydration boundaries.
- Keep signaling, peer connection, lifecycle, reconnection, and stats logic out of Astro components.
- Treat autoplay, backgrounding, network loss, device changes, and teardown as normal states.
- Do not hide browser policy failures behind automatic retries; surface a resumable state.
- Make each task small enough for one focused commit and give every task an observable proof.

## Validated starting point

The repository already contains `apps/web` (Astro 7.2.2, static output today) and a source-first `packages/web-rtc` package whose inert `RelayWebSession` exposes only `idle`. Extend these foundations; do not scaffold replacements. The canonical product link in the master plan is `/r/:sessionId`, not `/share/:sessionId`. The existing signaling V1 protobuf and `docs/protocols/signaling-v1.md` remain authoritative.

## Proposed package and route seams

```text
apps/web/
  src/pages/r/[sessionId].astro       # requires a decided on-demand adapter/route mode
  src/components/receiver/ReceiverShell.astro
  src/lib/receiver-entry.ts           # the single browser-only entry and DOM/media adapter
  src/lib/receiver-view-model.ts
  tests/e2e/
packages/web-rtc/
  src/types.ts
  src/lifecycle.ts
  src/peer-session.ts
  src/stats.ts
  src/reconnect.ts
  src/RelayWebSession.ts
  src/index.ts
  test/
```

Use Astro for the mostly static document shell and opt in only the receiver browser entry to client JavaScript. Do not add React/Svelte/Vue merely to obtain an island: the present repository has no UI-framework integration, and a small bundled module script is sufficient. If a UI framework is later selected, record that separately and keep the same adapter boundary.

`packages/web-rtc` must not import Astro or a component framework, touch browser globals at module evaluation time, or accept DOM elements in its public control API. WebRTC types are allowed because this is a browser/WebRTC adapter package. Inject timers, lifecycle/network signals, WebRTC construction, signaling transport, and media-sink callbacks so state-machine tests run deterministically.

## Execution plan

### Phase 0 — decisions, contracts, and fixtures

1. **Record supported browsers and devices.** Add `docs/web/browser-support.md` with minimum versions for stable Chromium, Firefox, Safari, iOS Safari, and Android Chrome, plus a named owner and review date. **Proof:** the matrix distinguishes automated engines from physical-device gates.
2. **Freeze the canonical link contract.** Define `/r/:sessionId`, identifier syntax, lookup, expiry/revocation behavior, and safe error states. Treat the path value as an opaque locator, not a signaling credential; obtain a short-lived join ticket through the control-plane boundary and keep it out of HTML, logs, referrers, and analytics. **Proof:** route examples and rejection/privacy cases align with the master plan and signaling V1 security rules.
3. **Reuse the V1 signaling contract.** Map the browser client to `proto/relay/v1/signaling.proto` and `docs/protocols/signaling-v1.md`; do not invent a second offer/answer/ICE schema. Record the web transport encoding/framing decision and generate/import types from the shared schema. **Proof:** browser fixtures round-trip canonical V1 envelopes and reject its documented invalid cases.
4. **Decide Astro route delivery before coding the dynamic page.** The current app is a static Astro build, while arbitrary `/r/:sessionId` values require either on-demand rendering with an adapter or a documented edge rewrite to a static receiver document. Choose one in an ADR, including how invalid/expired IDs are resolved. **Proof:** a production-like preview can request two previously unknown valid IDs and a missing ID directly (not through client navigation).
5. **Choose the TypeScript test/tooling boundary.** Add package scripts for `@relay/web-rtc` unit tests and `@relay/web` Playwright tests, retaining exact dependency pins and the current Node/pnpm policy. **Proof:** empty/smoke suites run through the workspace commands listed below.
6. **Create deterministic WebRTC fakes.** Add fake peer connection, signaling transport, remote track/stream, clock, visibility/page-transition, and online/offline adapters. **Proof:** a test drives idle → signaling → playing → recovering → closed without a real browser.

**Gate G0:** Route delivery, V1 signaling reuse, browser support, and the deterministic test seam are reviewed before session implementation.

### Phase 1 — extend the existing Astro shell

7. **Harden the existing shell rather than recreating it.** Extract the current document structure into the smallest useful layout/shell, retain metadata, and keep connection construction out of Astro frontmatter. **Proof:** `pnpm --filter @relay/web build` succeeds without evaluating `RTCPeerConnection`, networking, or timers during build/server rendering.
8. **Add the canonical receiver route.** Create `apps/web/src/pages/r/[sessionId].astro` using the G0 delivery decision; validate only the opaque locator and render non-secret/bootstrap-safe state. **Proof:** direct valid, malformed, expired, and revoked requests produce distinct safe results.
9. **Add a static receiver shell.** Create `ReceiverShell.astro` with meaningful connecting/failure fallback markup and the media element, but no live connection in Astro frontmatter. **Proof:** response HTML remains understandable with JavaScript disabled.
10. **Add one browser-only entry.** Load `receiver-entry.ts` only on the receiver route; it owns mount/dispose, DOM events, media-element binding, and construction of `RelayWebSession`. **Proof:** the landing page ships no receiver code and the receiver page creates exactly one session after client execution.
11. **Define the shell/module adapter.** Convert UI intents into `@relay/web-rtc` commands and module events into a serializable view model. **Proof:** adapter tests contain no signaling or direct `RTCPeerConnection` state logic.
12. **Add shell security policy.** Specify CSP `connect-src`/`media-src`, strict referrer policy, the minimal Permissions Policy, frame-ancestor policy, and HTTPS-only production assumptions. Ensure error reporting and artifacts redact the path locator and join ticket. **Proof:** production-like response headers and redaction snapshots match policy.

**Gate G1:** Direct receiver-route requests work in the chosen deployment mode; build has no browser-global side effects; static fallback and security headers pass.

### Phase 2 — framework-independent `web-rtc`

13. **Define public commands and events.** Commands: `start`, `approvePlayback`, `retry`, `setDocumentState`, and `stop`; events: lifecycle snapshot, remote media available, playback blocked, stats update, recoverable error, terminal error. **Proof:** public API compiles in a plain TypeScript test with no Astro dependency.
14. **Define the lifecycle state machine.** Use explicit states such as `idle`, `signaling`, `negotiating`, `awaiting-media`, `playback-blocked`, `playing`, `recovering`, `failed`, and `closed`; enumerate valid transitions. **Proof:** table-driven tests cover every allowed and forbidden transition.
15. **Separate transport from peer session.** Inject a typed signaling transport and make its open/message/close/error lifecycle explicit. **Proof:** peer-session tests run against an in-memory transport.
16. **Implement peer connection ownership.** One session owns creation, event handlers, remote description, ICE candidates, receivers, and deterministic close. **Proof:** repeated `stop` is idempotent and removes listeners/timers/tracks owned by the session.
17. **Handle ICE ordering.** Buffer remote candidates until the remote description is installed; reject candidates belonging to a superseded attempt. **Proof:** out-of-order fixture connects and stale-attempt fixture is ignored.
18. **Handle remote tracks.** Convert `track` events into framework-neutral media descriptors/callbacks and preserve stream/track replacement. **Proof:** add, replace, mute, unmute, and end events update the model.
19. **Prevent stale async completion.** Give each connection attempt an epoch/cancellation token and ignore late callbacks after retry/stop. **Proof:** delayed offer from attempt N cannot mutate attempt N+1.
20. **Make teardown exhaustive.** Stop stats polling, cancel reconnect, detach signaling, clear handlers, close the peer connection, and release owned media objects. **Proof:** leak-oriented test reports zero active timers/listeners after every terminal path.

### Phase 3 — browser lifecycle and media playback

21. **Add explicit document lifecycle input.** Adapt `visibilitychange`, `pagehide`, `pageshow`, `online`, and `offline` into commands rather than reading globals throughout the core. **Proof:** fake lifecycle sequences produce deterministic snapshots.
22. **Define hidden/frozen policy.** Keep an already-playing receive session alive while hidden unless platform evidence requires suspension; pause nonessential stats/UI work and revalidate on return. **Proof:** visibility test preserves session ownership and resumes polling once visible.
23. **Define page exit policy.** On non-persisted `pagehide`, close promptly; on a page eligible for back/forward cache, suspend UI work and revalidate on `pageshow`. **Proof:** persisted and non-persisted navigation tests diverge correctly.
24. **Build a media sink adapter.** Keep `HTMLMediaElement` binding outside the core: set `srcObject`, configure `playsInline`, request `play()`, classify rejection, and detach on stop. **Proof:** adapter test distinguishes successful play, policy rejection, and decode/unsupported failure.
25. **Gate audible playback on a user action.** Render a clear start/resume action when `play()` is blocked; do not loop automatic play attempts. **Proof:** blocked autoplay reaches `playback-blocked`, one gesture retries once, success reaches `playing`.
26. **Handle track interruption.** Surface mute/ended and resume/replace events without declaring immediate transport failure. **Proof:** a transient mute does not trigger reconnect; ended required media follows the documented grace policy.
27. **Handle audio routing limitations.** Use default output everywhere and expose output-device selection only where capability detection and secure-context policy support it. **Proof:** unsupported browsers omit the command without breaking playback.

### Phase 4 — connection statistics

28. **Define a normalized stats model.** Include timestamp, connection/ICE state, selected candidate pair when available, RTT, inbound bitrate, packets lost/received, jitter, decoded/dropped frames, frame size/rate, freeze indicators when available, and codec. **Proof:** model tolerates absent/vendor-varying reports.
29. **Implement `getStats()` parsing.** Correlate inbound RTP, remote inbound RTP, codec, transport, and nominated/selected candidate-pair reports by IDs rather than report order. **Proof:** Chromium-, Firefox-, and WebKit-shaped fixtures normalize to expected snapshots.
30. **Compute rates from deltas.** Store prior samples per SSRC/report identity, reject counter resets and nonpositive intervals, and mark insufficient data. **Proof:** unit tests cover first sample, normal delta, reset, and source replacement.
31. **Bound polling cost.** Poll at a documented foreground cadence, reduce/stop while hidden or disconnected, ensure one poll in flight, and cancel on teardown. **Proof:** fake-clock test never overlaps polls.
32. **Expose diagnostic snapshots safely.** Provide copyable redacted diagnostics without credentials, full IP addresses, or stable cross-session identifiers. **Proof:** privacy test scans exported diagnostics for forbidden fields.

### Phase 5 — reconnect and recovery

33. **Classify failures.** Separate playback-policy, signaling, ICE/transport, authentication/expiry, unsupported-browser, and terminal protocol failures. **Proof:** every error code maps to retryability and user action.
34. **Observe authoritative connection signals.** Use peer/ICE state and signaling state together; treat `disconnected` as a grace period and `failed` as recovery input. **Proof:** transient disconnect recovers without rebuilding; failed state starts recovery.
35. **Try ICE restart where negotiated by the protocol.** Request a fresh offer/restart through signaling and bind candidates to the new attempt. **Proof:** controlled ICE failure recovers without duplicate peer owners.
36. **Fall back to full session rebuild.** If restart is unsupported or times out, close the old owner before creating another. **Proof:** test asserts at most one live peer connection.
37. **Add bounded exponential backoff with jitter.** Reset after a stable connection, pause while offline, and cap attempts/time. **Proof:** fake-clock sequence matches lower/upper bounds and terminates.
38. **Make retry explicit after exhaustion.** Preserve safe share context and expose a user-triggered retry that starts a new epoch. **Proof:** exhausted → retry → signaling is a valid tested transition.
39. **Revalidate on browser return.** On `online`, `pageshow`, or visibility return, inspect actual connection state before reconnecting. **Proof:** healthy sessions do not reconnect merely because a lifecycle event fired.

### Phase 6 — share page behavior

40. **Define the share-page view model.** Cover loading, ready-to-start, connecting, waiting-for-media, playback-blocked, playing, recovering with attempt timing, expired/unauthorized, unsupported, failed, and closed. **Proof:** component story/fixture exists for every state.
41. **Make the primary action state-specific.** Use start, resume audio, retry, or request a new link; never show a generic button without consequence text. **Proof:** view-model tests map each actionable state to one command.
42. **Add minimal diagnostics disclosure.** Show human-readable quality/connection status and place detailed redacted stats behind an explicit disclosure/copy action. **Proof:** keyboard-only test can reach and copy diagnostics.
43. **Add accessible media/status semantics.** Provide logical focus order, visible focus, non-color status cues, restrained live-region updates, reduced-motion behavior, and touch targets. **Proof:** automated accessibility scan plus manual VoiceOver/TalkBack checklist.
44. **Add safe expiry and revocation handling.** Stop active media, clear retry timers, and replace content with a terminal explanation when the server revokes/expires a session. **Proof:** mid-session revocation immediately reaches terminal teardown.

### Phase 7 — Playwright gates

45. **Create a hermetic signaling/media harness.** Run a local test sender or controlled peer endpoint with deterministic fault injection. **Proof:** CI does not depend on public STUN/TURN or third-party services.
46. **Test the Chromium happy path.** Open a share URL, perform the user gesture, receive audio/video, assert playback evidence, stats movement, and clean close. **Proof:** trace/video retained on failure.
47. **Test autoplay rejection and recovery.** Force or simulate rejected `play()`, verify the resume action, then verify playback after a trusted click. **Proof:** no unhandled promise rejection.
48. **Test signaling and ICE disorder.** Delay candidates, duplicate messages, sever signaling, and induce peer failure. **Proof:** correct state sequence and no duplicate peer owners.
49. **Test offline/background/navigation lifecycle.** Toggle offline, change page visibility where automation supports it, exercise reload and back/forward navigation, and distinguish persisted-page behavior with targeted integration tests where automation cannot guarantee it. **Proof:** timers/listeners return to baseline.
50. **Add cross-engine smoke projects.** Run Chromium, Firefox, and WebKit for route, start gesture, remote-track binding, reconnect, and teardown. **Proof:** engine-specific expectations are documented, never silently skipped.
51. **Add security and privacy checks.** Assert share secrets do not appear in logs, HTML, referrers, diagnostics, screenshots, or analytics. **Proof:** artifact scanner passes.
52. **Define flake policy.** No blind retries for correctness tests; quarantine requires an owner, issue, expiry date, and retained trace. **Proof:** CI configuration enforces the metadata.

### Phase 8 — real-device release gates

53. **Create a device matrix.** At minimum: recent iPhone/iOS Safari, iPadOS Safari if supported, Android Chrome on a mid-tier device, macOS Safari, Windows Chromium, and desktop Firefox; include speaker/headphone/Bluetooth routes relevant to product scope. **Proof:** dated matrix records OS/browser/hardware.
54. **Run first-visit playback checks.** Test fresh profile/private context, locked/unlocked orientation where relevant, audible media, gesture requirement, mute switch/volume behavior, and screen lock/background transitions. **Proof:** signed checklist plus screen recording for failures.
55. **Run network transition checks.** Exercise Wi-Fi ↔ cellular, brief loss, captive/blocked UDP scenario where available, and TURN-only configuration. **Proof:** recovery time and resulting lifecycle state recorded.
56. **Run interruption checks.** Exercise phone/audio interruption where feasible, tab/app switching, Bluetooth connect/disconnect, and device sleep/wake. **Proof:** media resumes or exposes the documented recovery action.
57. **Run long-session checks.** Hold representative sessions for the product target duration and observe memory, battery/thermal behavior, stats stability, and reconnect count. **Proof:** timestamped metrics and pass thresholds attached.
58. **Gate release on zero critical gaps.** Block release for inability to start/resume media, unrecoverable common network transitions, secret leakage, duplicate audio, runaway retries, or teardown leaks. **Proof:** release record links all engine and device evidence.

## Recommended commit sequence

Each numbered task should normally be one commit. Keep contract/test commits before implementation commits; do not combine the Astro shell, WebRTC core, media adapter, stats parser, or reconnect state machine into one change. A phase can merge only when its listed proofs pass.

## Definition of done

- Astro builds and renders the share shell without evaluating browser-only code on the server.
- `web-rtc` is importable and testable independently of Astro and the chosen island framework.
- Lifecycle, media policy, stats, recovery, and teardown have deterministic state-machine coverage.
- Playwright passes its hermetic Chromium gate and cross-engine smoke gates.
- The dated real-device matrix passes, with exceptions explicitly owned and release-approved.
- No credentials or sensitive network identifiers leak through the share URL, markup, diagnostics, logs, analytics, or test artifacts.
