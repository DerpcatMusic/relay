# RELAY CI, Hardening, and Release Plan

**Date:** 2026-08-15  
**Status:** Proposed executable plan; validation notes are recorded in `docs/research/release-plan-validation.md`.  
**Scope:** CI, verification, artifact trust, release channels, promotion, and rollback. This document deliberately does not implement pipelines or releases.

## Goals and release policy

RELAY ships only from an immutable source commit after the same build outputs have passed progressively more expensive gates. A release candidate is promoted; it is not rebuilt. Every distributable must be traceable to its commit and workflow, signed where the platform supports signing, accompanied by an SBOM and provenance, and recoverable through a documented rollback.

The release manager owns the final promote/hold decision. Individual gate owners may block promotion. A failed required gate is never waived silently: any time-boxed exception needs a linked risk acceptance, named owner, expiry, and follow-up issue.

## Required check matrices

### Pull request matrix

Target completion: fast lane in 10 minutes; full required lane in 25 minutes. Cancellation/concurrency should stop superseded runs.

| Gate | Matrix / workload | Required | Owner | Failure action |
|---|---|---:|---|---|
| Repository hygiene | formatting, lint, generated-file drift, license/header and secret scan | Yes | Build/CI | Block merge |
| Core correctness | unit tests and contract tests on Linux, Windows, macOS; minimum supported toolchain plus current stable | Yes | Core | Block merge |
| Web/browser smoke | Chromium + Firefox + WebKit; connect, reconnect, media permission denial, device loss, two-peer session | Yes | Web | Block merge |
| Transport conformance | deterministic packet/vector tests for each enabled transport/provider; schema/version compatibility | Yes | Transport | Block merge |
| Plugin build smoke | Debug/validation build of supported plugin formats on native Windows/macOS runners; headless instantiate where possible | Yes | Plugin | Block merge |
| Security | dependency advisory scan, lockfile integrity, static analysis, forbidden capability checks | Yes | Security | Block merge |
| Packaging dry run | unsigned/non-notarized staging packages; manifest and install-layout verification | Yes for release-affecting paths | Release | Block merge |
| Changed-path integration | cross-component session setup when protocol, transport, plugin, web, or shared model changes | Yes when selected | Integration | Block merge |
| Coverage / sanitizer signal | coverage delta; Linux ASan/UBSan for changed native code | Yes, initially non-regression threshold | Core | Block merge or linked quarantine |

PR jobs must use least-privilege read-only tokens and must not receive signing or deployment secrets. Untrusted/fork code never executes on a privileged runner. Flaky-test reruns are recorded; two reruns may diagnose but never convert a real first failure into an unqualified green check.

### Nightly matrix

Target completion: 2 hours. Nightlies run from the protected default branch and retain machine-readable results and logs for trend analysis.

| Gate | Matrix / workload | Required for next RC | Owner | Failure action |
|---|---|---:|---|---|
| Full platforms/toolchains | supported Linux/Windows/macOS versions; minimum and current compiler/runtime | Yes | Build/CI | Open/refresh blocking issue |
| Browsers | current stable Chromium, Firefox, WebKit/Safari-equivalent coverage; webcam/mic permutations; background/foreground; reconnect | Yes | Web | Block RC cut |
| Transports | all supported transport/provider combinations; loss, duplication, reorder, jitter, bandwidth clamp, NAT/reconnect simulation | Yes | Transport | Block RC cut |
| DAW smoke | each supported OS × representative supported DAWs × plugin format; scan, instantiate, audio/MIDI I/O, state save/restore, reopen | Yes | Plugin QA | Block RC cut |
| Native hardening | ASan, UBSan, thread/concurrency instrumentation where viable; debug assertions | Yes | Core | Block RC cut |
| Web hardening | dependency/browser integration tests, accessibility smoke, CSP/security-header assertions on preview | Yes | Web | Block RC cut |
| Short soak | 2-hour multi-peer call/session with periodic reconnect, device change, and state churn | Yes | Reliability | Block RC cut after confirmation |
| Packaging | reproducible staging build comparison, manifest, installer install/uninstall/upgrade smoke | Yes | Release | Block RC cut |

### Weekly matrix

Target completion: 12 hours; failures page the owning team only after automatic infrastructure-failure classification.

| Gate | Matrix / workload | Required for stable promotion | Owner | Failure action |
|---|---|---:|---|---|
| Extended DAW qualification | full supported DAW/version/format matrix on clean machines, including project reopen and automation/state round-trip | Yes | Plugin QA | Block stable |
| Long soak | 24-hour session plus 8-hour high-churn scenario; memory, handle, thread, CPU and drift trend thresholds | Yes | Reliability | Block stable |
| Chaos | transport impairment, forced process/browser restart, signaling outage, dependency timeout, credential expiry, disk pressure | Yes | Reliability + Transport | Block stable |
| Performance | cold start, connect time, audio glitch/xrun rate, CPU, memory, bundle size and latency against stored baseline | Yes | Performance | Block stable on threshold regression |
| Compatibility | previous stable ↔ candidate protocol/session interoperability and installer upgrade/downgrade rehearsal | Yes | Integration | Block stable |
| Supply chain | full dependency/license review, SBOM policy validation, provenance verification, artifact malware scan | Yes | Security/Release | Block stable |
| Disaster rehearsal | restore prior stable pointers/downloads/config in staging; verify client recovery | Yes monthly; last success required | Release/SRE | Block stable if stale/failed |

## Gate definitions and acceptance criteria

### Transport gate

1. Maintain versioned, deterministic wire fixtures and negative fixtures.
2. Test every claimed provider/transport independently from any plugin-shell choice.
3. Required assertions: negotiation, ordering rules, duplicate handling, bounded queues/backpressure, reconnect/resume semantics, timeout behavior, authentication rejection, incompatible-version failure, and telemetry redaction.
4. Chaos profiles: 0/1/5/10% loss; 0/50/200 ms added latency; jitter; reorder; duplication; abrupt half-open disconnect; signaling outage; token expiry. Exact supported thresholds become release criteria after baseline collection, not arbitrary post-failure tuning.
5. Candidate must interoperate with the previous stable release or include an explicitly approved breaking-version and migration plan.

### Browser gate

1. Pin the supported browser policy in a single manifest and test the current stable releases represented by Chromium, Firefox, and WebKit; run real Safari checks on macOS before stable promotion if Safari is claimed.
2. Validate secure-context/media permission behavior, denial/revocation, no-device and device-switch flows, background throttling, refresh/rejoin, multiple tabs, network handoff, autoplay/audio-context recovery, and accessibility smoke.
3. Capture console errors, WebRTC stats, traces/screenshots on failure, and browser versions as artifacts without recording sensitive media.

### DAW and plugin gate

1. Keep a versioned support matrix listing OS, architecture, DAW version, plugin format, and status: supported, best-effort, or unsupported.
2. Automated smoke: plugin scan, instantiate, activate/deactivate, audio/MIDI pass, parameter automation, serialize/deserialize state, close/reopen, and clean unload.
3. Weekly clean-machine runs include installer install, upgrade from previous stable, uninstall, rescans, renamed/moved project, sample-rate and buffer-size changes, offline bounce if supported, and crash/hang collection.
4. Quarantined DAW cases require an owner and expiry and cannot be claimed supported while quarantined.

### Soak and performance gate

Record wall time, reconnect count, successful media/session time, underruns/xruns, latency distribution, CPU, resident memory, open handles, threads, queue depth, and drift. Establish baselines for two weeks, then encode thresholds. Immediate hard failures are any crash, deadlock, corrupted state, unbounded growth, credential leak, or unrecovered required connection. Trend regressions require triage before promotion.

### Chaos gate

Chaos runs only in isolated test accounts and networks. Every injected fault has an expected recovery invariant and maximum recovery time. The test must prove the injection occurred; otherwise it is inconclusive, not passing. Preserve a seed and timeline for replay. Never inject against production during release qualification.

## Artifact, signing, and notarization flow

1. **Build once:** a protected, pinned release workflow checks out the version tag/commit and produces immutable artifacts in isolated hosted or ephemeral runners. Dependencies and actions/tools are pinned and caches are treated as untrusted inputs.
2. **Inventory:** generate one SPDX or CycloneDX SBOM per artifact/package plus an aggregate release SBOM. Include bundled runtime dependencies; validate syntax and license/security policy.
3. **Attest:** emit signed build provenance binding source commit, workflow identity, build inputs, and artifact digests. Publish provenance and checksums beside artifacts.
4. **Sign:** use short-lived workload identity or hardware-backed/managed signing where available. No long-lived signing material is exposed to PR jobs. Windows installers/binaries are Authenticode-signed and timestamped. macOS apps, plugins, and installers are signed with the appropriate Developer ID identities and hardened-runtime/entitlement policy.
5. **Notarize macOS:** submit the final signed distributable to Apple notarization, wait for success, staple where supported, then verify signature, notarization assessment, and staple offline/online as applicable. Rejection blocks promotion.
6. **Verify from clean machines:** download by digest; verify checksum, signature, provenance, SBOM presence, archive/install layout, clean install, launch/scan, upgrade, uninstall, and malware scan.
7. **Publish atomically:** upload versioned immutable objects first; only then update channel metadata/pointers. Record the exact manifest of artifact names, sizes, SHA-256 digests, signature/provenance identifiers, and destination URLs.

Signing/notarization jobs are isolated approval environments with minimum permissions, protected reviewers, immutable logs, and no arbitrary shell steps after secrets/credentials become available. Logs must be checked for accidental credential or notarization-profile disclosure.

## Channels and promotion

| Channel | Audience | Source | Entry criteria | Retention / support |
|---|---|---|---|---|
| PR preview | reviewers only | merge request commit | PR required gates | Ephemeral; no compatibility promise |
| Canary | internal/team opt-in | protected default-branch commit | PR + relevant nightly gates; unsigned web preview allowed, native artifacts still signed if distributed | Keep recent builds; telemetry-heavy |
| Beta | external opt-in | immutable release candidate | all nightly gates; weekly critical gates current; signed/notarized artifacts and trust metadata | Maintain current and previous beta |
| Stable | general users | promoted beta artifacts, never rebuild | release checklist; weekly/DAW/soak/chaos/supply-chain gates; approvals | Maintain current plus at least two prior stable artifact sets |
| Hotfix | affected stable users | branch from affected stable tag | scoped PR gates, regression proof, security/release approvals, targeted nightly/DAW smoke; complete signing/trust flow | Promote through beta when time permits; document any expedited gate and expiry |

Version identifiers and update metadata must distinguish channels. A client moves to a less stable channel only by explicit opt-in. Stable updates are staged (for example internal → 5% → 25% → 100%) with automatic pause signals and a release-manager hold between stages. Exact timing and health thresholds are recorded in the release checklist.

## Release execution checklist

### Cut candidate

- [ ] Release issue names release manager, deputy, gate owners, version, commit, and rollback target.
- [ ] Version/changelog/support matrix are reviewed; migration and compatibility notes exist.
- [ ] PR and current nightly gates pass; weekly gates are within their validity window.
- [ ] Protected tag/commit is created; release build runs in the protected environment.
- [ ] Artifact manifest, SBOM, checksums, provenance, signatures, and notarization results verify.
- [ ] Clean-machine install/upgrade/uninstall and browser/transport/DAW candidate smoke pass.
- [ ] Beta metadata is atomically updated to the already-built candidate.

### Promote stable

- [ ] Beta observation window completed with no unresolved stop signal.
- [ ] Required weekly soak/chaos/DAW/performance/supply-chain results are green and linked.
- [ ] Previous-stable compatibility and rollback rehearsal pass.
- [ ] Security, QA, release manager, and product owner approvals are recorded.
- [ ] Immutable candidate digests equal stable artifacts; there was no rebuild.
- [ ] Stable rollout begins in stages; dashboards, alerts, on-call and status communication are ready.

## Stop signals and rollback

Stop or pause rollout for signature/provenance verification failure, crash/hang or corrupted state, auth/security incident, protocol incompatibility, material DAW scan/load regression, failed update/install, sustained SLO breach, or unexplained telemetry loss.

Rollback is a forward control-plane action, not deletion or mutation:

1. Freeze promotion and preserve logs, manifests, metrics, and affected artifacts.
2. Repoint channel/update metadata atomically to the last-known-good immutable version; invalidate only channel metadata caches, never replace versioned artifacts in place.
3. Disable server-side features through pretested kill switches when client rollback alone cannot restore compatibility.
4. Confirm fresh clients resolve and verify the prior manifest, and confirm active cohorts recover.
5. Communicate impact, affected versions, workaround, and next update. Keep the withdrawn build available only where needed for forensics; mark it revoked so clients do not select it.
6. Branch a hotfix from the affected stable tag or last-known-good base as appropriate. Run the scoped gates plus the complete artifact trust flow.
7. Complete incident review, identify detection/escape points, and add a regression test before resuming rollout.

Database/protocol changes must be expand/contract and backward compatible across at least the staged rollout and rollback window. Any irreversible migration requires separate approval, tested restoration, and blocks automatic rollback until proven safe.

## Small executable tasks and ownership

Tasks are intentionally sized to one focused pull request (roughly half a day to two days) and can proceed in parallel after the policy files land.

| ID | Task / output | Owner | Depends on | Done when |
|---|---|---|---|---|
| CI-01 | Add `ci/support-matrix` data file for OS/toolchain/browser/DAW/format claims | QA lead | None | Schema-reviewed; docs render from or link to it |
| CI-02 | Define required PR check names and branch protection checklist | CI owner | CI-01 | Names are stable and ownership documented |
| CI-03 | Add PR path-to-test mapping document | Integration owner | CI-02 | Protocol/shared changes cannot skip cross-component tests |
| CI-04 | Specify fork/untrusted workflow permission model | Security owner | CI-02 | Threat review proves no privileged secret/runner exposure |
| CI-05 | Define flaky-test quarantine record (owner, issue, expiry, claimed-support effect) | QA lead | None | CI can report quarantine without hiding first failure |
| BR-01 | Create browser scenario manifest and evidence-retention contract | Web owner | CI-01 | Each supported browser maps to required scenarios/artifacts |
| TR-01 | Version transport fixtures and impairment profile manifest | Transport owner | CI-01 | Every claimed transport maps to positive/negative/chaos cases |
| DAW-01 | Inventory supported clean-machine DAW images/licenses/runners | Plugin QA | CI-01 | Feasible automation vs manual cases are explicit |
| DAW-02 | Define plugin scan/instantiate/state/unload harness contract | Plugin owner | DAW-01 | Pass/fail and crash artifact formats are agreed |
| REL-01 | Define canonical release artifact manifest format | Release owner | CI-01 | Names, digests, platform/arch/format, channel are represented |
| REL-02 | Select SBOM formats/tools and policy checks | Security owner | REL-01 | Sample output validates and includes bundled dependencies |
| REL-03 | Specify provenance generator/verifier and consumer verification command | Security owner | REL-01 | Sample attestation binds commit/workflow/artifact digest |
| REL-04 | Document Windows signing identity, timestamping, custody, and verification | Release owner | REL-01 | Clean-machine verification procedure is approved |
| REL-05 | Document macOS identities, entitlements, notarization, stapling, verification | macOS owner | REL-01 | Each artifact type has exact sign/notarize/verify sequence |
| REL-06 | Design protected release environment, approvals, and least privileges | CI + Security | CI-04, REL-03..05 | Threat model and access owners approve |
| REL-07 | Specify immutable storage layout and atomic channel manifest | Release/SRE | REL-01 | Staging test can promote/repoint without rebuilding |
| REL-08 | Define staged rollout cohorts, health thresholds, hold times, and stop signals | SRE/Product | REL-07 | Checklist contains measurable promote/pause decisions |
| SOAK-01 | Define metrics schema and two-week baseline collection | Performance owner | TR-01 | Dashboard distinguishes leaks/drift/glitches/recovery |
| SOAK-02 | Specify deterministic 2h/24h soak workloads and success thresholds | Reliability owner | SOAK-01 | Seeded workload is replayable and has hard failures |
| CHAOS-01 | Write fault catalog with injection proof and recovery invariants | Reliability owner | TR-01 | Each fault has seed, expected recovery, max duration |
| RB-01 | Write staging rollback game-day runbook | SRE owner | REL-07, REL-08 | Prior channel is restored and verified end-to-end |
| RB-02 | Define compatibility/migration rollback window policy | Architecture owner | TR-01, REL-08 | Protocol/schema changes state reversible boundaries |
| OBS-01 | Define CI/release evidence retention and redaction policy | Security/SRE | REL-06 | Retention durations, access, PII/media exclusions are explicit |
| DOC-01 | Create release issue/checklist template linking every required result | Release manager | All specifications | A dry-run RC has no unowned or ambiguous checkbox |

## Initial sequencing

1. **Policy (week 1):** CI-01..05, REL-01, OBS-01.
2. **Gate contracts (week 1–2):** BR-01, TR-01, DAW-01..02, SOAK-01, CHAOS-01.
3. **Trust design (week 2):** REL-02..07; perform sample offline verification before automation.
4. **Promotion safety (week 2–3):** REL-08, RB-01..02, SOAK-02.
5. **Dry run (week 3):** DOC-01 and one non-public release candidate; record timing, failures, missing evidence, and revise thresholds.
6. **Enforcement:** enable required checks and stable promotion only after the dry run demonstrates the gate and rollback paths.

## Evidence record

Every candidate’s release issue links to immutable or retention-controlled evidence for each gate: workflow run, environment/tool versions, test summary, failure/quarantine list, soak/chaos metrics, artifact manifest, checksums, signatures, notarization result, SBOM, provenance verification, approvals, channel update, rollout observation, and rollback target. Evidence containing logs or telemetry follows the redaction and retention policy; public trust artifacts remain downloadable with the release.
