# RELAY Web Plan Validation

## Scope

Validate `docs/plans/2026-08-15-relay-web-plan.md` against a maximum of four targeted primary sources covering Astro islands and browser WebRTC/autoplay behavior. This task does not modify the plan or code.

## Validation Criteria

- Astro island/client-loading claims are accurate for the proposed shell boundary.
- ICE candidate ordering, generation handling, and recovery-state assumptions match WebRTC platform behavior.
- Browser autoplay assumptions and user-activation recovery are accurate.
- Plan tasks and proofs in those focused areas are mutually consistent.

This is a focused platform-evidence check, not exhaustive validation of all 58 tasks. Route deployment, signaling-schema details, statistics fields, page lifecycle/bfcache, security headers, device routing, accessibility, and CI/device matrices are outside this four-source review.

## Source Table

| # | Primary source | Plan area | Evidence captured |
|---|---|---|---|
| 1 | [Astro: Islands architecture](https://docs.astro.build/en/concepts/islands/) (accessed 2026-08-15) | Astro shell and hydration boundary | Astro renders UI components to HTML/CSS without client JavaScript by default. An explicitly marked interactive UI component uses a `client:*` directive, and only marked components load client JavaScript. |
| 2 | [MDN: Autoplay guide for media and Web Audio APIs](https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Autoplay) (accessed 2026-08-15) | Remote media playback and recovery UX | Scripted `play()` outside user input is autoplay; audible media is generally restricted, while muted/inaudible media is not. The `play()` promise must be observed; `NotAllowedError` identifies policy/permission denial and can trigger a play control. Prior interaction or allowlisting may permit autoplay. |
| 3 | [W3C WebRTC: `RTCPeerConnection.addIceCandidate()`](https://www.w3.org/TR/webrtc/#dom-peerconnection-addicecandidate) (accessed 2026-08-15) | ICE ordering and attempt/generation safety | `addIceCandidate()` rejects with `InvalidStateError` while `remoteDescription` is null. Candidate `usernameFragment` identifies the ICE generation; a fragment not present in an applied remote description rejects with `OperationError`. Empty candidate strings represent end-of-candidates. |
| 4 | [MDN: `iceconnectionstatechange`](https://developer.mozilla.org/en-US/docs/Web/API/RTCPeerConnection/iceconnectionstatechange_event) (accessed 2026-08-15) | Recovery classification and ICE restart | Normal state sequences are not rigid (`connected` may be skipped); `disconnected` is transient in the documented restart case; an ICE restart returns connectivity checking to progress; exhausted candidate checks lead to `failed` or `completed`, with end-of-candidates affecting that determination. |

## Findings

### Astro shell/island boundary

**Supported.** The plan’s central boundary—mostly static Astro output with explicitly scoped browser JavaScript—is consistent with Astro’s islands model. The source directly supports keeping unmarked components server/static and limiting client JavaScript to explicitly interactive pieces.

**Qualification.** Astro’s islands page describes `client:*` hydration for supported UI-framework components. The plan deliberately avoids adding React/Svelte/Vue and proposes a bundled module script instead. That is compatible with the architectural goal, but it should not call that plain script itself a hydrated Astro island. “Browser-only entry” or “scoped client module” is the precise term unless a framework component is introduced.

### Autoplay and `play()` recovery

**Supported with a wording correction.** Tasks 24–25 correctly put the media element in a browser adapter, observe the `play()` result, distinguish policy denial from other playback failure, and expose one user-driven recovery action rather than retrying indefinitely. MDN explicitly recommends handling the returned promise and treating `NotAllowedError` as permission/policy denial.

**Potential overstatement.** The heading “Gate audible playback on a user action” is stricter than the browser rule and stricter than the task’s own body. Browsers can allow audible autoplay after prior interaction, allowlisting, or policy delegation. The implementation may attempt `play()` and only require a trusted action after policy rejection. The master plan should describe the state as “recover policy-blocked audible playback with a user action,” not imply that every successful receive session must always begin with a gesture.

### ICE ordering and generations

**Supported.** Task 17’s buffering requirement is directly justified: the normative WebRTC algorithm rejects `addIceCandidate()` when `remoteDescription` is null. Its stale-attempt protection is also directionally correct because the standard models ICE generations and checks the candidate `usernameFragment` against applied remote descriptions.

**Needs precision.** “Reject candidates belonging to a superseded attempt” conflicts slightly with the task’s proof, which says the stale fixture is “ignored.” The core should discard/ignore a stale signaled candidate before calling `addIceCandidate()` (and optionally record diagnostics), while reserving rejection/error handling for invalid current-attempt input. Candidate buffering should explicitly include the empty-string end-of-candidates signal so ordering does not lose completion markers.

### ICE state and recovery

**Supported.** Tasks 34–35 correctly avoid treating every `disconnected` transition as immediate terminal failure and treat `failed` as recovery input. The source explicitly describes `disconnected` as transient in an ICE-restart scenario and documents that restart re-enters connectivity checking. The plan’s use of multiple authoritative state inputs is appropriately more defensive than assuming one fixed ICE sequence.

**Test implication.** Tests must not require `connected` to occur on every successful attempt; MDN notes that a valid sequence may move from `checking` directly to `completed`. The plan’s table-driven lifecycle tests should model semantic outcomes, not one universal browser event sequence.

## Explicit Potential Corrections to the Master Plan

1. Clarify that a plain Astro-bundled module script is a scoped browser entry, not a `client:*` hydrated island.
2. Retitle/reword task 25 so user activation is required after policy rejection, rather than claiming every audible playback start is unconditionally gesture-gated.
3. In task 17, replace ambiguous “reject” with “discard/ignore before `addIceCandidate()`” for candidates from superseded attempts, and explicitly preserve/order end-of-candidates markers alongside ordinary candidates.
4. Ensure lifecycle/recovery fixtures do not mandate a single ICE success sequence or require the `connected` state; `checking → completed` is valid.

## Decisions Reflected in the Plan

The following plan decisions are supported or appropriately conservative relative to the reviewed sources:

- **Static-first Astro boundary:** keep routes/document structure in Astro and ship client JavaScript only on the receiver route.
- **No framework solely for hydration:** use a small scoped browser module unless a UI framework is chosen for independent reasons.
- **DOM/media adapter outside the core:** bind `HTMLMediaElement`, call and observe `play()`, and translate browser-specific failures at the app boundary.
- **Explicit blocked-playback state:** surface a resume action after autoplay-policy rejection and avoid automatic retry loops.
- **Candidate ordering:** buffer remote candidates until a remote description exists.
- **Attempt/generation isolation:** prevent candidates and asynchronous completion from old attempts from mutating the current one.
- **Nonterminal disconnect handling:** allow a grace/revalidation path for `disconnected`; use actual peer/ICE/signaling state before rebuilding.
- **Layered recovery:** attempt protocol-supported ICE restart, then perform a bounded full rebuild with one live peer owner.
- **Browser variability as a release concern:** combine deterministic fakes with cross-engine and physical-device gates rather than encoding a single event sequence.

### Focused verdict

**Validated with four targeted corrections.** The sampled Astro, autoplay, and WebRTC design is technically sound. None of the findings requires changing the proposed package seam or phase order. Corrections are precision changes to terminology, autoplay wording, candidate/end-marker handling, and test expectations. The unreviewed plan areas listed under the criteria remain unvalidated by this evidence file rather than implicitly approved.

## Validation Proof

- Evidence file created before plan/source inspection.
- Plan/code changes: none. The plan SHA-256 before and after review is `092ee01dd87df81beb92c8acdef393e5fdc88130a3ebc4ececac6c4608180b80`.
- Required evidence sections are present exactly once; the source table contains exactly four numbered source rows.
- Primary-source count: 4 of 4 maximum. No additional sources were consulted.
- Source 1 fetched successfully over HTTPS (HTTP 200); relevant rendered text was inspected before its update.
- Source 2 fetched successfully over HTTPS (HTTP 200); relevant rendered text was inspected before its update.
- Source 3 fetched successfully over HTTPS (HTTP 200); the normative method algorithm was inspected before its update.
- Source 4 fetched successfully over HTTPS (HTTP 200); usage notes and state-transition text were inspected before this update.
