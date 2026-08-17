# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

Musicians and mix engineers who need to hear a DAW insert on another machine or a phone in the same session. Primary scene: two people on a LAN, or one person sending a named listen link to someone in the house / next room.

## Product Purpose

RELAY is a low-latency insert: send the track that is playing, hear it on another RELAY instance or a browser listen page. Success is audio that stays up, stays in time enough to play along on LAN, and does not require an account.

## Positioning

Unofficial assumption, from the existing repo and this request: RELAY is the Matari Audio communications plugin in the same product line as BUFFR. LAN is uncompressed 5 ms PCM. The website is a named listen fan-out, not the musician path. SonoBus-like P2P on the LAN; not a billed LISTENTO clone.

## Operating Context

Loaded as a CLAP/VST3 insert in Bitwig (and other hosts). Link hosts a named session; Join attaches by name or `host:port`. Browser listen is `relay.matari-audio.com/<session>`. Paid TURN, subscriptions, and production WebRTC are out of scope until the transport bake-off names a winner.

## Capabilities and Constraints

- Wire clock is 48 kHz stereo after SRC. Host rate and block are inferred from the DAW.
- Codecs on the LAN/plugin wire: Opus (default 192 kbps), FLAC 16-bit, PCM 16-bit.
- Web listen is event-only: one claim HTTP, one `/in` and `/out` WebSocket, room snapshot on connect/leave/settings. Media is 20 ms PCM only while a listener is connected. Browser decodes i16 directly (WebCodecs Opus packets were silent without OpusHead). No `/info` `/pcm` `/ctrl` polls.
- Silence DTX: one `dtx` event, then nothing; one `go` event on resume.
- No billing, no paid TURN in this product slice.

## Brand Commitments

- Vendor: **Matari Audio**. Product name stays **RELAY**.
- Visual authority pinned by the user: **BUFFR** (studio utility, tactile knobs, Polar Night / Studio Blue family) and **Plugcat** widgets.
- Voice: direct, technical, tactile. Studio tool, not a marketing surface.

## Evidence on Hand

- Master plan and progress: `docs/plans/2026-08-15-relay-master-plan.md`, `docs/plans/PROGRESS.md`
- LAN/plugin path: `docs/research/connect-stream-plugin-path.md`
- ADRs 0001–0006 (Rust core, 48 kHz clock, Opus, transport bake-off still open)
- Installed binaries: `~/.clap/RELAY.clap`, `~/.vst3/RELAY.vst3`
- Live listen: https://relay.matari-audio.com

## Product Principles

- Deepen the session/listen seam; do not add billing or WebRTC until the bake-off.
- The plugin editor is the product surface musicians live in. Hierarchy: status → route → session → codec → levels.
- LAN first. The website is a listen accessory and must not spend Durable Object quota on polls.
- Same control language as the rest of the Matari line: Plugcat knobs, segmented rows, stereo dB meter.

## Accessibility & Inclusion

Readable contrast on Polar Night, labels that are not color-only, fixed-size knobs that do not jump while dragging.
