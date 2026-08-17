# Connect, Stream, and plugin path — Research and implementation evidence

**Date:** 2026-08-17  
**Task owner:** agent  
**Status:** Complete (local unpaid path; no subscriptions)

## Scope

Ship a working native Connect (DAW↔DAW P2P), unpaid local Stream fan-out, and a Truce plugin adapter around the same session engine. Billing, credits, paid TURN, browser Link, and hosted WebRTC provider selection are explicit non-goals.

## Acceptance criteria

- [x] Two session engines exchange stereo Opus over localhost UDP (Connect).
- [x] A local Stream hub fans one producer to two listeners.
- [x] Audio callback never drives sockets or the codec; a worker thread does.
- [x] A Truce plugin shell can host the same engine (CLAP/VST3/standalone).
- [x] A late-joining listener hears a host whose RTP clock has already advanced.
- [x] Local named-session web listen works via `relay-link` HTTP + PCM (not browser UDP).
- [x] No subscription, credit, or paid-TURN code is required to use these paths.

## Sources consulted

| Source | Why it is authoritative | Accessed |
|---|---|---|
| `crates/relay-audio/src/rx.rs` tick contract | Caller-paced deadline API; drain-until-None invents PLC forever | 2026-08-17 |
| `docs/plans/2026-08-15-relay-plugin-shell-plan.md` | Plugin is an adapter; process() is RT-safe | 2026-08-17 |
| Truce 7.0 `PluginLogic` / worker examples | Host callback vs background worker split | 2026-08-17 |

## Findings

`RxWorker::tick` always pops a deadline. An empty rebased reorder buffer yields `MissingAtDeadline`, not `None`. A `while let Some(tick())` loop therefore PLC-decodes forever. The worker must tick once per received media datagram, matching the designed one-frame lookahead.

Standalone Connect and Stream reuse `SessionEngine`. The plugin splits it with `into_parts()` / `SessionRuntime`: `CallbackFace` stays on the host callback; `SessionWorker` owns UDP and Opus.

The Truce plugin crate lives in `apps/plugin` as its own Cargo workspace (`exclude`d from the RELAY workspace) so Truce's custom license and FFI `unsafe` do not break `cargo deny` or the workspace `unsafe_code = forbid` lint.

## Potential corrections to the master plan

- Native RELY UDP is the V1 media plane for unpaid Connect/Stream. WebRTC bake-off remains open for NAT/TURN later; it is not a blocker for localhost or same-LAN use.
- Plugin-shell plan Gate P0 required a tagged cross-OS standalone Connect report. Product direction overrides that sequencing for a working local plugin; hosted three-OS evidence is still outstanding.
- Stream V1 is a local hub, not a billed SFU.

## Decisions applied

- RELY v1 datagrams (`Hello` / `HelloAck` / `Subscribe` / `Publish` / `Goodbye` / `Media`).
- RX epoch adopted on first media packet only, so Hello does not reset the timeline to sequence 0. `adopt_remote` takes that packet's RTP timestamp; a hardcoded 0 rejected live late-join media.
- `SessionRuntime` polls lock-free link/role/port plus a mutex-protected peer string on the worker; the callback never locks.
- Plugin persist stores the peer `host:port` and session slug. The worker re-claims the slug to `127.0.0.1:8787` every 2s while linked.
- Local Link is HTTP (`GET /{name}` player + `GET /{name}/pcm` i16le). Browsers cannot speak RELY UDP.

## Validation evidence

```text
$ cargo test -p relay-session --all-targets -- --test-threads=1
wire::tests::media_round_trips ... ok
two_connect_peers_exchange_stereo_opus_on_localhost ... ok
listen_only_guest_hears_host_capture ... ok
late_join_guest_hears_live_host ... ok
loopback_hears_own_opus_round_trip ... ok
stream_hub_fans_producer_to_two_listeners ... ok
into_parts_keeps_callback_independent_of_drive ... ok
split_runtime_exchanges_stereo_on_localhost ... ok

$ cargo clippy -p relay-session --all-targets -- -D warnings
Finished `dev` profile
```

```text
$ cargo test --manifest-path apps/plugin/Cargo.toml --all-targets
tests::bus_config_effect ... ok
tests::info_is_valid ... ok
tests::peer_persist_round_trips ... ok
tests::has_editor ... ok
tests::state_round_trips ... ok
tests::process_is_allocation_free ... ok
tests::dry_unlinked_passthrough ... ok
```

Installed locally (2026-08-17): `~/.clap/RELAY.clap`, `~/.vst3/RELAY.vst3`, standalone. LAN path: 5 ms i16 PCM, `push_direct` (no Opus/FEC lookahead), Who/Announce name join — `lan_pcm_loopback_is_immediate` and `lan_name_join_finds_host` pass. Deployed `relay-link` Worker: https://relay.matari-audio.com custom domain + `relay-link.djderpcat.workers.dev`. Claim smoke: POST `/api/claim` → `{"ok":true}`. Cross-NAT / paid TURN still not claimed.
