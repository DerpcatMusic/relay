# ADR 0004: Use a 48 kHz network media clock

## Status
Accepted for V1 wire timing; device sample rates and resampler implementations remain provisional.

## Context
Devices may capture and render at different rates, but peers need one timestamp domain for packet duration, jitter buffering, synchronization, and diagnostics. The Opus RTP payload format increments timestamps at 48 kHz for every Opus mode and sampling rate ([RFC 7587 §4.1](https://www.rfc-editor.org/rfc/rfc7587.html#section-4.1)).

## Decision
Define V1 RTP audio timestamps and packet-duration calculations in a 48,000-ticks-per-second clock. Convert at the device boundary with explicit resampling when hardware uses another rate. Do not infer wall-clock time directly from RTP timestamp values; handle wraparound and clock drift explicitly. This timing decision selects no resampler, transport implementation, or service provider.

## Consequences
- Packet timing and cross-implementation diagnostics share one unit.
- Non-48 kHz devices require resampling and drift control.
- Internal DSP may use other rates only behind explicit conversion boundaries.
- A 10 ms network duration is 480 ticks and a 20 ms duration is 960 ticks.

## Validation gates
- Golden tests cover duration conversion, timestamp wraparound, discontinuity, and drift.
- Long-run loopback tests show bounded buffer occupancy between mismatched device clocks.
- Packet traces use a 48 kHz RTP clock regardless of negotiated Opus bandwidth.
- Metrics label timestamp ticks separately from monotonic or wall-clock time.
