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

Loaded as a CLAP, VST3, VST2, LV2, or AU (v2/v3 on macOS) insert in Bitwig and other hosts. **Share** hosts a named session and copies a listen link. **Join** attaches by name or `host:port`. LAN PCM and the public listen page are both on whenever Share is live; idle with nobody connected does not send audio. Browser listen is `relay.matari-audio.com/<session>`: Cloudflare carries SDP/ICE only; audio is plugin→browser WebRTC P2P (10 listeners max). Same-/24 browsers hop to the plugin LAN page and hear local PCM. Paid TURN is still out of scope. Session names are three real words (`big-filthy-papaya`).

## Capabilities and Constraints

- Wire clock is 48 kHz stereo after SRC. Host rate and block are inferred from the DAW.
- Codecs on the LAN/plugin wire: Opus (default 192 kbps), FLAC 16-bit, PCM 16-bit.
- Web listen signaling is event-only: one claim HTTP, one `/in` and `/out` WebSocket for SDP/ICE (cap 10 listeners). Media is P2P WebRTC (plugin sendonly Opus audio track, STUN `stun.cloudflare.com`). The browser plays a native `MediaStream` on `<audio>`. The plugin uses libdatachannel behind `relay-transport` (libnice when the linked `.so` has it; this host's package is juice, so STUN/TURN-UDP only). Cloudflare never sees PCM. PCM stays on the LAN listen socket. Symmetric NAT without a hole punch will fail until TURN exists. No `/info` `/pcm` `/ctrl` polls.
- Silence DTX: after ~400 ms of already-quiet audio, fade 20 ms, one `dtx` event, then nothing; one `go` plus fade-in on resume. The plugin pings the `/in` socket only while silent (PCM already keeps the socket alive). Host reset at the same sample rate keeps the session engine and fan-out; a worker panic reconnects without tearing the DAW callback. The editor shows **asleep** and can wake the stream.
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

- Deepen the session/listen seam. Native WebRTC stays behind `relay-transport`; do not add billing or paid TURN in this slice.
- The plugin editor is the product surface musicians live in. Hierarchy: Share/Join → session name → levels. Status is Ready / Live / Asleep, not “streaming”.
- LAN and the listen page stay armed together. The website is a listen accessory and must not spend Durable Object quota on polls.
- Same control language as the rest of the Matari line: Plugcat knobs, Phosphor icons, vertical stereo dB meters.

## Accessibility & Inclusion

Readable contrast on Polar Night, labels that are not color-only, fixed-size knobs that do not jump while dragging.
