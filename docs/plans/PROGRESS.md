# RELAY delivery progress

**Persistent goal:** build RELAY through validated release; do not declare completion while required gates remain.

| Phase | State | Evidence / next gate |
|---|---|---|
| Master architecture | Complete | `2026-08-15-relay-master-plan.md` |
| Phase 0 foundation slice | Validated | `../research/phase-0-integration.md` |
| Phase 0 CI / testkit / contracts / bootstrap | Implemented locally; hosted gate open | MPL-2.0 and cargo-deny pass; CI includes pinned cargo-deny/release gates; first hosted three-OS run and tracked Git baseline remain |
| Phase 1 audio engine/lab | Local automated gates pass | Reviewed primitives/TX/RX/playback, loopback, 60 s media, finite drain, corrected 12-hour matrix, callback-safety audit, and locked workspace fmt/check/test/clippy/deny all pass locally; independent 12-hour re-review, hosted three-OS CI, and physical-device smoke remain |
| Phase 2 transport bake-off | Fake Gate 0 pass; provider bake-off open | T0 fixtures, provider-neutral seam, T1a, and T1b independently pass (43 tests; send/backpressure, TURN/TLS policy, stats, lifecycle, timeout/fatal/drop seams); all three pinned dossiers and no-winner comparison exist; reproducible builds, browser/Coturn/adverse-network/teardown probes, and selection remain open |
| Standalone Connect | Local + LAN PCM path works | 5 ms uncompressed PCM, name join, `relay-connect`; hosted three-OS P2P and NAT/TURN remain |
| Browser Link | Deployed listen page | https://relay.matari-audio.com/`<name>` Worker + Durable Object; WebRTC still open |
| Plugin shell | Installed for local test | `~/.clap/RELAY.clap` + `~/.vst3/RELAY.vst3` + standalone; host scan/signing matrix remains |
| TURN fallback | Deferred | Paid TURN/subscriptions are out of scope |
| Stream fan-out | Local unpaid hub works | `relay-session` hub + two listeners + `apps/stream` CLI; billed SFU remains out of scope |
| Authentication | Not started | Requires anonymous direct flow first |
| Credits/billing | Not started | Requires paid route beta |
| Hardening/release | Not started | Requires complete product feature gates |

## Current execution rule

Work only on the earliest open gate unless a parallel task is contract-only and cannot create false completion. Every task must write research evidence, list potential corrections, and record exact validation.
