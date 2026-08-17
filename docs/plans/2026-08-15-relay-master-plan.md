# RELAY — Master Architecture & Implementation Plan

**Status:** Proposed architecture, August 15, 2026
**Product:** RELAY
**Goal:** Build the lowest-friction professional audio transport product where a musician can stream DAW audio directly to another DAW or any browser, preferably P2P at zero media-server cost, with paid infrastructure used only when it provides real value: hostile-network traversal or scalable fan-out.

---

# 1. What RELAY actually is

RELAY is **not** primarily a streaming service and it is **not** primarily a plugin.

The actual product is:

> **A low-latency real-time audio transport engine with native DAW, browser and server adapters.**

That distinction determines the architecture.

The plugin is one adapter.

The browser player is one adapter.

Cloudflare is one transport provider.

The SFU is one topology.

None of those should own RELAY.

The permanent part should be:

```text
                    RELAY CORE

          Audio clocking / buffering
                     +
             Codec / packet policy
                     +
            Session state machine
                     +
             Transport abstraction
                     +
             Routing abstraction

                         │
          ┌──────────────┼──────────────┐
          │              │              │
       Plugin          Browser        Server
```

The user-facing product has three core experiences.

### RELAY Connect

```text
DAW / Plugin
     │
     │ WebRTC
     │
     ▼
DAW / Plugin
```

Bidirectional, SonoBus-like native connection.

Default:

* direct P2P
* free
* stereo Opus
* extremely small buffer
* remote monitoring
* no server audio

If direct P2P cannot work:

```text
Plugin
   │
   ▼
Cloudflare TURN
   │
   ▼
Plugin
```

Paid relay fallback.

---

### RELAY Link

```text
DAW / Plugin
      │
      │ direct WebRTC
      ▼
     Browser
```

Producer creates:

```text
relay.audio/r/7hd92kd...
```

Listener opens it.

No plugin.

No download.

No account required for the listener.

Preferably no account required for the sender for basic free use either.

---

### RELAY Stream

```text
                     ┌── Browser
                     ├── Browser
Plugin ──► SFU ──────┼── Browser
                     ├── Browser
                     └── Browser
```

Paid.

One producer upload.

Potentially very large listener count.

This is where infrastructure provides real value.

---

# 2. Product philosophy

There should be five fundamental rules.

### Rule 1 — direct first

If two endpoints can communicate directly:

**do not put RELAY infrastructure in the audio path.**

That keeps your marginal media cost at essentially zero.

### Rule 2 — servers solve problems, not subscriptions

Users pay when RELAY infrastructure actually does something:

* TURN traversal
* SFU fan-out
* perhaps recording/transcoding later

Not simply because they use the plugin.

### Rule 3 — no networking vocabulary in normal UX

Never make ordinary users choose:

```text
STUN
TURN
ICE
TURN-TLS
SFU
SRTP
Candidate
NAT
```

Internally, yes.

Externally:

```text
Connection

● Direct
  Free

or

● RELAY
  Server-assisted
```

### Rule 4 — latency cannot grow without explanation

A generic VoIP solution is often willing to continuously enlarge a jitter buffer to keep audio glitch-free.

RELAY should instead optimize for **bounded latency**.

A concealed/lost audio packet is sometimes preferable to mysteriously gaining another 100 ms of latency.

### Rule 5 — frameworks are replaceable

This is the biggest codebase rule:

```text
Truce                 replaceable
libdatachannel        replaceable
Cloudflare            replaceable
LiveKit               replaceable
WorkOS                replaceable
Paddle/Stripe         replaceable
Astro                 replaceable
```

while:

```text
relay-domain
relay-audio
relay-engine
relay-protocol
```

remain.

---

# 3. Stack I would lock today

## Core decision table

| Area               | Primary                                                    | Alternative                     | Status       |
| ------------------ | ---------------------------------------------------------- | ------------------------------- | ------------ |
| Core language      | **Rust**                                                   | C++                             | 🔒 Lock      |
| Architecture       | **Monorepo + ports/adapters**                              | Polyrepo                        | 🔒           |
| Plugin framework   | **Truce**                                                  | JUCE                            | 🟡 Validate  |
| Native WebRTC      | **Transport bake-off, libdatachannel+libnice currently A** | Shiguredo libwebrtc / webrtc-rs | 🟡 Gate      |
| Codec              | **libopus 1.6.x**                                          | none serious                    | 🔒           |
| RT SPSC            | **rtrb**                                                   | custom SPSC                     | 🔒           |
| Adaptive SRC       | **Rubato 4 Async**                                         | soxr/libsamplerate              | 🟡 Benchmark |
| Browser            | **native WebRTC**                                          | none                            | 🔒           |
| Web app            | **Astro 7**                                                | SvelteKit                       | 🔒           |
| Signaling          | **Cloudflare Worker + Durable Objects**                    | Rust/Axum                       | 🔒           |
| Free NAT discovery | **Cloudflare STUN**                                        | Google/self-hosted              | 🔒           |
| Paid relay         | **Cloudflare TURN**                                        | coturn/Twilio/etc.              | 🔒           |
| Paid SFU           | **Cloudflare Realtime**                                    | LiveKit                         | 🟡 CF beta   |
| Persistent DB      | **D1**                                                     | Postgres                        | 🟡           |
| Plugin auth        | **WorkOS Device Flow**                                     | Better Auth custom device flow  | 🟡           |
| Payments           | **Paddle + internal credit ledger**                        | Stripe                          | 🟡           |
| JS package manager | **pnpm workspace**                                         | Bun/npm                         | 🔒           |
| Rust tests         | **cargo-nextest**                                          | cargo test                      | 🔒           |
| Coverage           | **cargo-llvm-cov**                                         | llvm tools directly             | 🔒           |
| Dependency audit   | **cargo-deny**                                             | manual                          | 🔒           |

Truce is unusually well matched: its current documentation covers CLAP, VST3, LV2, AUv2, AUv3, AAX and standalone from Rust, including iOS AUv3, and explicitly documents lock/allocation-free `process()` behavior. It currently requires Rust 1.92+. ([truce.audio][1])

The important caveat is that Truce is young. That is why **Truce must only be the plugin shell**. JUCE remains the boring fallback with broad desktop/mobile plug-in support; upstream NIH-plug is currently explicitly in maintenance mode. ([GitHub][2])

---

# 4. Native WebRTC should deliberately remain acceptance-gated

I would slightly modify our previous “libdatachannel is locked” conclusion.

The architecture is locked.

**The implementation is not.**

We now have three serious candidates.

## Candidate A — libdatachannel + libnice

Current leader.

libdatachannel supports native WebRTC media transport on Windows, macOS, Linux, iOS and Android and interoperates with Chromium, Firefox and Safari. Its C API is narrow enough to hide behind a Rust wrapper. ([GitHub][3])

The specific reason for using **libnice rather than libjuice** is hostile networks: libdatachannel documents client→TURN connections over TCP/TLS when using its libnice backend. ([GitHub][4])

That's extremely important because Cloudflare exposes:

```text
STUN       3478/UDP and 53/UDP
TURN UDP   3478/UDP and 53/UDP
TURN TCP   3478/TCP and 80/TCP
TURN TLS   5349/TCP and 443/TCP
```

So TCP 443 becomes the “this awful corporate network still lets HTTPS-like outbound traffic through” fallback. ([Cloudflare Docs][5])

### Its weakness

libnice introduces another native dependency and licensing/packaging review. It is dual-licensed LGPL-2.1-or-later **or MPL-1.1**, so the release process needs an explicit compliance review rather than blindly statically linking everything. ([GitHub][6])

That's a release gate, not something to discover two days before launch.

---

## Candidate B — Shiguredo `shiguredo_webrtc`

This is an interesting newer contender.

Rather than reimplementing WebRTC, it gives Rust a safe wrapper over Google's libwebrtc and currently ships prebuilt native libraries. The current 0.150.x line was active through July 2026, and its API includes ICE, RTP, stats and TURN-TLS-related functionality. ([Docs.rs][7])

Advantages:

```text
Google's WebRTC behavior
TURN-TLS
lots of interoperability history
Rust-facing API
prebuilt libwebrtc
```

Disadvantages:

```text
much larger dependency
more internal threading
less control
larger binary
more complicated encoded-audio integration
current documented platform matrix is narrower
```

I would absolutely include it in the torture test.

---

## Candidate C — webrtc-rs

This is the architectural beauty candidate.

The new `rtc` is pure-Rust Sans-I/O and includes ICE/STUN/TURN/DTLS/SRTP/RTP/SCTP and media tracks, while the async `webrtc` 0.20 line is now built around that core. ([GitHub][8])

Its ecosystem even has a Sans-I/O SFU. ([GitHub][9])

It is exactly what I would eventually *like* RELAY to run.

But this is a connectivity product, so elegance doesn't win automatically.

### Therefore

Create:

```text
crates/relay-transport/
```

and make this interface permanent.

Then:

```text
crates/relay-transport-libdatachannel/
crates/relay-transport-webrtcrs/       experimental
crates/relay-transport-libwebrtc/      experimental
```

The winner is selected empirically.

---

# 5. Monorepo structure

This is the structure I would start with.

```text
relay/
│
├── README.md
├── LICENSE
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
│
├── package.json
├── pnpm-workspace.yaml
├── pnpm-lock.yaml
│
├── justfile
├── deny.toml
├── nextest.toml
├── typos.toml
├── .editorconfig
├── .gitignore
│
├── .cargo/
│   └── config.toml
│
├── .github/
│   ├── CODEOWNERS
│   │
│   └── workflows/
│       ├── ci-rust.yml
│       ├── ci-web.yml
│       ├── ci-native.yml
│       ├── ci-contracts.yml
│       ├── ci-security.yml
│       ├── nightly-netlab.yml
│       ├── nightly-soak.yml
│       └── release.yml
│
├── apps/
│   │
│   ├── plugin/
│   │   ├── Cargo.toml
│   │   ├── truce.toml
│   │   │
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── plugin.rs
│   │       ├── processor.rs
│   │       ├── params.rs
│   │       ├── state.rs
│   │       ├── bridge.rs
│   │       └── ui/
│   │           ├── mod.rs
│   │           ├── app.rs
│   │           ├── connection.rs
│   │           ├── meters.rs
│   │           └── settings.rs
│   │
│   ├── web/
│   │   ├── astro.config.mjs
│   │   ├── package.json
│   │   │
│   │   └── src/
│   │       ├── pages/
│   │       │   ├── index.astro
│   │       │   ├── pricing.astro
│   │       │   ├── r/
│   │       │   │   └── [session].astro
│   │       │   └── account/
│   │       │       └── index.astro
│   │       │
│   │       ├── components/
│   │       ├── layouts/
│   │       └── styles/
│   │
│   └── control-plane/
│       ├── package.json
│       ├── wrangler.jsonc
│       │
│       ├── migrations/
│       │   ├── 0001_initial.sql
│       │   ├── 0002_credits.sql
│       │   └── 0003_usage.sql
│       │
│       └── src/
│           ├── index.ts
│           │
│           ├── api/
│           │   ├── sessions.ts
│           │   ├── auth.ts
│           │   ├── routes.ts
│           │   ├── billing.ts
│           │   └── health.ts
│           │
│           ├── durable/
│           │   ├── session-do.ts
│           │   └── account-meter-do.ts
│           │
│           ├── providers/
│           │   ├── turn/
│           │   │   ├── provider.ts
│           │   │   └── cloudflare.ts
│           │   │
│           │   ├── fanout/
│           │   │   ├── provider.ts
│           │   │   ├── cloudflare.ts
│           │   │   └── livekit.ts
│           │   │
│           │   ├── auth/
│           │   │   ├── provider.ts
│           │   │   └── workos.ts
│           │   │
│           │   └── billing/
│           │       ├── provider.ts
│           │       ├── paddle.ts
│           │       └── stripe.ts
│           │
│           ├── db/
│           │   ├── users.ts
│           │   ├── sessions.ts
│           │   ├── usage.ts
│           │   ├── ledger.ts
│           │   └── purchases.ts
│           │
│           ├── security/
│           │   ├── tickets.ts
│           │   ├── rate-limit.ts
│           │   └── redaction.ts
│           │
│           └── observability/
│               ├── logs.ts
│               └── metrics.ts
│
├── crates/
│   │
│   ├── relay-domain/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ids.rs
│   │       ├── mode.rs
│   │       ├── route.rs
│   │       ├── quality.rs
│   │       ├── capability.rs
│   │       └── error.rs
│   │
│   ├── relay-protocol/
│   │   └── src/
│   │       ├── lib.rs
│   │       └── generated.rs
│   │
│   ├── relay-rt/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── input_ring.rs
│   │       ├── output_ring.rs
│   │       ├── counters.rs
│   │       └── snapshot.rs
│   │
│   ├── relay-opus/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── encoder.rs
│   │       ├── decoder.rs
│   │       ├── packet.rs
│   │       └── config.rs
│   │
│   ├── relay-resample/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── fixed.rs
│   │       └── adaptive.rs
│   │
│   ├── relay-clock/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── drift.rs
│   │       ├── pll.rs
│   │       └── estimator.rs
│   │
│   ├── relay-jitter/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── buffer.rs
│   │       ├── arrival.rs
│   │       ├── policy.rs
│   │       └── concealment.rs
│   │
│   ├── relay-audio/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── tx.rs
│   │       ├── rx.rs
│   │       ├── packetizer.rs
│   │       ├── depacketizer.rs
│   │       └── profile.rs
│   │
│   ├── relay-transport/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── peer.rs
│   │       ├── candidate.rs
│   │       ├── media.rs
│   │       ├── stats.rs
│   │       └── capabilities.rs
│   │
│   ├── relay-libdatachannel-sys/
│   │   ├── build.rs
│   │   └── src/lib.rs
│   │
│   ├── relay-transport-libdatachannel/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── peer.rs
│   │       ├── track.rs
│   │       ├── ice.rs
│   │       ├── callbacks.rs
│   │       └── error.rs
│   │
│   ├── relay-signaling/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs
│   │       ├── websocket.rs
│   │       ├── resume.rs
│   │       └── auth.rs
│   │
│   ├── relay-fanout/
│   │   └── src/
│   │       ├── lib.rs
│   │       └── provider.rs
│   │
│   ├── relay-fanout-cloudflare/
│   │
│   ├── relay-engine/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs
│   │       ├── command.rs
│   │       ├── event.rs
│   │       ├── state.rs
│   │       ├── session.rs
│   │       ├── peer.rs
│   │       ├── routing.rs
│   │       └── worker.rs
│   │
│   ├── relay-telemetry/
│   │
│   └── relay-testkit/
│       └── src/
│           ├── fake_clock.rs
│           ├── fake_transport.rs
│           ├── fake_signaling.rs
│           ├── network_model.rs
│           └── audio_source.rs
│
├── packages/
│   │
│   ├── protocol/
│   │
│   ├── web-rtc/
│   │   └── src/
│   │       ├── peer.ts
│   │       ├── signaling.ts
│   │       ├── stats.ts
│   │       ├── playback.ts
│   │       └── capabilities.ts
│   │
│   ├── ui/
│   └── config/
│
├── proto/
│   ├── buf.yaml
│   ├── buf.gen.yaml
│   │
│   └── relay/
│       └── v1/
│           ├── common.proto
│           ├── signaling.proto
│           ├── capabilities.proto
│           └── telemetry.proto
│
├── openapi/
│   └── relay-v1.yaml
│
├── third_party/
│   ├── README.md
│   ├── manifest.toml
│   ├── libdatachannel/
│   ├── patches/
│   └── licenses/
│
├── tools/
│   ├── xtask/
│   ├── relay-probe/
│   ├── relay-netlab/
│   └── relay-audio-lab/
│
├── tests/
│   ├── protocol/
│   ├── audio/
│   ├── transport/
│   ├── browser/
│   ├── netlab/
│   ├── plugin-hosts/
│   ├── soak/
│   └── fixtures/
│
├── infra/
│   ├── cloudflare/
│   ├── dev/
│   └── scripts/
│
└── docs/
    ├── architecture/
    ├── protocols/
    ├── adr/
    ├── runbooks/
    └── plans/
```

That is a **real monorepo**, not a directory with unrelated projects dumped into it.

---

# 6. Dependency direction

This rule needs to be enforced mechanically.

```text
relay-domain
     ▲
     │
relay-protocol
     ▲
     │
     ├──────── relay-audio
     │             ▲
     │             │
     └──────── relay-transport
                   ▲
                   │
              relay-engine
                   ▲
          ┌────────┼─────────┐
          │        │         │
       Plugin    Probe     Mobile
```

Forbidden:

```text
relay-audio → Truce
relay-engine → Cloudflare
relay-domain → Tokio
relay-transport → WorkOS
relay-engine → Astro
relay-opus → libdatachannel
```

Correct:

```text
Plugin imports relay-engine

relay-transport-libdatachannel
implements relay-transport

relay-fanout-cloudflare
implements relay-fanout
```

This is classic **hexagonal/ports-and-adapters architecture**, but without turning it into enterprise abstraction soup.

---

# 7. `relay-domain`: the boring center

`relay-domain` should contain nothing asynchronous, nothing platform-specific and almost no third-party dependencies.

Core types:

```rust
pub enum SessionMode {
    Connect,
    Link,
    Stream,
}

pub enum MediaRoute {
    Direct,
    TurnRelay,
    Sfu,
}

pub enum PaidFallbackPolicy {
    Never,
    Ask,
    Auto,
}

pub enum ConnectionState {
    Idle,
    Creating,
    Signaling,
    Connecting,
    Connected,
    Recovering,
    Closing,
    Closed,
    Failed,
}
```

Quality:

```rust
pub enum QualityProfile {
    UltraLowLatency,
    Balanced,
    Stable,
    Custom(AudioProfile),
}
```

Media:

```rust
pub struct AudioProfile {
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub bitrate_bps: u32,
    pub frame_duration: FrameDuration,
    pub fec: FecPolicy,
    pub dtx: bool,
}
```

V1 canonical media clock:

```text
48,000 Hz
Stereo
Opus
DTX off
```

libopus 1.6.1 is the current stable release, and the 1.6 API contains in-band FEC and newer DRED support. ([opus-codec.org][10])

Don't put DRED into V1 simply because it is new.

---

# 8. Real-time audio architecture

This is arguably the most important subsystem.

## Never do this

```text
DAW process()
     │
     ├ Opus encode
     ├ WebRTC
     ├ WebSocket
     ├ mutex
     ├ logging
     └ allocation
```

Absolutely not.

## Sender

```text
DAW AUDIO THREAD
      │
      │ copy
      ▼
bounded SPSC
      │
      ▼
MEDIA WORKER
      │
      ├ sample-rate conversion
      ├ packet accumulation
      ├ Opus encoding
      ▼
encoded frame queue
      │
      ▼
TRANSPORT
      │
      ▼
WebRTC
```

`rtrb` is specifically designed as a realtime-safe SPSC ring buffer. ([Docs.rs][11])

The callback contract is:

```text
no heap allocation
no mutex
no filesystem
no networking
no waiting
no logging
no async
```

If the outgoing queue is full:

```text
DROP NEW INPUT
increment overrun counter
continue
```

Do not block.

Do not dynamically grow the queue.

Do not permit backpressure to become latency.

---

# 9. Receiver

```text
WebRTC
   │
   ▼
RTP packets
   │
   ▼
reorder/depacketize
   │
   ▼
adaptive encoded jitter buffer
   │
   ▼
Opus decode
FEC / PLC when required
   │
   ▼
adaptive sample-rate converter
   │
   ▼
bounded output SPSC
   │
   ▼
DAW AUDIO THREAD
```

If the receiver underruns:

```text
short fade → zero
```

not:

```text
reuse random stale buffer
click violently
block DAW
```

When samples recover:

```text
short fade in
```

---

# 10. Why there are two resampling problems

These must not be conflated.

### Sender rate conversion

If the DAW runs:

```text
44.1 kHz
96 kHz
192 kHz
```

RELAY converts it to the network clock:

```text
48 kHz
```

This is ordinary fixed-ratio SRC.

### Receiver clock recovery

Suppose:

```text
remote interface ≈ 48,001 Hz
local interface  ≈ 47,999 Hz
```

Both nominally say 48 kHz.

They're still not the same clock.

Eventually the receiving buffer will fill or drain.

That requires an **asynchronous SRC** whose ratio is continuously nudged by a clock-recovery controller.

Rubato 4 explicitly exposes asynchronous resamplers whose ratio can be changed while running and recommends its Async family where input/output clocks may drift relative to each other. ([Docs.rs][12])

So:

```text
RTP timestamp progression
          +
local monotonic audio clock
          +
jitter-buffer fill error
          │
          ▼
       DriftEstimator
          │
          ▼
           PLL
          │
          ▼
     adaptive ratio
    0.99998 / 1.00003
          │
          ▼
        ASRC
```

That is a first-class RELAY subsystem.

Not a utility function.

---

# 11. `relay-clock`

Keep the controller isolated.

```text
relay-clock/
├── estimator.rs
├── pll.rs
└── drift.rs
```

Inputs:

```text
RTP timestamp
packet arrival monotonic time
decoded sample count
current output-ring fill
target output-ring fill
```

Output:

```text
resample_ratio_adjustment_ppm
```

Normal correction should be intentionally slow.

The network jitter buffer should handle **network timing**.

The ASRC should handle **clock drift**.

Do not let ASRC chase individual packet jitter.

---

# 12. Adaptive jitter controller

Don't build:

```rust
VecDeque<Packet>
```

and call it finished.

`relay-jitter` owns:

```text
sequence reorder
late packet detection
loss classification
burst classification
arrival jitter
target buffer calculation
buffer shrink policy
FEC policy hints
```

Conceptually:

```text
Network observations
       │
       ├ RTT
       ├ arrival variance
       ├ loss %
       ├ burst loss
       ├ reorder
       └ late packets
              │
              ▼
       Latency Policy
              │
       ┌──────┼────────┐
       ▼      ▼        ▼
    target   FEC     bitrate
    delay   policy    policy
```

### Important behavior

Increase buffer relatively rapidly when required.

Decrease it **slowly**.

This prevents:

```text
20 → 60 → 20 → 60 → 20 ms
```

oscillation.

---

# 13. Initial latency profiles

These should be treated as tested policy ranges, not marketing guarantees.

### Connect — Ultra

```text
Opus packet       5 ms
Jitter target     ~10–25 ms
Priority          latency
```

### Connect — Balanced

```text
Opus packet       10 ms
Jitter target     ~20–50 ms
Priority          stability/latency
```

### Stable

```text
Opus packet       10–20 ms
Jitter target     ~40–100 ms
Priority          bad network
```

### Browser Link / Stream

Start with:

```text
48 kHz
stereo
Opus
10 ms packetization
192–256 kbps
```

because browser playback ultimately has its own WebRTC jitter behavior.

Do not advertise 20 ms latency because you sent 5 ms Opus frames.

---

# 14. Audio encoding should happen once per source

For multiple listeners:

Wrong:

```text
PCM
 ├ encode listener A
 ├ encode listener B
 ├ encode listener C
 └ encode listener D
```

Correct:

```text
PCM
 │
 ▼
ONE OPUS ENCODER
 │
 ▼
EncodedAudioFrame
 │
 ├ peer A
 ├ peer B
 ├ peer C
 └ peer D
```

Each WebRTC connection still performs its own RTP/SRTP handling, but expensive audio encoding does not need to repeat.

---

# 15. Native Connect audio flow

This needs a feedback-safe signal path.

```text
DAW input
   │
   ├────────────────────────────► local output
   │
   └────► RELAY transmit tap
```

Incoming remote:

```text
remote receive
      │
      ▼
monitor gain
      │
      ▼
local output mix
```

Critically:

```text
remote return
```

must **not** automatically re-enter the outgoing send tap.

Otherwise:

```text
A → B → A → B → A
```

feedback loop.

The send tap should therefore be **pre-remote-monitor**.

---

# 16. Plugin must remain zero-latency locally

The network path is out-of-band.

Local DAW audio should be:

```text
input → output
```

without waiting for network audio.

Therefore RELAY should report:

```text
0 samples plugin latency
```

unless some future DSP feature explicitly introduces latency.

Remote monitoring is asynchronous and should not cause DAW PDC.

---

# 17. Plugin lifecycle rules

These are product-security rules as much as technical rules.

RELAY must:

* never auto-start broadcasting when a DAW project is opened;
* never put authentication credentials into the DAW project state;
* never persist an active public share token in host automation;
* never perform networking during offline rendering;
* gracefully suspend when the host stops processing;
* tolerate block-size changes;
* tolerate sample-rate changes;
* tolerate plugin editor close/reopen;
* continue audio networking if the editor closes;
* stop cleanly when the instance is destroyed.

Only user preferences belong in plugin state.

For example:

```text
quality preference
remote-monitor level
paid fallback preference
UI size
```

Not:

```text
access_token
refresh_token
TURN password
live session secret
```

---

# 18. Transport interface

The core should know almost nothing about WebRTC.

Conceptually:

```rust
pub trait PeerTransport {
    fn configure(&mut self, config: PeerTransportConfig)
        -> Result<(), TransportError>;

    fn start(&mut self)
        -> Result<(), TransportError>;

    fn add_remote_candidate(
        &mut self,
        candidate: IceCandidate,
    ) -> Result<(), TransportError>;

    fn set_remote_description(
        &mut self,
        description: SessionDescription,
    ) -> Result<(), TransportError>;

    fn send_audio(
        &mut self,
        frame: EncodedAudioFrame,
    ) -> Result<(), TransportError>;

    fn restart_ice(
        &mut self,
    ) -> Result<(), TransportError>;

    fn stats(&self) -> TransportStats;
}
```

The actual API will likely be event-driven rather than exactly this synchronous interface.

The important thing is that these types contain **no libdatachannel pointers**.

---

# 19. Unsafe code quarantine

Every normal crate:

```rust
#![forbid(unsafe_code)]
```

The only exceptions:

```text
relay-libdatachannel-sys
future platform FFI crates
```

`unsafe` never leaks upward.

Structure:

```text
C API
 │
 ▼
relay-libdatachannel-sys
 │       unsafe
 ▼
relay-transport-libdatachannel
 │       safe Rust
 ▼
relay-transport trait
```

Callbacks must convert:

```text
C pointer
 ↓
validated owned handle
 ↓
bounded event
```

No C callback gets to mutate `relay-engine` directly.

---

# 20. Thread ownership

Avoid the classic:

```text
Arc<Mutex<Everything>>
```

architecture.

Use explicit ownership.

### Audio thread

Owns:

```text
host buffers
SPSC producer/consumer ends
atomic RT counters
```

### Media worker

Owns:

```text
OpusEncoder
OpusDecoder
Resampler
JitterBuffer
ClockRecovery
Packetizer
```

### Transport thread/runtime

Owns:

```text
PeerConnection
ICE state
RTP transport
network sockets
```

### Control/signaling worker

Owns:

```text
WebSocket
HTTP requests
session signaling state
auth refresh
```

### UI

Gets:

```text
immutable snapshots
engine events
```

and sends:

```text
EngineCommand
```

No shared mutable god object.

---

# 21. Engine command/event API

Example commands:

```rust
pub enum EngineCommand {
    StartConnect,
    StartLink,
    StartStream,

    JoinSession(SessionCode),

    Stop,

    SetQuality(QualityProfile),
    SetMonitorGain(f32),

    AllowPaidFallback,
    DenyPaidFallback,

    RefreshAuth,
}
```

Events:

```rust
pub enum EngineEvent {
    StateChanged(ConnectionState),

    PeerJoined(PeerInfo),
    PeerLeft(PeerId),

    RouteChanged(MediaRoute),

    StatsUpdated(SessionStats),

    PaidFallbackRequired(FallbackQuote),

    Error(RelayError),
}
```

This makes:

```text
Plugin UI
CLI
mobile app
tests
```

all control the same engine.

---

# 22. Connection routing state machine

Something like:

```text
IDLE
 │
 ▼
SESSION_CREATED
 │
 ▼
SIGNALING
 │
 ▼
DIRECT_CONNECTING
 │
 ├──── success ───► DIRECT
 │
 └──── failure
          │
          ▼
    fallback allowed?
       /       \
      no       yes
      │         │
    FAILED      ▼
          TURN_CONNECTING
                 │
           ┌─────┴─────┐
           ▼           ▼
         TURN        FAILED
```

Stream mode:

```text
IDLE
 │
 ▼
STREAM_PROVISIONING
 │
 ▼
SFU_CONNECTING
 │
 ▼
SFU_CONNECTED
```

Recovery:

```text
CONNECTED
   │
network changes
   ▼
RECOVERING
   │
   ├ ICE restart
   ├ reconnect signaling
   └ preserve SessionId
```

---

# 23. Paid fallback UX

Three settings:

```text
Server fallback

○ Never
● Ask me
○ Automatically
```

`Automatically` additionally gets:

```text
Monthly/session spending limit
```

Never charge money merely because ICE happens to test a TURN candidate.

Bill based on the selected server-backed route.

---

# 24. Cloudflare TURN

This is one of the choices I'd genuinely lock.

Cloudflare's STUN is currently documented as free/unlimited, while TURN and Realtime SFU share a 1,000 GB free tier before the current $0.05/GB egress rate. ([Cloudflare Docs][13])

So RELAY can give everybody:

```text
stun.cloudflare.com
```

without putting audio through Cloudflare.

TURN credentials must come from the control plane.

Never:

```text
TURN secret key
inside plugin binary
```

Instead:

```text
Plugin
 │
 ▼
RELAY API
 │
 │ authenticated
 ▼
Cloudflare TURN credentials
 │
 ▼
short-lived client ICE config
```

---

# 25. RELAY Stream provider abstraction

Create:

```typescript
interface FanoutProvider {
  createPublisher(
    request: CreatePublisherRequest
  ): Promise<PublisherPlan>;

  createSubscriber(
    request: CreateSubscriberRequest
  ): Promise<SubscriberPlan>;

  closeParticipant(
    participantId: string
  ): Promise<void>;

  closeSession(
    sessionId: string
  ): Promise<void>;
}
```

Then:

```text
CloudflareFanoutProvider
LiveKitFanoutProvider
```

No:

```typescript
if (provider === "cloudflare")
```

spread throughout the application.

---

# 26. Why Cloudflare Realtime remains primary

A Cloudflare Realtime Session maps to one PeerConnection reaching a nearby Cloudflare data center, and the SFU exposes tracks as its basic unit. ([Cloudflare Docs][14])

Current limits include 50 Realtime API calls/second **per session**, 64 tracks in one API call, and no declared hard per-session track count beyond practical connection limits. ([Cloudflare Docs][15])

Current pricing is particularly favorable for audio:

```text
client → Cloudflare      free
Cloudflare → clients     billed

1,000 GB monthly free tier
then $0.05/GB
```

for Realtime SFU/TURN egress. ([Cloudflare Docs][13])

But Cloudflare's Realtime changelog still describes the platform as **open beta**, so I would not allow its API objects to leak into the rest of RELAY. ([Cloudflare Docs][16])

---

# 27. Why LiveKit stays

LiveKit is not the primary economic choice anymore.

It is **provider insurance**.

LiveKit Cloud currently advertises a 99.99% uptime target and has a mature managed deployment model. ([LiveKit Docs][17])

Before paid RELAY Stream reaches GA, implement enough of the LiveKit adapter to prove that:

```text
provider = Cloudflare
```

can become:

```text
provider = LiveKit
```

without rewriting the product.

It does not need automatic mid-stream failover in V1.

Provider switch can happen on reconnect.

---

# 28. Control plane architecture

Use:

```text
Cloudflare Worker
       │
       ├ HTTP API
       │
       └ Durable Objects
```

Not:

```text
one persistent VPS
```

### `SessionDO`

One per live RELAY session.

It owns:

```text
connected producer
connected listeners
peer roles
signaling revisions
SDP relay
ICE candidate relay
resume state
route state
presence
```

It carries **zero audio**.

Durable Objects are specifically intended to act as a single coordination point for multiple clients and support WebSocket Hibernation, where the object can sleep while sockets remain connected. ([Cloudflare Docs][18])

There can theoretically be tens of thousands of WebSockets associated with one DO, although practical CPU/memory limits are lower; RELAY shouldn't design around the headline maximum anyway. ([Cloudflare Docs][19])

---

# 29. WebSocket reconnection is mandatory

Durable Objects can be evicted/recreated and their lifecycle must not be treated as a permanent process. ([Cloudflare Docs][20])

Therefore every signaling client gets:

```text
session_id
peer_id
resume_token
last_revision
```

Reconnect:

```text
hello {
  session
  resume_token
  last_seen_revision
}
```

Server returns:

```text
welcome {
  current_revision
  missing_events[]
}
```

or tells client:

```text
full renegotiation required
```

---

# 30. Signaling wire protocol

I would use **Protocol Buffers** as the canonical WebSocket contract.

Not because bandwidth matters.

Because schema evolution matters.

```text
proto/relay/v1/
```

Example:

```protobuf
message Envelope {
  uint32 protocol_version = 1;
  string message_id = 2;
  string session_id = 3;
  string peer_id = 4;
  uint64 revision = 5;

  oneof payload {
    Hello hello = 10;
    Welcome welcome = 11;
    Offer offer = 12;
    Answer answer = 13;
    IceCandidate ice_candidate = 14;
    PeerJoined peer_joined = 15;
    PeerLeft peer_left = 16;
    RouteChanged route_changed = 17;
    Error error = 18;
  }
}
```

Generate:

```text
prost types → Rust
protobuf-es → TypeScript
```

CI runs schema-breaking checks.

Rules:

```text
Never reuse protobuf field numbers.
Never silently change field meaning.
Additive V1 changes only.
Breaking semantics → V2.
```

---

# 31. Capability negotiation

Version alone is not enough.

Peers advertise:

```protobuf
message Capabilities {
  repeated uint32 opus_frame_ms = 1;
  uint32 max_opus_bitrate = 2;
  bool inband_fec = 3;
  bool native_pcm = 4;
  bool turn_tls = 5;
  bool ice_restart = 6;
  uint32 max_audio_tracks = 7;
}
```

Then:

```text
old RELAY ↔ new RELAY
```

negotiates common behavior.

That lets experimental future features exist without breaking older clients.

---

# 32. HTTP API

Keep HTTP human-readable JSON and describe it with OpenAPI.

Initial endpoints:

```text
POST /v1/sessions
GET  /v1/sessions/:id

POST /v1/sessions/:id/join-ticket

POST /v1/sessions/:id/turn
POST /v1/sessions/:id/stream

POST /v1/billing/checkout
GET  /v1/billing/balance

POST /v1/webhooks/paddle
POST /v1/webhooks/stripe

GET  /v1/health
```

The browser/player never receives provider API secrets.

---

# 33. Anonymous free sessions

I would strongly recommend this.

First-run experience:

```text
Install RELAY
    ↓
Start Link
    ↓
Copy URL
```

No signup wall.

Anonymous sessions can:

```text
Direct P2P only
finite live lifetime
small direct-listener limit
no saved history
no server relay
no server stream
```

Sign-in becomes required when they want:

```text
TURN fallback
Stream
credits
saved account
usage history
```

That's dramatically better acquisition UX.

---

# 34. Authentication

For plugin authentication, **OAuth Device Authorization Flow** is almost perfect.

WorkOS AuthKit currently implements this exact flow: the application requests a device code, shows a code/browser link, and polls until the user completes authentication in their normal browser. ([WorkOS][21])

Plugin:

```text
Sign in
  ↓
Shows

ABCD-EFGH

[ Open Browser ]

  ↓
Browser login
  ↓
plugin receives token
```

Much better than embedding a web browser inside a DAW plugin.

Alternative:

**Better Auth** is attractive if you want to own auth and it supports Cloudflare Workers, but you'd need to design/secure the device authorization workflow yourself. ([Better Auth][22])

So:

```text
WorkOS = primary
Better Auth = sovereignty/cost fallback
```

---

# 35. Credential storage

Use OS credential storage.

```text
macOS      Keychain
Windows    Credential Manager
Linux      Secret Service
iOS        Keychain
Android    Keystore
```

Never serialize refresh tokens inside:

```text
.vstpreset
DAW project
plugin state
```

---

# 36. Database architecture

D1 is reasonable for V1 because RELAY's control-plane write rate should be low.

Cloudflare's D1 currently executes writes on a primary database and its Sessions API gives sequentially consistent reads when using read replicas. Each individual D1 database remains inherently single-threaded. ([Cloudflare Docs][23])

That means:

**do not write one billing row for every audio packet.**

Write meaningful events.

Tables:

```sql
users

devices

sessions

purchases

usage_events

credit_ledger

route_leases
```

---

# 37. Credit ledger

Never store:

```text
users.balance = 42.5
```

as the only authority.

Use append-only accounting.

```text
credit_ledger
────────────────────────────────────────
+10000   purchase
-60      relay usage
-60      relay usage
+10      refund
```

Every row:

```text
id
user_id
delta_units
reason
idempotency_key
purchase_id?
usage_event_id?
created_at
```

Money/credits:

**integers only.**

Never floats.

---

# 38. Usage unit

I would call it a **RELAY minute**.

Definition:

> One minute of one encoded audio stream delivered from RELAY infrastructure to one endpoint.

Examples:

```text
Plugin → TURN → Browser

1 wall-clock minute
= 1 RELAY minute
```

Bidirectional Connect:

```text
A → TURN → B
B → TURN → A

1 wall-clock minute
≈ 2 RELAY minutes
```

Stream:

```text
Plugin → SFU → 5 listeners

1 wall-clock minute
= 5 RELAY minutes
```

This maps intuitively to infrastructure cost without making musicians think in gigabytes.

---

# 39. Billing model

I would launch with **prepaid usage credits**, not open-ended postpaid billing.

Why:

```text
no surprise user bill
no surprise infrastructure bill
much easier fraud exposure
simple UX
```

User buys:

```text
1,000 RELAY minutes
5,000 RELAY minutes
20,000 RELAY minutes
```

Then balance counts down.

### Primary payments: Paddle

Paddle currently supports one-time credit packs/usage-based models and operates as merchant of record, including the tax/compliance layer for digital products. ([Paddle Developer Docs][24])

That makes it very attractive for worldwide software usage credits.

### Alternative: Stripe

Stripe has mature usage meters and prepaid Billing Credits, although the latter remain described as public preview in current documentation. ([Stripe Docs][25])

I therefore would **not make Stripe's credit-balance feature your accounting authority**.

Even with Stripe:

```text
Stripe handles money
RELAY ledger handles RELAY credits
```

---

# 40. Metering

Don't deduct every second with a database write.

Use leases.

Example:

```text
request server route
      ↓
reserve 60 RELAY seconds
      ↓
grant 60-second route lease
      ↓
near expiry
      ↓
renew another lease
```

If the session ends early:

```text
finalize actual duration
refund unused reservation
```

This limits both:

```text
write frequency
overspend exposure
```

`AccountMeterDO` serializes concurrent spending for one user.

That handles the case where somebody starts ten Stream sessions simultaneously.

---

# 41. Browser architecture

Astro remains the best fit.

The website is predominantly:

```text
marketing
pricing
docs
account
player shell
```

with one highly interactive real-time island.

Cloudflare officially documents Astro deployment on Workers, and Astro has a current v7 migration line. ([Cloudflare Docs][26])

But do **not** put the actual networking engine inside a Svelte/React component.

Use:

```text
packages/web-rtc
```

plain TypeScript.

---

# 42. Browser player

```typescript
const session = new RelayWebSession(...)

session.connect()
session.onStats(...)
session.onState(...)
```

UI framework merely renders its state.

Playback:

```text
WebRTC MediaStream
       │
       ▼
HTMLAudioElement
```

Prefer direct media-element playback rather than routing audible output through a large WebAudio graph.

If you want metering:

```text
track clone
   ↓
AudioContext
   ↓
AnalyserNode
```

but the audible path remains simple.

---

# 43. Web page UX

```text
┌────────────────────────────────┐
│ RELAY                          │
│                                │
│ Derpcat's Studio               │
│ ● LIVE                         │
│                                │
│          ▶ LISTEN              │
│                                │
│ Direct · 256 kbps · Stereo     │
│                                │
│ Connection: Excellent          │
└────────────────────────────────┘
```

Click is required anyway for reliable browser audio playback behavior.

Advanced diagnostics hidden under:

```text
Connection Details
```

not on the main screen.

---

# 44. Direct Link fan-out

The engine should technically support N direct peers.

But I would initially make the UX recommend Stream around **4 direct listeners**.

Not because four is a protocol limit.

Because:

```text
4 × 256 kbps ≈ manageable
20 × 256 kbps ≈ producer upload becomes significant
```

The engine should not hardcode four.

Server policy decides the free recommended threshold.

That means later:

```text
4 → 8
```

requires no plugin release.

---

# 45. Seamless Link → Stream upgrade

The share URL does not change.

```text
relay.audio/r/foo
```

begins:

```text
Producer ↔ Browser
```

If producer selects Stream:

```text
same URL
   ↓
listener reconnects
   ↓
Producer → SFU → Browser
```

The page should briefly show:

```text
Optimizing stream...
```

This is a key product-quality detail.

---

# 46. Security model

Direct P2P WebRTC provides encrypted media transport.

Server-routed WebRTC is also encrypted in transit, but an SFU generally terminates WebRTC security at the provider, so **do not market Stream as end-to-end encrypted** unless you later add a separate application-layer media encryption scheme.

Say:

```text
Encrypted connection
```

not:

```text
Nobody including RELAY can access it
```

until that is actually true.

---

# 47. Session IDs

Never:

```text
/stream/12345
```

Use high-entropy opaque IDs.

For example:

```text
/r/Pn39cE7FZk82vAY...
```

Separate:

```text
SessionId
JoinSecret
ResumeToken
```

Do not make one token serve every security role.

---

# 48. Logs must never contain

```text
access tokens
refresh tokens
TURN passwords
full SDP
raw ICE candidate IPs
payment details
join secrets
```

Redaction belongs in:

```text
apps/control-plane/src/security/redaction.ts
```

not developer discipline.

---

# 49. Observability

RELAY should be extremely observable because networking issues otherwise become impossible to diagnose.

Every endpoint should expose local diagnostics:

```text
Route
Direct / TURN / SFU

RTT
Jitter

Packets lost
Packets late

Jitter buffer
Current / target

Clock correction
+38 ppm

Audio queue
3.2 ms

Bitrate
247 kbps

Codec
Opus

ICE candidate type
host / srflx / relay

Transport
UDP / TCP / TLS
```

But ordinary UI reduces that to:

```text
Excellent
Good
Unstable
```

---

# 50. Separate diagnostics from telemetry

`SessionStats` exists locally regardless of whether analytics is enabled.

Telemetry export is separate.

```text
relay-telemetry
```

can turn a sanitized snapshot into analytics.

That ensures disabling analytics doesn't disable troubleshooting.

---

# 51. Error taxonomy

Don't show:

```text
ICE failed: 701
```

Define stable domain errors.

```text
DIRECT_CONNECTION_FAILED
NETWORK_BLOCKS_UDP
SERVER_RELAY_REQUIRES_CREDITS
REMOTE_VERSION_INCOMPATIBLE
AUDIO_QUEUE_OVERRUN
AUDIO_DEVICE_CLOCK_UNSTABLE
SIGNALING_RECONNECTING
STREAM_PROVIDER_UNAVAILABLE
```

Then the UI maps them to human language.

---

# 52. Plugin UI framework

I would initially use **Truce + egui or Truce's built-in UI**, because RELAY's interface is mostly:

```text
status
buttons
meters
settings
connection diagnostics
```

not a giant animated synthesizer.

Alternative:

```text
Slint
```

if you want much more layout/design richness.

Again, the UI only receives `EngineSnapshot` and emits `EngineCommand`.

No networking logic in widgets.

---

# 53. Plugin parameters

Do **not** expose every setting as a DAW automation parameter.

Good automation parameters:

```text
Remote Monitor Gain
Remote Monitor Mute
```

Maybe:

```text
Send Enable
```

although I'd be cautious about automatable broadcasting.

Not plugin parameters:

```text
Session ID
Account
TURN mode
Quality diagnostics
Sign in
Billing
URL
```

These are application state.

---

# 54. V1 plugin formats

Ship:

```text
Windows
  CLAP
  VST3

Linux
  CLAP
  VST3

macOS
  CLAP
  VST3
  AU
```

Later:

```text
LV2
AAX
```

AAX creates additional Avid/PACE process, and Truce's own install documentation calls out those separate SDK/signing requirements. ([truce.audio][1])

Don't expand the launch matrix unnecessarily.

---

# 55. iOS

Because the core is independent:

```text
relay-engine
     │
     ▼
Truce AUv3 shell
```

is plausible later.

Truce currently explicitly documents AUv3 on iOS/iPadOS. ([truce.audio][27])

iOS needs:

```text
container app
AUv3 extension
shared authenticated state
Keychain/App Group
```

Don't put that into desktop V1.

---

# 56. Android

Last.

The useful Android product initially is:

```text
RELAY standalone listener/transmitter
```

not trying to invent a standardized Android DAW plugin ecosystem.

The core remains usable.

---

# 57. Third-party dependency policy

Create:

```text
third_party/manifest.toml
```

Example concept:

```toml
[[dependency]]
name = "libdatachannel"
version = "0.24.x"
source = "..."
license = "MPL-2.0"
reason = "Native WebRTC transport"

[[dependency]]
name = "libnice"
version = "..."
reason = "ICE/TURN TCP/TLS backend"
```

Each native dependency requires:

```text
exact commit/tag
SHA
license
build flags
patches
upstream URL
upgrade notes
```

Never:

```text
Fetch main
```

during release builds.

---

# 58. Rust dependency policy

Workspace-level dependencies:

```toml
[workspace.dependencies]
serde = ...
thiserror = ...
tracing = ...
rtrb = ...
rubato = ...
```

Avoid crate-level random version divergence.

Policies:

```text
Cargo.lock committed
cargo-deny in CI
cargo-machete periodically
no wildcard versions
```

`cargo-nextest`, `cargo-deny`, and `cargo-llvm-cov` are all actively available in the current Rust toolchain ecosystem. ([Docs.rs][28])

---

# 59. Error-handling policy

Library crates:

```text
thiserror
typed errors
```

Application/tool crates:

```text
anyhow allowed at outer boundaries
```

Core rules:

```text
no unwrap() in production audio/network core
no expect() in packet parsers
no panic on remote input
```

Fuzz anything parsing:

```text
SDP wrappers
signaling messages
packet metadata
session tokens
```

---

# 60. Testing architecture

Testing is not a final phase.

Each subsystem should have a deterministic fake.

`relay-testkit` provides:

```text
FakeClock
FakeTransport
FakeSignaling
FakeNetwork
FakeAudioSource
FakeAudioSink
```

That lets this test run without the internet:

```text
create two engines
connect fake transport
inject 3% loss
inject +80 ppm clock drift
run 2 virtual hours
assert:
    no buffer runaway
    no deadlock
    no unbounded allocation
```

That is immensely valuable.

---

# 61. Audio unit tests

Test:

```text
44.1 → 48 kHz
48 → 48
96 → 48
192 → 48

48 → 44.1
48 → 48
48 → 96

mono
stereo

variable host block:
16
32
64
128
256
512
1024
```

Verify:

```text
no NaN
no clipping generated by SRC
correct frame count
bounded drift
```

---

# 62. Clock torture tests

Artificially drive clocks:

```text
-250 ppm
-100 ppm
-20 ppm
0 ppm
+20 ppm
+100 ppm
+250 ppm
```

Run:

```text
2 hours virtual time
12 hours virtual time
24 hours virtual time
```

Assertions:

```text
ring fill remains bounded
latency does not monotonically increase
no sample discontinuity outside recovery conditions
ASRC correction remains stable
```

---

# 63. Network torture laboratory

This should exist **before the real plugin is polished**.

`tools/relay-netlab`.

Linux namespaces + nftables + `tc netem`.

Test:

```text
0 ms jitter
5 ms
20 ms
80 ms

0% loss
0.1%
1%
3%
5%
10%

burst loss

20 ms RTT
60 ms
120 ms
200 ms
300 ms
```

NAT/firewall:

```text
open internet
CGNAT-like
endpoint-dependent mapping
UDP blocked
TURN UDP
TURN TCP
TURN TLS 443
IPv4 only
IPv6
dual-stack
```

---

# 64. Browser matrix

Automated:

```text
Chromium
Firefox
WebKit
```

with Playwright.

But Playwright WebKit is **not enough to claim Safari compatibility**.

Real machines:

```text
Safari macOS
Safari iOS
Chrome Android
```

remain release gates.

---

# 65. Transport bake-off

Before choosing libdatachannel permanently, make each candidate implement the same harness:

```text
relay-probe A
relay-probe B
```

Measurements:

| Metric                   | Weight |
| ------------------------ | -----: |
| Direct connectivity      |    20% |
| TURN/TLS connectivity    |    20% |
| Browser interoperability |    15% |
| Reconnect/ICE restart    |    10% |
| Added latency            |    10% |
| CPU                      |     5% |
| memory                   |     5% |
| binary size              |     5% |
| packaging difficulty     |     5% |
| licensing/compliance     |     5% |

Candidates:

```text
A libdatachannel + libnice
B Shiguredo libwebrtc
C webrtc-rs
```

The winner goes behind `relay-transport`.

This is much more defensible than falling in love with one library.

---

# 66. DAW host matrix

Before desktop stable:

### Linux

```text
Bitwig
REAPER
Ardour
```

### Windows

```text
Ableton
Bitwig
REAPER
FL Studio
Cubase
Studio One
```

### macOS

```text
Logic
Ableton
Bitwig
REAPER
Cubase
Studio One
```

Automated where possible:

```text
pluginval
clap-validator
auval
```

Truce explicitly includes validation and packaging tooling, which is one reason it's attractive. ([truce.audio][27])

---

# 67. Soak tests

A nightly dedicated runner should do:

```text
Plugin A
 ↕
Plugin B

12 hours
```

and:

```text
Plugin
 ↓
browser

12 hours
```

Measure:

```text
RSS memory
buffer fill
clock correction
packet loss
reconnections
CPU
handles/threads
```

Pass condition:

**no monotonic memory or latency growth.**

---

# 68. Chaos tests

Randomly:

```text
kill signaling WebSocket
restart SessionDO
change IP
force ICE restart
drop network 5 seconds
disable UDP
re-enable UDP
expire TURN credential
close browser tab
restart browser
suspend laptop
resume laptop
```

The result should normally be:

```text
Recovering...
Connected
```

not:

```text
restart your DAW
```

---

# 69. Performance budgets

Initial engineering budgets:

### Audio callback

At:

```text
48 kHz
64 samples
stereo
```

RELAY callback work should consume **well under 5% of the callback period** on your supported baseline hardware.

It should predominantly be:

```text
copy
mix remote buffer
atomic counters
```

### Heap

After `prepare()`:

```text
0 allocations/process callback
```

### Queue

Audio queues are bounded.

No network stall may create:

```text
500 ms
1 sec
10 sec
```

of queued audio.

### Memory

12-hour soak:

```text
no sustained upward RSS trend
```

---

# 70. Connection targets

These are product SLO targets, not physical guarantees.

Healthy network:

```text
Direct session setup:
target < 2 seconds

Server fallback:
target < 5 seconds

Signaling reconnect:
target < 3 seconds
```

International propagation itself cannot be eliminated.

RELAY's objective is to avoid **unnecessary software latency** on top of geography.

---

# 71. CI

## Pull request — Rust

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run
cargo llvm-cov
cargo deny check
```

## Pull request — web

```text
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

## Contracts

```text
protobuf lint
protobuf breaking check
OpenAPI validation
Rust ↔ TS golden message tests
```

## Native

Matrix:

```text
ubuntu
windows
macOS
```

Build:

```text
relay-engine
relay-probe
plugin
```

---

# 72. Nightly CI

Not PR-blocking:

```text
ASAN native FFI
UBSAN
fuzz
netem matrix
TURN/TLS test
browser interop
2h soak
dependency freshness
```

Weekly:

```text
12h/24h soak
full DAW matrix on dedicated machines
```

---

# 73. Release channels

```text
Canary
Beta
Stable
```

### Canary

Every main build.

### Beta

Signed, packaged, selected testers.

### Stable

Only after:

```text
transport matrix passes
host matrix passes
12h soak passes
TURN fallback passes
browser matrix passes
```

---

# 74. Platform releases

Artifacts:

```text
RELAY.clap
RELAY.vst3
RELAY.component
```

and installers.

Signing:

```text
macOS
Developer ID
notarization

Windows
Authenticode

Linux
checksums/signature
```

Truce already documents signing/notarization and package tooling, but RELAY should own its release pipeline rather than trust a magic CLI invocation forever. ([truce.audio][1])

---

# 75. SBOM and supply chain

Every release generates:

```text
SBOM
dependency license manifest
SHA-256 checksums
build provenance
```

Native libraries are explicitly included.

Pin:

```text
Rust
Cargo.lock
pnpm lock
libdatachannel commit
libnice version
Truce version
```

No auto-updating native dependencies during release.

---

# 76. Observability backend

Control plane:

```text
structured tracing
request ID
session ID
provider
route
duration
```

Never media payload.

Native diagnostic bundle can export:

```text
RELAY version
OS
plugin format
DAW
network route
WebRTC stats
buffer stats
clock ppm
error timeline
```

with an explicit:

```text
Copy diagnostics
```

button.

That will save immense support time.

---

# 77. Provider rollout

Cloudflare Realtime should not instantly receive 100% of paid traffic.

Rollout:

```text
internal
  ↓
5%
  ↓
25%
  ↓
50%
  ↓
100%
```

Provider assignment belongs server-side.

```text
FanoutRoutingPolicy
```

so you can route:

```text
Europe → CF
US → CF
specific failure cohort → LiveKit
```

without releasing another plugin.

---

# 78. D1 escape hatch

D1 is fine while control-plane writes remain modest.

If billing/storage growth eventually becomes awkward:

```text
DbRepository trait
       │
       ├ D1
       └ Postgres
```

But don't prematurely build both.

The SQL/domain boundary simply shouldn't depend on a D1-specific result type.

---

# 79. What should NOT be abstracted

Perfect modularity can become garbage if taken too far.

Do **not** write:

```text
AudioEncoderFactoryFactory
GenericDataRepositoryManager
AbstractSessionObjectInterface
```

Concrete core modules are good.

Abstract only boundaries that have genuine alternative implementations:

```text
transport
fanout provider
billing
auth
persistent storage
plugin shell
```

Do not abstract basic DSP math.

---

# 80. Naming conventions

Rust:

```text
RelayEngine
AudioTx
AudioRx
JitterBuffer
ClockRecovery
PeerTransport
FanoutProvider
SessionId
PeerId
TrackId
```

Avoid ambiguous:

```text
Manager
Helper
Utils
Processor2
HandlerThing
```

`utils.rs` should essentially not exist.

A function belongs to the domain that owns it.

---

# 81. File-size philosophy

Prefer files roughly:

```text
100–400 lines
```

where naturally possible.

If:

```text
engine.rs = 2,800 lines
```

the module decomposition failed.

If:

```text
20 files × 25 lines
```

the decomposition also failed.

Split on **responsibility**.

---

# 82. Documentation architecture

`docs/architecture/`:

```text
overview.md
audio-pipeline.md
clock-recovery.md
transport.md
signaling.md
routing.md
control-plane.md
billing.md
security.md
```

`docs/protocols/`:

```text
signaling-v1.md
session-resume.md
route-selection.md
billing-units.md
```

`docs/runbooks/`:

```text
cloudflare-outage.md
turn-failure.md
billing-webhook-failure.md
release-rollback.md
certificate-expiry.md
```

---

# 83. ADRs

Start with these.

```text
0001-monorepo.md
0002-rust-core.md
0003-webRTC-wire-protocol.md
0004-audio-network-clock-48khz.md
0005-opus-default-codec.md
0006-native-transport-bakeoff.md
0007-truce-plugin-shell.md
0008-cloudflare-turn.md
0009-cloudflare-sfu-provider.md
0010-sfu-provider-abstraction.md
0011-durable-object-signaling.md
0012-astro-web.md
0013-protobuf-signaling.md
0014-anonymous-direct-sessions.md
0015-prepaid-relay-credits.md
0016-workos-device-auth.md
```

Every major architecture change later gets another ADR.

Don't rewrite history.

---

# 84. Development commands

Root developer UX should be very simple.

```bash
just bootstrap
just check
just test
just web
just control
just plugin
just probe
just netlab
just package
```

Complex logic lives in:

```text
tools/xtask
```

not a 1,500-line shell script.

---

# 85. Implementation order

This matters enormously.

Do **not** begin by making a beautiful RELAY VST UI.

## Phase 0 — Foundation

Deliver:

```text
monorepo
CI
ADRs
domain model
protocol skeleton
testkit
```

Exit:

```text
all three OSes compile basic workspace
```

---

# 86. Phase 1 — Audio engine without networking

Build:

```text
relay-rt
relay-opus
relay-resample
relay-clock
relay-jitter
relay-audio
```

Test:

```text
audio file → encode → fake network → decode → output
```

Inject:

```text
jitter
loss
clock drift
```

Exit:

**12 virtual hours without latency drift.**

---

# 87. Phase 2 — Transport bake-off

Build:

```text
relay-probe
```

No DAW.

CLI A:

```text
relay-probe send
```

CLI B:

```text
relay-probe listen
```

Test all three transport candidates.

Exit:

one is selected by measured score.

This is where `libdatachannel+libnice` either becomes genuinely locked or gets replaced.

---

# 88. Phase 3 — Native Connect standalone

Compose:

```text
audio engine
+
transport
+
signaling
```

Two standalone processes exchange stereo audio.

No plugin yet.

Exit:

```text
Windows ↔ Linux
Windows ↔ macOS
macOS ↔ Linux
```

direct P2P works.

---

# 89. Phase 4 — Browser Link

Build:

```text
Astro player
packages/web-rtc
SessionDO
```

Flow:

```text
relay-probe send
      ↓
share URL
      ↓
Chrome / Firefox / Safari
```

Exit:

browser reliably receives Opus directly P2P.

---

# 90. Phase 5 — Truce plugin

Only now wrap it.

Plugin:

```text
process()
 ↓
relay-engine
```

Exit:

```text
Bitwig
REAPER
Ableton
Logic
```

basic connection works.

---

# 91. Phase 6 — TURN fallback

Add Cloudflare credentials and paid route logic.

Test:

```text
UDP completely blocked
only TCP 443 available
```

This phase is strategically huge.

Exit criterion:

**RELAY still connects.**

---

# 92. Phase 7 — Cloudflare Stream

Add:

```text
relay-fanout
relay-fanout-cloudflare
```

Producer:

```text
one upload
```

Listeners:

```text
5
20
100
```

Exit:

fan-out remains stable without re-encoding per listener.

---

# 93. Phase 8 — Authentication

Add WorkOS device flow.

Anonymous free users remain functional.

Exit:

```text
plugin → browser login → plugin authenticated
```

without embedding a browser.

---

# 94. Phase 9 — Credits/billing

Implement:

```text
Paddle checkout
webhook
ledger
AccountMeterDO
usage leases
```

Then paid TURN/Stream can leave beta.

Exit:

```text
purchase
→ exact idempotent credit
→ consume
→ reconcile
```

Webhooks may safely replay repeatedly.

---

# 95. Phase 10 — LiveKit fallback

Implement provider adapter.

Exit:

same share URL/session abstraction can use either:

```text
Cloudflare
LiveKit
```

This is GA insurance.

---

# 96. Phase 11 — Hardening

Run:

```text
netlab
host matrix
browser matrix
soaks
chaos
security review
license review
```

Fix every P0/P1.

---

# 97. Phase 12 — Desktop GA

Ship:

```text
Windows
macOS
Linux
```

with:

```text
Connect
Link
TURN fallback
Stream
credits
```

---

# 98. Phase 13 — iOS

Only after desktop core is boring and stable.

Reuse:

```text
relay-domain
relay-audio
relay-engine
transport
```

Add:

```text
AUv3 shell
container app
mobile session UI
```

---

# 99. What I would deliberately NOT build in V1

No:

```text
video
chat
file sharing
recording
cloud DAW
social network
multiuser mixer
multi-track stems
PCM lossless
96k streaming
MoQ
QUIC custom protocol
AI features
remote DAW control
screen sharing
Android DAW plugin
AAX
E2EE SFU
```

Every single one can be added later.

The launch proposition stays:

> **Put RELAY on audio. Send it anywhere.**

---

# 100. The exact architectural lock

After all this research, this is what I would consider **truly locked**:

```text
Rust core
Cargo + pnpm monorepo
ports/adapters architecture
48 kHz Opus network clock
separate TX/RX audio pipelines
bounded lock-free RT queues
PLL + adaptive SRC
bounded adaptive jitter policy
WebRTC as V1 media wire protocol
native transport abstraction
Cloudflare STUN
Cloudflare TURN
Workers + Durable Objects signaling
Astro 7 web
framework-independent TS WebRTC package
provider-independent SFU interface
append-only usage/credit ledger
anonymous free direct sessions
prepaid paid-infrastructure usage
```

### Provisionally selected

```text
Truce
libdatachannel + libnice
Rubato
D1
Cloudflare Realtime SFU
WorkOS
Paddle
```

### Explicitly retained alternatives

```text
Truce
  ↔ JUCE

libdatachannel
  ↔ Shiguredo libwebrtc
  ↔ webrtc-rs

Rubato
  ↔ soxr/libsamplerate

Cloudflare Realtime
  ↔ LiveKit

D1
  ↔ Postgres

WorkOS
  ↔ Better Auth

Paddle
  ↔ Stripe

Astro
  ↔ SvelteKit
```

The important difference is that **those alternatives now have designed seams**, rather than being something you'd need a six-month rewrite to adopt.

---

# 101. The most important implementation principle

If someone opens:

```text
crates/relay-engine
```

six years from now, they should **not be able to tell** whether RELAY currently uses:

```text
Cloudflare
LiveKit
Truce
JUCE
libdatachannel
Google WebRTC
```

They should see:

```text
Session
Peer
AudioTx
AudioRx
Transport
Route
ClockRecovery
QualityPolicy
```

That is what a genuinely modular RELAY codebase looks like.

And I would be especially strict about the first three development deliverables being **audio-lab → transport-probe → standalone Connect**, *before* a production plugin or paid backend exists. If those three pieces are excellent, everything else becomes packaging and product. If those three are bad, no framework choice can save RELAY.

[1]: https://truce.audio/docs/guide/install/ "https://truce.audio/docs/guide/install/"
[2]: https://github.com/juce-framework/JUCE "https://github.com/juce-framework/JUCE"
[3]: https://github.com/paullouisageneau/libdatachannel "https://github.com/paullouisageneau/libdatachannel"
[4]: https://github.com/paullouisageneau/libdatachannel/blob/master/DOC.md "https://github.com/paullouisageneau/libdatachannel/blob/master/DOC.md"
[5]: https://developers.cloudflare.com/realtime/turn/ "https://developers.cloudflare.com/realtime/turn/"
[6]: https://github.com/libnice/libnice "https://github.com/libnice/libnice"
[7]: https://docs.rs/crate/shiguredo_webrtc/latest "https://docs.rs/crate/shiguredo_webrtc/latest"
[8]: https://github.com/webrtc-rs/rtc "https://github.com/webrtc-rs/rtc"
[9]: https://github.com/webrtc-rs "https://github.com/webrtc-rs"
[10]: https://opus-codec.org/release/stable/2026/01/14/libopus-1_6_1.html "https://opus-codec.org/release/stable/2026/01/14/libopus-1_6_1.html"
[11]: https://docs.rs/rtrb/latest/rtrb/index.html "https://docs.rs/rtrb/latest/rtrb/index.html"
[12]: https://docs.rs/crate/rubato/latest "https://docs.rs/crate/rubato/latest"
[13]: https://developers.cloudflare.com/realtime/sfu/pricing/ "https://developers.cloudflare.com/realtime/sfu/pricing/"
[14]: https://developers.cloudflare.com/realtime/sfu/sessions-tracks/ "https://developers.cloudflare.com/realtime/sfu/sessions-tracks/"
[15]: https://developers.cloudflare.com/realtime/sfu/limits/ "https://developers.cloudflare.com/realtime/sfu/limits/"
[16]: https://developers.cloudflare.com/realtime/sfu/changelog/ "https://developers.cloudflare.com/realtime/sfu/changelog/"
[17]: https://docs.livekit.io/intro/cloud/ "https://docs.livekit.io/intro/cloud/"
[18]: https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/ "https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/"
[19]: https://developers.cloudflare.com/durable-objects/api/state/ "https://developers.cloudflare.com/durable-objects/api/state/"
[20]: https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/ "https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/"
[21]: https://workos.com/docs/authkit/cli-auth "https://workos.com/docs/authkit/cli-auth"
[22]: https://better-auth.com/docs/installation "https://better-auth.com/docs/installation"
[23]: https://developers.cloudflare.com/d1/best-practices/read-replication/ "https://developers.cloudflare.com/d1/best-practices/read-replication/"
[24]: https://developer.paddle.com/get-started/how-paddle-works/ai-companies/ "https://developer.paddle.com/get-started/how-paddle-works/ai-companies/"
[25]: https://docs.stripe.com/billing/subscriptions/usage-based "https://docs.stripe.com/billing/subscriptions/usage-based"
[26]: https://developers.cloudflare.com/workers/framework-guides/web-apps/astro/ "https://developers.cloudflare.com/workers/framework-guides/web-apps/astro/"
[27]: https://truce.audio/ "https://truce.audio/"
[28]: https://docs.rs/cargo-nextest/latest/cargo_nextest/ "https://docs.rs/cargo-nextest/latest/cargo_nextest/"