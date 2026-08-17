# ADR 0001: Use a monorepo

## Status
Accepted for V1 foundation; internal package boundaries and plugin/provider choices remain provisional.

## Context
Relay spans a realtime media core, platform adapters, applications, protocol definitions, tests, and release tooling. These parts must evolve against the same wire and timing contracts while the product boundary is still being discovered.

## Decision
Keep V1 source, protocol fixtures, applications, platform integration, tests, and operational tooling in one repository. Preserve explicit package boundaries and dependency direction inside the repository; do not treat the monorepo as permission for unrestricted coupling.

## Consequences
- Cross-layer changes can be reviewed and validated atomically.
- One revision identifies compatible protocol, core, and application code.
- CI may become broader, so affected-package selection and hermetic tests will be needed.
- A future split remains possible once contracts and ownership stabilize.

## Validation gates
- One clean checkout can build and test every V1 deliverable with documented commands.
- Dependency checks reject cycles and undeclared cross-package access.
- Wire fixtures and compatibility tests are versioned beside implementations.
- Repository layout does not encode Truce, another plugin shell, or any service provider as permanent architecture.
