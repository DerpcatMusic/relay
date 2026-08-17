# ADR 0002: Implement the portable realtime core in Rust

## Status
Accepted for V1 foundation; FFI shape and platform wrappers remain provisional.

## Context
Relay needs a portable core for media framing, timing, buffering, codec orchestration, and transport-independent session logic. It must offer predictable performance and safe concurrency across desktop and mobile hosts without forcing UI or provider choices into the core.

## Decision
Implement the portable realtime core in Rust. Expose narrow, versioned host interfaces and keep platform capture/render, permissions, UI, and provider SDK integration outside the core. Unsafe code and FFI must be isolated, documented, and tested at explicit boundaries.

## Consequences
- Memory safety and explicit ownership reduce common concurrency defects.
- Hosts incur FFI, packaging, and cross-compilation work.
- Contributors need Rust and platform-language expertise.
- The core remains independently testable and reusable across provider experiments.

## Validation gates
- The core builds for every declared V1 target in CI.
- Realtime paths have bounded work and documented allocation/locking rules.
- FFI tests cover ownership, threading, error propagation, and teardown.
- No public core API exposes Truce, another plugin-shell type, or a provider-specific type.
