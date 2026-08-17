# relay-audio scheduled playback implementation

## Scope and thread model

`PlaybackWorker` is caller-driven off the device callback. It accepts a resolved
48 kHz RX `PcmFrame`, an extended media sample position, and the local device
frame at which that position was scheduled. It performs drift estimation,
adaptive SRC, control, and one bounded all-or-drop ring publication.
`PlaybackRenderer` is callback-facing and performs only full-buffer zero-fill,
bounded SPSC copy, and primitive atomic counters. Endpoint/metrics destruction
is explicitly outside callback scope.

## Primary sources (4)

1. [`relay-clock` crate contract](../../crates/relay-clock/src/lib.rs) — scheduled
   media progression, not packet arrival time, is the only drift observation;
   controller ratio is output/input correction.
2. [`relay-resample` adaptive contract](../../crates/relay-resample/src/adaptive.rs)
   — fixed input chunks, preallocated output, typed
   `OutputInputRatioCorrectionPpm`, smoothing, and reset behavior.
3. [`relay-rt` ring contract](../../crates/relay-rt/src/lib.rs) — SPSC endpoints,
   all-or-drop writes, partial reads, callback-safe operations, and off-callback
   destruction order.
4. [rubato `Async` 4.0.0 API](https://docs.rs/rubato/4.0.0/rubato/struct.Async.html)
   — authoritative adjustable asynchronous resampler used behind the local
   wrapper.

## Potential corrections resolved

- **Do not infer drift from delivery timing.** The worker API has no arrival,
  network, wall-clock, or socket timestamp; it requires the scheduler's extended
  media position and device-frame position.
- **Do not pass raw drift into SRC.** `ClockRecoveryOutput::ratio_multiplier` is
  converted through `OutputInputRatioCorrectionPpm::from_ratio_multiplier`.
- **Do not hide committed progress behind an error.** SRC publication is
  all-or-drop; a rare controller failure after publication is returned inside
  `PlaybackProcessReport::control_fault` with the publication outcome intact.
- **Do not flush an SPSC ring from the producer side.** Reset is accepted only
  when the ring is empty. Otherwise the host must stop/detach and recreate the
  pair, preventing old/new epoch mixing.
- **Do not set latency from arbitrary maximum capacity.** The default fill
  target is one adaptive output transaction plus algorithmic delay, clamped
  strictly inside the fixed ring.
- **Do not consume malformed callback buffers.** A non-frame-aligned output is
  fully zeroed and leaves the ring untouched.

## Decisions and bounded behavior

- Construction validates exact estimator clock domains (remote 48 kHz and the
  configured local device rate), a nonzero interior fill target, all checked
  buffer sizes, and reconstructs the authoritative clock/SRC implementations.
- Worker buffers are allocated once. Tests verify stable pointer/capacity across
  rate/duration matrices and reset.
- Ring fill is sampled after publication at one stable phase. Controller cadence
  is based only on the scheduled device-frame timeline.
- Estimator regression/stall faults the worker. Old queued audio must drain
  before a fixed-storage reset can start a new epoch.
- Renderer starvation and disconnect initialize every missing sample to zero;
  no callback path allocates, locks, waits, logs, performs I/O/networking/DSP, or
  destroys ownership.

## Tests

The module tests cover every supported playback rate (44.1/48/96/192 kHz) and
Opus duration (5/10/20 ms), finite output, fixed workspace reuse, construction
bounds, positive-remote-drift/negative-output-ratio sign, full-ring drops,
starvation zero-fill and resume, misalignment without consumption, disconnect,
nonfinite recoverability, discontinuity faulting, drain-before-reset, and reset
storage reuse.

## Validation

_Pending independent review and final workspace validation._

## Disposition

_Pending independent review._
