# Audio finite drain interface synthesis

## Decision

Select **Option A, three explicit end operations**, for the Phase-1 finite-source gate.
Keep Option B as a possible offline oracle and defer Option C unless Relay later needs
network-visible graceful EOS across a live peer.

The selected seam is the smallest one that finishes state already owned by the
production pipeline:

1. `FiniteTxWorker::process_finite` remains the finite capture/SRC/Opus operation,
   with a strict no-progress `RequireComplete` policy or explicit `ZeroPad` manifest.
2. `RxWorker::drain` remains the only terminal resolution of its one-frame lookahead.
3. `PlaybackWorker::finish_finite` passes the withheld last decoded frame and its valid
   media prefix to a terminal `AdaptiveClockConverter::finish_interleaved`, retains any
   ring-blocked tail, and resumes publication on later calls.
4. After finish reports all tail frames published, the owner drops the producer on its
   control thread. A callback `RenderState::Disconnected` is the existing terminal
   acknowledgement that the ring was empty and the producer gone. The owner first
   stops/detaches the physical device and waits for its API acknowledgement before
   destroying callback-owned endpoints.

No arrival time, `NetworkTime`, delivery jitter, or wall time enters playback recovery
or determines converter completion. Correction remains output/input through
`OutputInputRatioCorrectionPpm`.

## Comparison

| Property | A: explicit operations | B: separate offline pipeline | C: generic ending protocol |
|---|---|---|---|
| Reuses current live state | Yes | No | Yes |
| Finishes adaptive playback SRC | Yes | No; fixed offline render | Yes |
| Preserves current callback | Yes | Yes, but bypasses it | Adds terminal fence vocabulary |
| Handles ring backpressure | Retained bounded tail | Sink-owned | General poll/budget state machine |
| Wire/schema change | None; valid final count is local manifest | Finite file manifest | Requires integrity-bound media extent |
| Scope/implementation risk | Smallest | Duplicates pipeline/oracle only | Largest and cross-module |
| Solves current finite lab gate | Yes | Only as an oracle | Yes |
| Solves peer-visible live graceful EOS | No | No | Yes |

Option B is deep and testable but switching an active stream into it would duplicate
codec/filter state and lose phase continuity. Option C is the right shape for a future
peer-negotiated graceful end, but its new media extent, resumable states, budgets, and
ring fence are not justified by the local Phase-1 finite gate and would disturb frozen
transport work.

## Required corrections to Option A before implementation

* Rename `RejectIncomplete` to `RequireComplete`. If a valid media remainder exists,
  return `IncompleteFinalOpusFrame` with an empty batch, unchanged RTP state, and zero
  input/packet progress. Never emit the complete prefix and silently discard the tail.
* Treat `FiniteTxReport::{final_valid_media_frames,zero_padded_media_frames}` as an
  out-of-band local finite manifest. They must sum to one Opus packet for a padded end.
  If the manifest cannot be carried with the local test batch, require alignment.
* Withhold the final `PcmFrame` returned by `RxWorker::drain` from ordinary
  `PlaybackWorker::process_frame`; pass it exactly once to `finish_finite` with the
  manifest's valid prefix. An empty source never manufactures a frame.
* Validate finish buffers and valid lengths before mutating smoothing/filter state.
  Construction sizes the worst-case finish workspace for configured ratio bounds.
  Processing and retry allocate nothing.
* Report generated, leading-trim, trailing-trim, valid output, published, pending, and
  rendered/terminal facts separately. Startup latency already heard in a live render
  is not retroactively removed; finite collected-output tests may trim it explicitly.
* A full ring is `Pending`, not a dropped tail and not success. Codec/SRC faults are
  sticky. Repeated success emits nothing. Abort/disconnect stays distinct from finish.
* Do not claim libopus encoder-lookahead/pre-skip compensation: the current codec API
  does not expose a container pre-skip contract. This gate proves declared source
  frame accounting and SRC tail completion through a lossy frame codec, not waveform
  identity or gapless file-container semantics.

## Acceptance tests

For 5/10/20 ms and capture/playback rates 44.1, 48, 96, and 192 kHz, cover aligned,
short, padded, and zero-length sources. Prove fixed-SRC trim accounting; exact packet,
sequence, timestamp and valid/padded counts; mandatory RX drain; adaptive generated and
trimmed output accounting; repeated ring-full retry without loss/duplication; render to
`Disconnected`; and checked identity across source-valid, media-valid, playback-valid,
published, and rendered frame domains. Include capacity/no-progress and sticky-fault
cases, no allocations after construction, and strict debug/release/Clippy validation.

This selection is finite/lab-only. A later product requirement for end-to-end live EOS
must reopen Option C rather than silently assigning disconnect a graceful-end meaning.
