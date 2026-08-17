# Web Foundation Review

## Scope

Independent review limited to:

- package manifests and lockfile/workspace configuration
- the Astro application shell
- `packages/web-rtc`

Reviewed files were not edited. This report is the only intended repository change.

## Criteria

- current-version compatibility against the declared toolchain and selected registry facts
- framework independence of `packages/web-rtc`
- accidental behavior, including implicit side effects and environment-sensitive execution
- reproducibility under frozen installation, type checking, and production build via `npx pnpm`
- severity-ranked, evidence-backed potential corrections

## Method constraints

- Exactly three package-registry checks were used; no broader web research was performed.
- Frozen install, typecheck, and build were run through `npx pnpm`.
- Confirmed failures are separated from risks and optional improvements.

## Executive summary

The foundation currently installs, typechecks, and builds successfully on Node `v24.18.1` with pnpm `11.22.0`. Astro and TypeScript 7 are pinned to the registry's current releases, the lockfile is accepted frozen, and `packages/web-rtc` has no framework dependency. There are no high-severity or release-blocking findings.

The main correction is a real peer-range mismatch: `@astrojs/check@0.9.6` declares TypeScript 5 support while the app supplies TypeScript 6. The current commands happen to pass, but the combination is outside the installed checker's contract. Reproducibility is also incomplete because Astro requires Node 22.12 or later while the workspace declares no Node baseline.

## Validation results

| Check | Command | Result |
| --- | --- | --- |
| package manager | `npx pnpm --version` | PASS: `11.22.0` (matched `packageManager`) |
| frozen install | `npx pnpm install --frozen-lockfile` | PASS: all 3 workspace projects; lockfile unchanged; existing install already up to date |
| typecheck | `npx pnpm typecheck` | PASS: `packages/web-rtc` and `apps/web`; Astro reported 0 errors, warnings, and hints |
| production build | `npx pnpm build` | PASS: static Astro output, 1 page |

The generated `index.html` contained `Session state: idle` and no client JavaScript files. This is important to the Astro execution finding below.

### Registry checks (3/3)

Checked 2026-08-16 with `npm view`:

1. `astro@latest`: `7.2.2`; engines include Node `>=22.12.0`, npm `>=9.6.5`, and pnpm `>=7.1.0`.
2. `typescript@latest`: `7.0.2`; engines include Node `>=16.20.0`.
3. `@astrojs/check@latest`: `0.9.10`; peer range is TypeScript `^5.0.0 || ^6.0.0`.

Consequently, the declared Astro `7.2.2` and TypeScript `7.0.2` are current. The app's older `@astrojs/check@0.9.6` is not current and its installed metadata only accepts TypeScript `^5.0.0`.

## Severity-ranked findings

### Medium — Astro checker is paired with an unsupported TypeScript major

**Evidence:**

- `apps/web/package.json:15-16` pairs `@astrojs/check: 0.9.6` with `typescript: 6.0.2`.
- `pnpm-lock.yaml:35-41` records `@astrojs/check@0.9.6` with peer dependency `typescript: ^5.0.0`.
- The lockfile snapshot resolves that checker against TypeScript 6 despite the incompatible declared peer range.
- The registry check shows current `@astrojs/check@0.9.10` explicitly supports both TypeScript 5 and 6.

**Impact:** The present `astro check` passes, but the workspace relies on behavior outside the installed package's compatibility contract. A checker code path or future environment can fail even though the frozen installation is accepted.

**Potential correction:** Upgrade `@astrojs/check` to `0.9.10` and refresh the lockfile, retaining app TypeScript 6. Re-run frozen install, typecheck, and build.

### Medium — Required Node baseline is not declared

**Evidence:**

- Root `package.json` pins pnpm but has no `engines.node` or other Node runtime policy.
- `apps/web/package.json` also has no Node engine declaration.
- `pnpm-lock.yaml:935-937` and the Astro registry metadata require Node `>=22.12.0`.
- Validation passed on Node `v24.18.1`; it does not demonstrate compatibility with an unqualified developer/CI Node version.

**Impact:** A fresh environment can select Node 20 or an older Node 22 release, then encounter engine warnings or build/runtime failures despite an exact pnpm version and frozen lockfile.

**Potential correction:** Declare a root Node baseline compatible with Astro (at minimum `>=22.12.0`), and pin a concrete development/CI Node release using the repository's chosen version-manager mechanism. Keep CI on the same major used for release builds.

### Low — The Astro shell constructs the “web session” at build time, not in the browser

**Evidence:**

- `apps/web/src/pages/index.astro:1-5` imports and constructs `RelayWebSession` in Astro frontmatter.
- The successful build reports static output.
- The emitted page contains the precomputed string `Session state: idle` and no client JavaScript.

**Impact:** There is no current runtime defect because the class is a side-effect-free placeholder. However, the shell can misleadingly look like it exercises a browser session. Adding `RTCPeerConnection`, browser globals, network activity, or cleanup to the constructor would make those operations run during prerender/build or fail there rather than execute for each browser client.

**Potential correction:** Keep frontmatter construction only if it is intentionally a build-time import smoke test. Before the session gains browser behavior, instantiate it in a client script/island with an explicit mount and disposal lifecycle; keep build-time rendering free of connection side effects.

### Low — `web-rtc` is framework-independent but only consumable as bundler-transpiled source

**Evidence:**

- `packages/web-rtc/src/**` imports no Astro or other framework APIs; its manifest has no runtime dependencies. This passes the framework-independence criterion.
- `packages/web-rtc/package.json:6-11` exports `./src/index.ts`, while both `build` and `typecheck` run `tsc --noEmit`.
- `packages/web-rtc/tsconfig.json:7-8` combines `noEmit: true` with `declaration: true`; no declarations or JavaScript are produced.
- Astro/Vite consumes this layout successfully, but a direct Node ESM import from `apps/web` fails resolving the source's `.js` specifier because no emitted `RelayWebSession.js` exists.

**Impact:** This is acceptable for a private, source-first workspace package, but “build” does not create an artifact and consumers must understand/transpile TypeScript source. That is toolchain coupling even though there is no framework coupling.

**Potential correction:** Choose and document one model:

- for a private source package, rename the package `build` script to reflect validation or make the root build semantics explicit, and remove ineffective `declaration: true`; or
- for a generally consumable module, emit ESM JavaScript and declarations to `dist`, point conditional `exports`/`types` at those files, and validate the emitted artifact directly.

## Additional observations

- Exact dependency versions plus lockfile integrities and `packageManager: pnpm@11.22.0` are strong reproducibility choices.
- `pnpm-workspace.yaml:5-6` explicitly allows the esbuild lifecycle script under pnpm 11; the frozen install and Astro build confirm this configuration works in the tested environment.
- Two TypeScript majors are intentional in effect: the Astro app uses 6 while root and `web-rtc` use the current 7. This is reproducible under the lockfile, but the root TypeScript dependency appears redundant because each participating workspace declares its own compiler. Removing it or introducing a deliberate workspace version policy would reduce drift; this is cleanup, not a correctness issue.
- `RelayWebSession` is currently deterministic and side-effect free. Its public API uses only standard language/web types and does not leak Astro types.

## Recommended correction order

1. Upgrade `@astrojs/check` to a TypeScript-6-compatible release and refresh the lockfile.
2. Declare and pin the Node baseline required by Astro.
3. Decide whether Astro's current session construction is intentionally build-time or should become a browser lifecycle.
4. Decide whether `web-rtc` is permanently source-first/private or should emit a portable package artifact.

## Review conclusion

**Pass with corrections.** Frozen install, typecheck, and build all pass; current Astro and TypeScript pins are valid; and `packages/web-rtc` is framework-independent. The two medium findings should be corrected to make the compatibility and reproducibility claims explicit rather than incidental.
