# ADR 0006: Gate the native transport choice on an acceptance bakeoff

## Status
Accepted process; native transport selection and service providers remain deliberately unresolved and provisional. Truce separately remains a provisional plugin-shell choice.

## Context
A native transport or managed provider may reduce delivery time, but early commitment could couple Relay to opaque behavior, pricing, deployment constraints, or non-portable APIs. Paper comparisons cannot establish realtime quality or operational fit on Relay's target networks and devices.

## Decision
Run a repeatable bakeoff of credible native transport candidates, including a standards-based reference path, behind the same Relay transport boundary. Accept a candidate only after it passes predeclared functional, interoperability, quality, reliability, security, operability, portability, cost, and exitability gates. Until then, no candidate or provider name may appear in the wire contract or portable core API. Truce is not a transport candidate; its separate plugin-shell evaluation stays acceptance-gated.

## Consequences
- Transport and provider choices are evidence-based and reversible.
- Parallel adapters and test infrastructure add near-term work.
- Schedule pressure cannot silently waive acceptance criteria; exceptions require a superseding ADR.
- A provider can win operationally without becoming the protocol or domain model.

## Validation gates
- The bakeoff publishes identical scenarios, datasets, target-device coverage, and pass/fail thresholds before measurement.
- Each candidate passes WebRTC/Opus conformance, loss/jitter/roaming/reconnect tests, and security review.
- Tail latency, audible failure rate, CPU, battery, bandwidth, availability, observability, and total cost are measured over representative runs.
- Candidate removal is demonstrated by swapping adapters without changing the V1 wire contract or portable core API.
- The final selection records raw evidence, rejected alternatives, risks, and a rollback/exit plan in a superseding ADR.
