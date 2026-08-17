# Web workspace foundation

Date: 2026-08-16

## Scope

This change adds only a minimal pnpm workspace, an Astro app, and a framework-independent `@relay/web-rtc` package. The session class is deliberately an inert placeholder: it stores options and exposes the single `idle` state, but creates no browser or WebRTC objects.

## Dependency checks

Two npm registry checks were performed (the requested maximum):

- `npm view typescript version` returned `7.0.2`.
- `npm view pnpm version` returned `11.22.0`.

The root `packageManager` and root/package-library TypeScript versions use those exact current versions. Astro is pinned to the requested exact version `7.2.2`.

## Compatibility correction

`astro check` cannot currently use TypeScript 7's programmatic API. The first validation run failed with Astro's explicit instruction to use TypeScript 6.x. Therefore `apps/web` has a local `typescript` pin at `6.0.2`, while the root and `packages/web-rtc` retain registry-current `7.0.2`. This keeps the framework-independent package checked with current TypeScript and makes Astro diagnostics operational.

Potential follow-up: remove the app-local TypeScript 6 pin once Astro's language server supports TypeScript 7. Also revisit `@astrojs/check` when its published TypeScript peer range catches up; pnpm currently reports a peer-range warning, although `astro check` succeeds with zero diagnostics.

pnpm 11 blocks unapproved dependency build scripts. `pnpm-workspace.yaml` explicitly allows the required `esbuild` build so a clean install succeeds without an interactive approval step.

## Validation

Bootstrap used `npx --yes pnpm@11.22.0` because no standalone `pnpm` executable was available. A `pnpm-lock.yaml` was generated.

The final runs succeeded:

```text
npx --yes pnpm@11.22.0 install --frozen-lockfile --offline  # success
npx --yes pnpm@11.22.0 typecheck                  # success; 0 errors/warnings/hints
npx --yes pnpm@11.22.0 build                      # success; 1 static page built
```

No control-plane, Rust, protobuf, or CI files were intentionally changed.
