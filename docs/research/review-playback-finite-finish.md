# Independent Adversarial Review: Option-A PlaybackWorker Finite Finish

**Disposition: CHANGES REQUESTED / BLOCKED**

## Scope

Read-only audit of the Option-A finite playback implementation and its tests against
`docs/design/audio-finite-drain-synthesis.md` and the accepted concrete adaptive
finish in `relay-resample`. Production and committed test source were not edited.
Temporary integration tests were created only for adversarial execution and then
removed, including their named build artifacts.

The reviewed thread model is unchanged: `PlaybackWorker` and finite retries run on a
worker/control thread; `PlaybackRenderer::render` is callback-facing; the owner must
stop/detach the physical callback and receive the device API's acknowledgement before
destroying callback-owned endpoints.

## Findings

### Critical (C)

No critical finding.

### High (H)

#### H1 — `RenderState::Disconnected` can be reported while hundreds of buffered tail samples remain

**Locations:** `crates/relay-audio/src/playback.rs:1021-1050`, especially the direct
mapping at `:1039-1044`; `crates/relay-rt/src/ring.rs:158-181`, especially `:164-175`;
terminal contract in `docs/design/audio-finite-drain-synthesis.md:14-19`.

The selected design gives `RenderState::Disconnected` a strong terminal meaning: the
producer is gone **and the ring is empty**. `PlaybackRenderer` currently maps
`ReadState::Disconnected` directly. `AudioConsumer::read`, however, selects
`Disconnected` solely because the producer is abandoned after the bounded read; its
own documentation explicitly permits `read_samples != 0`, and it does not require the
ring to be empty. If the callback asks for less than the queued tail, it consumes only
that prefix and still returns `Disconnected` with unread audio remaining.

A removed public-API adversarial test finished a valid 5 ms frame, observed more than
four queued scalar samples, dropped the worker/producer, and rendered a four-sample
callback. It failed in debug as follows:

```text
assertion `left != right` failed: terminal ack while 728 samples remain
  left: Disconnected
 right: Disconnected
```

An owner that treats this documented state as the finite terminal acknowledgement may
stop playback after that callback and abandon the remaining tail. This contradicts
the Option-A drain protocol even though the state is never emitted before producer
drop.

**Required correction:** do not expose `RenderState::Disconnected` until the producer
is observed gone and the post-read readable count is zero. If an abandoned producer
still has buffered samples after the current read, report the ordinary completed
read state and continue draining. Add a regression that drops the producer with more
than one callback of queued tail, proves no early terminal state, preserves exact
sample order/count, and observes exactly one usable terminal acknowledgement only
when the final buffered prefix has been consumed (or on the next empty callback,
according to the documented contract).

### Medium (M)

#### M1 — committed tests do not prove the required public finite TX/RX/playback handoff

**Locations:** `crates/relay-audio/tests/playback_finish_allocation.rs:52-113`;
`crates/relay-audio/src/playback.rs:1402-1719`.

The committed allocation test is the only test that mentions `FiniteTxWorker`,
`RxWorker::drain`, and `PlaybackWorker::finish_finite` together. It drains the sole RX
frame, then submits that same decoded frame first through ordinary `process_frame` and
again through `finish_finite`. It also supplies literal valid counts (`240` during
prewarm and `239` while measured) rather than propagating
`FiniteTxReport::final_valid_media_frames`. Thus it is useful allocation evidence, but
it actively does not model the required rule that the final RX-drained frame is
withheld from ordinary processing and passed once with the TX padding manifest.

The playback unit matrix uses synthetic `PcmFrame` values. It covers full/partial
prefixes and all rates/durations, but no committed test carries a real `ZeroPad`
manifest through encoded packets, mandatory RX lookahead/drain, prior ordinary
playback, terminal finish, retained publication, producer drop, and rendering.
Committed playback tests also do not directly cover `Final` twice, renderer loss
after a partial tail publication, or playback-level collected `S + G - L - T`
accounting (the accepted lower-level adaptive suite does cover the last property).

Removed adversarial tests established that the current public pieces can be composed:

- 44.1 kHz finite TX, 20 ms, 1,000 source frames produced two packets and a manifest
  of 129 valid plus 831 padded media frames; the first RX result went through
  `process_frame`, the one `RxWorker::drain` result went once through `finish_finite`
  with the reported 129-frame prefix, and a second drain returned `None` — **PASS**.
- `Final` twice returned `InvalidTransition` without a second conversion — **PASS**.
- dropping the renderer after a minimum-ring partial publication made `Continue`
  return `RendererDisconnected` with the pending count retained, then sticky
  `Faulted` — **PASS**.
- playback collection with 0/1/2/7 prior streaming transactions satisfied
  `collected - L == S + G - L - T` — **PASS**.

**Required correction:** commit the real `ZeroPad -> packet batch -> RX tick/drain ->
PlaybackWorker` regression and the three focused lifecycle/accounting cases above.
The public path is presently functional in the exercised case, so this is an
acceptance-evidence gap rather than a second release-blocking source defect.

## Requirement disposition

| Requirement | Disposition | Evidence |
|---|---|---|
| Final frame is the one RX-drained frame, withheld once, with valid prefix | **Implementation seam present; committed evidence incomplete (M1)** | `FinitePlaybackInput` carries only scheduled positions and valid prefix; removed real public-path test passed. |
| Genuinely empty source manufactures no frame | **Pass** | `finish_empty`; committed `Continue`-before-end, empty, repeated-empty, and missing-final tests. |
| Clock observes scheduled media/device positions only; no arrival time | **Pass** | `FinitePlaybackInput` and `process_frame` accept only extended media and scheduled local-device positions; both construct `PlayoutClockObservation::from_scheduled_playout`. |
| Adaptive terminal conversion is one shot | **Pass** | State changes to `Finishing` immediately after the sole converter finish; `Final`/`Empty` are rejected in `Finishing`/`Finished`; removed `Final`-twice test passed. |
| Ring-full tail is retained and retried without loss/duplication | **Pass** | Minimum ring is smaller than the 192 kHz/5 ms tail; publication advances its cursor only on `Written`; committed reference-vs-bounded sample equality and zero-drop test passes debug/release. |
| Worst-case workspace and no allocation after construction | **Pass** | Finish workspace derives from accepted `AdaptiveFinishRequirements`; pointer/capacity matrix and counting-allocator integration pass; accepted opposite-ratio matrix passes debug/release without panic. |
| Leading/trailing and prior streaming accounting | **Pass, but commit playback regression (M1)** | Reports separate `G`, `L`, `T`, valid, published, pending, and queued facts; accepted adaptive strong-accounting test plus removed playback `S + G - L - T` test pass. |
| States, idempotence, sticky fault, reset | **Pass** | Continue-before-Final, repeated success, prior loss, discontinuity, renderer loss, pending reset rejection, completed reset, storage reuse; removed missing adversarial transitions passed. |
| Existing live worker semantics and callback boundedness remain intact | **Pass except terminal-state contract H1** | Live path remains all-or-drop; callback zero-fills, copies boundedly, and performs no allocation/lock/wait/I/O/DSP. |
| `RenderState::Disconnected` is a true finite terminal acknowledgement after producer drop and empty drain | **Fail (H1)** | Removed test received `Disconnected` with 728 unread samples. |
| Public finite TX padding manifest propagation | **Functional removed test passes; committed evidence incomplete (M1)** | Real `129 + 831 == 960` manifest reached the one RX-drained frame and finish. |

## Adaptive workspace and allocator audit

The accepted adaptive implementation constructs pinned Rubato with the
outward-rounded recurrence-derived private phase workspace, resizes it back to the
public fixed chunk, and publishes a checked finish workspace from the authoritative
converter requirements. Playback allocates that exact interleaved capacity once with
`try_reserve_exact`/`resize`; finish and retry only slice/copy retained storage.

Allocator dependency evidence is clean:

- `crates/relay-audio/Cargo.toml` declares
  `relay-resample-test-allocator = { version = "=0.0.0", path = "../relay-resample/tests/allocation-counter" }`.
- Locked Cargo metadata resolves it as `kind = "dev"`, `req = "=0.0.0"`, `source = null`,
  to the local package version `0.0.0`.
- The allocator package declares `license = "MPL-2.0"` and `publish = false`.
- Its unsafe scope is the necessary test-only `GlobalAlloc` implementation: allocation,
  zeroed allocation, reallocation, and deallocation forward the original arguments to
  `System`; only allocation operations increment an `AtomicUsize`.
- The exact locked deny gate passes all four families. Its three
  `license-not-encountered` messages are non-failing warnings for unused allow-list
  entries.

## Validation

All commands ran from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` after the temporary
adversarial test source was removed.

| Command | Result |
|---|---|
| `cargo test -p relay-audio --all-targets --all-features --locked` | **PASS** — 29 unit, 18 foundation, 3 loopback, 3 media soak, 1 playback allocation, 11 RX, 15 TX, 1 virtual-hours. |
| `cargo test --release -p relay-audio --all-targets --all-features --locked` | **PASS** — same target/test counts. |
| `cargo clippy -p relay-audio --all-targets --all-features --locked -- -D warnings` | **PASS**. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | **PASS**. |
| `cargo deny --locked check licenses advisories sources bans` | **PASS** — advisories, bans, licenses, sources all `ok`. |
| `cargo fmt --all -- --check` | **PASS**. |
| Accepted opposite-ratio exact finish test, debug and release | **PASS**. |
| Accepted stereo alternating/opposite-ratio all-pair/duration/prefix matrix, debug and release | **PASS**. |
| Accepted adaptive counting-allocator test, release | **PASS**, zero allocations and stable caller storage. |
| Removed adversarial: small callback after producer drop with buffered tail | **FAIL as expected**, premature `Disconnected` with 728 samples remaining (H1). |
| Removed adversarial: `Final` twice | **PASS**. |
| Removed adversarial: renderer drop during pending retry | **PASS**. |
| Removed adversarial: real padded TX manifest through RX drain/finish | **PASS**. |
| Removed adversarial: playback prior-streaming strong accounting | **PASS**. |

## Overall disposition

**BLOCKED by H1.** Finite generation, retention/retry, workspace bounds, allocation
behavior, lifecycle rejection, accounting, and real manual manifest propagation are
otherwise sound in the exercised cases. Correct the premature callback terminal
state, then commit the M1 public-path/lifecycle/accounting regressions and rerun the
same gates before accepting Option A.
