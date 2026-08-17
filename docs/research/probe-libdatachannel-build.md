# libdatachannel v0.24.5 build/profile probe

**Status:** COMPLETE — build-only/profile evidence captured; mandatory network and cross-platform gates remain not run  
**Decision:** No provider is selected, rejected, integrated, or scored by this probe.  
**Probe root:** `/tmp/relay-provider-probes/libdatachannel`  
**Run date:** 2026-08-16 UTC

## Scope and evidence boundary

This probe fetched and built the official pinned source, installed a minimal
shared library, and compiled and ran a C API lifecycle smoke on the current
Linux x86_64 host. It produced two independent profiles:

1. **JUICE-MIN** — bundled default libjuice ICE backend.
2. **NICE-MIN** — system libnice ICE backend required to reach the wrapper's
   TURN TCP/TLS mapping.

The profiles have separate build/install/smoke/log/artifact/manifest/scorecard
directories. Results are not combined. No browser, Coturn, public STUN/TURN,
live ICE route, impairment, restart exchange, provider adapter, or other target
was run. In particular, NICE-MIN's compiled TCP/TLS mapping is not a live
TURN-TCP/TLS success claim.

The relay source, schemas, lockfiles, and T0 templates were not changed. The
only relay artifact written by this task is this research note. Copied and
filled T0 manifests/scorecards live under the isolated probe root.

## Immutable source identity

Official repository: <https://github.com/paullouisageneau/libdatachannel.git>

| Item | Exact identity |
|---|---|
| Release | `v0.24.5` |
| Annotated tag object | `61204eb447916d259eea6e90f4a50c73d2d062e8` |
| Peeled commit / checked-out HEAD | `443f6934d9007eb7076ab7825ba330f355fcbead` |
| Commit tree | `8559576cd906fd472bb4be5096449513e8dad37f` |
| `deps/json` | `55f93686c01528224f448c19128836e7df245f72` |
| `deps/libjuice` | `3c40a3545b6b1b62c7adee7f8f2bd58aa290afd6` |
| `deps/libsrtp` | `24b3bf8f19b6f5ab4cd2bcceb4f4064efca86fd5` |
| `deps/plog` | `94899e0b926ac1b0f4750bfbd495167b4a6ae9ef` |
| `deps/usrsctp` | `fec583d54493f879d2ae44a743423bf8a04371ab` |

The tag was fetched from `origin`, peeled, and asserted equal to the supplied
commit. All five official gitlinks were initialized recursively at the exact
commits above. Final `git status --porcelain=v1` was empty. Evidence:
`logs/source-fetch.txt` and `logs/source-identity-final.txt`.

Exact checkout commands:

```sh
ROOT=/tmp/relay-provider-probes/libdatachannel
git init "$ROOT/src"
git -C "$ROOT/src" remote add origin \
  https://github.com/paullouisageneau/libdatachannel.git
git -C "$ROOT/src" fetch --no-tags --depth=1 origin \
  443f6934d9007eb7076ab7825ba330f355fcbead
test "$(git -C "$ROOT/src" rev-parse FETCH_HEAD^{commit})" = \
  443f6934d9007eb7076ab7825ba330f355fcbead
git -C "$ROOT/src" checkout --detach \
  443f6934d9007eb7076ab7825ba330f355fcbead
git -C "$ROOT/src" fetch --depth=1 origin tag v0.24.5
test "$(git -C "$ROOT/src" rev-parse v0.24.5^{})" = \
  443f6934d9007eb7076ab7825ba330f355fcbead
git -C "$ROOT/src" submodule sync --recursive
git -C "$ROOT/src" submodule update --init --recursive --depth=1
```

## Host, toolchain, and environment

| Field | Captured value |
|---|---|
| Host | CachyOS Linux rolling, kernel `7.2.0-rc7-1-cachyos-rc`, x86_64 |
| CMake | `4.4.2` |
| Generator | Ninja `1.13.2` |
| C/C++ | GCC/G++ `16.1.1 20260728` |
| Git | `2.55.0` |
| pkg-config | `3.0.5` |
| OpenSSL | `3.6.3` (`openssl` package `3.6.3-1.1`) |
| libnice | pkg-config `0.1.23` (`libnice` package `0.1.23-1.1`) |
| GLib/GObject | pkg-config `2.88.3` (`glib2` package `2.88.3-1.1`) |
| Build image | **Missing:** direct host build, no container/image digest |
| Build environment | `CC`, `CXX`, `CFLAGS`, `CXXFLAGS`, `CPPFLAGS`, `LDFLAGS`, `MAKEFLAGS`, and `SOURCE_DATE_EPOCH` all unset |

The full OS/compiler/OpenSSL/PATH capture is `logs/host-toolchain.txt`; explicit
unset build variables are in `logs/build-env-explicit.txt`. Because this was a
rolling host build with no image digest and `SOURCE_DATE_EPOCH` unset, the
commands are repeatable against the immutable source but byte-for-byte
reproducibility across machines was **not** established.

## Profile 1: JUICE-MIN (default libjuice)

### Configuration and build

```sh
cmake -S "$ROOT/src" -B "$ROOT/build-juice-min" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$ROOT/install-juice-min" \
  -DBUILD_SHARED_LIBS=ON -DBUILD_SHARED_DEPS_LIBS=OFF \
  -DUSE_NICE=OFF -DUSE_GNUTLS=OFF -DUSE_MBEDTLS=OFF \
  -DPREFER_SYSTEM_LIB=OFF -DUSE_SYSTEM_JUICE=OFF \
  -DUSE_SYSTEM_USRSCTP=OFF -DUSE_SYSTEM_PLOG=OFF \
  -DNO_MEDIA=ON -DNO_WEBSOCKET=ON -DNO_EXAMPLES=ON -DNO_TESTS=ON \
  -DRTC_UPDATE_VERSION_HEADER=OFF
cmake --build "$ROOT/build-juice-min" --target datachannel --parallel 2 -v
cmake --install "$ROOT/build-juice-min"
```

All three commands exited 0. The verbose compile lines prove
`RTC_ENABLE_MEDIA=0`, `RTC_ENABLE_WEBSOCKET=0`, `USE_NICE=0`, bundled static
libjuice, bundled static usrsctp, and dynamic OpenSSL. The bundled usrsctp build
emitted one non-fatal `cnt set but not used` warning. Logs are
`logs/juice-min-{configure,build,install}.txt`; selected cache values are in
`logs/juice-min-cmake-cache-selected.txt`.

### C API lifecycle smoke

A standalone C11 executable was compiled against the installed header/shared
library with an install-directory rpath. With auto-negotiation disabled it
called `rtcPreload`, created a peer, created and verified the label of a data
channel, closed/deleted the channel, closed/deleted the peer, and called
`rtcCleanup`. It exited 0:

```text
profile=juice-min pc_created=1 dc_created=1 label=relay-build-smoke close_dc=0 delete_dc=0 close_pc=0 delete_pc=0 cleanup=1
```

Evidence: `smoke/juice-min/capi_smoke.c`,
`logs/juice-min-smoke-{compile,run}.txt`.

### Pinned hard negatives

These are source/API negatives, not simulated live routes:

- The public C API header has no `rtcRestartIce`; an intentional C11
  compile-negative failed with `implicit declaration of function
  'rtcRestartIce'` (expected exit 1).
- The wrapper gathers only while `GatheringState::New` at pinned
  `src/peerconnection.cpp:175`.
- The exact libjuice gitlink explicitly logs `ICE restart is not supported`
  and returns failure on changed remote credentials at
  `deps/libjuice/src/agent.c:547`.
- The JUICE wrapper explicitly logs `TURN transports TCP and TLS are not
  supported with libjuice` and returns without adding that server at
  `src/impl/icetransport.cpp:159-162`.

Machine assertions are in `logs/juice-min-source-negative-assertions.txt`; the
API compile-negative is `logs/juice-min-restart-api-negative.txt`. Thus
JUICE-MIN has the expected same-peer restart and TURN-control TCP/TLS hard
negatives. This alone does not score or select the candidate.

### Artifact and link closure

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| `artifacts/juice-min/libdatachannel.so.0.24.5` | 3,355,712 | `534b7f76da33e8103002b89eb31e225367d701771db7c48b26a9335bb8a29180` |
| `artifacts/juice-min/capi_smoke` | 16,664 | `3381deb4870aa7a22263c70f310cf2bd51427a2f3924ca210cdff5cc8093984d` |

Direct ELF `NEEDED` entries are `libssl.so.3`, `libcrypto.so.3`,
`libstdc++.so.6`, `libm.so.6`, `libgcc_s.so.1`, `libc.so.6`, and the loader;
libjuice and usrsctp are incorporated statically. The complete resolved host
closure (including OpenSSL's compression dependencies) is in
`logs/juice-min-ldd-library.txt`; package versions/license identifiers are in
`logs/juice-min-link-package-license-closure.txt`.

## Profile 2: NICE-MIN (libnice)

### Configuration and build

```sh
cmake -S "$ROOT/src" -B "$ROOT/build-nice-min" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$ROOT/install-nice-min" \
  -DBUILD_SHARED_LIBS=ON -DBUILD_SHARED_DEPS_LIBS=OFF \
  -DUSE_NICE=ON -DUSE_GNUTLS=OFF -DUSE_MBEDTLS=OFF \
  -DPREFER_SYSTEM_LIB=OFF -DUSE_SYSTEM_USRSCTP=OFF \
  -DUSE_SYSTEM_PLOG=OFF \
  -DNO_MEDIA=ON -DNO_WEBSOCKET=ON -DNO_EXAMPLES=ON -DNO_TESTS=ON \
  -DRTC_UPDATE_VERSION_HEADER=OFF
cmake --build "$ROOT/build-nice-min" --target datachannel --parallel 2 -v
cmake --install "$ROOT/build-nice-min"
```

All three commands exited 0. Configure found system libnice `0.1.23` and GLib
`2.88.3`. Verbose compilation proves `RTC_ENABLE_MEDIA=0`,
`RTC_ENABLE_WEBSOCKET=0`, and `USE_NICE=1`. Logs are
`logs/nice-min-{configure,build,install}.txt`; selected cache values are in
`logs/nice-min-cmake-cache-selected.txt`.

### C API lifecycle smoke

The independently linked NICE-MIN smoke executed the same bounded lifecycle
and exited 0:

```text
profile=nice-min pc_created=1 dc_created=1 label=relay-build-smoke close_dc=0 delete_dc=0 close_pc=0 delete_pc=0 cleanup=1
```

Evidence: `smoke/nice-min/capi_smoke.c`,
`logs/nice-min-smoke-{compile,run}.txt`.

### Capability-path and restart boundary

Pinned source assertions prove the NICE branch maps `TurnTcp` to
`NICE_RELAY_TYPE_TURN_TCP` and `TurnTls` to `NICE_RELAY_TYPE_TURN_TLS` at
`src/impl/icetransport.cpp:637-642`. They also prove the wrapper warns that
custom ICE attributes are unsupported with libnice, retains the one-shot
`GatheringState::New` gate, and exposes no C `rtcRestartIce` symbol. The
independent compile-negative again failed as expected.

This establishes that the TCP/TLS **build path exists**, not that it works
against Coturn or a browser. Same-peer restart remains an API hard gap for the
probe; no runtime restart exchange was attempted. Evidence:
`logs/nice-min-source-capability-assertions.txt` and
`logs/nice-min-restart-api-negative.txt`.

### Artifact and link closure

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| `artifacts/nice-min/libdatachannel.so.0.24.5` | 3,162,640 | `2a1fbb8db2a5308da665be1dab4ff149ca5c11cca2da617d057fef61ed7b5f25` |
| `artifacts/nice-min/capi_smoke` | 16,664 | `a44c29ca9897907f87d3b39c5836df5f2a4c550800170965c2c16ca70b24d4ae` |

Direct ELF `NEEDED` adds `libnice.so.10`, `libgobject-2.0.so.0`, and
`libglib-2.0.so.0` to the OpenSSL/C++/libc closure. Current-host `ldd` resolves
49 lines because libnice/GLib transitively pull in GIO, GnuTLS, GUPnP,
libsoup, XML, Kerberos, ICU, and other system libraries. Exact resolved paths
are in `logs/nice-min-ldd-library.txt`; package versions and declared license
identifiers are in `logs/nice-min-link-package-license-closure.txt`. This large
system closure is a packaging/compliance input, not a completed gate.

## License archive

Each profile has its own `artifacts/<profile>/licenses` directory and
`LICENSE-SHA256SUMS`.

- Both archive the pinned libdatachannel MPL-2.0 text, bundled usrsctp license,
  bundled plog MIT text, and installed OpenSSL Apache-2.0 text.
- JUICE-MIN separately archives the exact libjuice MPL-2.0 text.
- NICE-MIN records installed package declarations for libnice
  (`MPL-1.1 OR LGPL-2.1-only`) and GLib (`LGPL-2.1-or-later`) plus all resolved
  closure package identifiers. Their CachyOS packages did not install standalone
  license text files, so a redistribution-ready full notice bundle is
  **missing** and the licensing gate remains `not_run`.
- libsrtp and nlohmann/json were fetched as official gitlinks but are excluded
  by `NO_MEDIA=ON` and `NO_EXAMPLES=ON`; they are not linked into either binary.

This is an inventory, not legal advice or compliance approval.

## T0 manifest and scorecard copies

Separate copies were filled and JSON-parsed:

| Profile | Environment manifest | SHA-256 | Scorecard | SHA-256 |
|---|---|---|---|---|
| JUICE-MIN | `manifests/environment-juice-min.json` | `c137183d3db48c4d18f1270cf2c490fd9ed32f8b7fe778a7e2e3ae61b55ff7f0` | `manifests/scorecard-juice-min.json` | `8d156028468fe676373ede43c666d9087f15679018cd66050484c581f0e9f478` |
| NICE-MIN | `manifests/environment-nice-min.json` | `826ee0c757df2c91630736c9bbf4690358370991ff12a7ddbc392d6217672b8e` | `manifests/scorecard-nice-min.json` | `4bba8e2b1886846343ccec3003104bdcd3cfe562bb496075c24dd551b319bf13` |

Both manifests explicitly mark the build as a partial/no-network probe. Only
Linux x86_64 is enabled; Windows x86_64 and macOS arm64/x86_64 are false/not
run. Browser, Coturn/TLS, image digest, impairment, and live transport fields
are explicitly `NOT_RUN`, missing, empty, or null as appropriate. Both
scorecards keep all seven hard gates at `not_run`, all ratings and total null,
`eligibleForWeightedComparison=false`, and `result=not_evaluated`. Build sizes,
smoke exit codes, evidence paths, and missing-gate rationales are filled, but no
score was opened.

`logs/manifest-validation.txt` records validation. Each profile's
`EVIDENCE-SHA256SUMS` hashes its own logs, manifests, and assertions;
`SHA256SUMS` and `LICENSE-SHA256SUMS` separately hash its binaries and license
archive.

The evidence was also packed without mixing profiles. Both archives reproduced
byte-identically when regenerated with sorted paths, epoch-zero mtimes, and
numeric root ownership:

| Archive | Bytes | SHA-256 |
|---|---:|---|
| `archives/juice-min-evidence.tar.gz` | 1,176,283 | `0d3a6f8d405c65e573e72389ac447c7fd398dd2cad2b285a2a31ef17fe95f5ea` |
| `archives/nice-min-evidence.tar.gz` | 1,092,085 | `2321c755be99c130b7160efb3126ae97dbddbf060d616c57334a5b5a59c35f91` |

## Exact successes and blockers

### Successes

- Official tag/commit and all five exact official gitlinks fetched and verified.
- JUICE-MIN shared Release build/install passed.
- NICE-MIN shared Release build/install passed with installed libnice/GLib.
- Independent compiled C API create/data-channel/close/delete/cleanup smokes
  passed for both profiles.
- Artifact sizes, hashes, ELF metadata, current-host link closures, package
  license identifiers, copied source license texts, CMake cache selections,
  and T0 copies were archived.
- JUICE-MIN's restart and TURN TCP/TLS hard negatives were asserted from the
  exact pinned source; NICE-MIN's TCP/TLS mapping and restart API gap were kept
  separate.

### Blockers / missing evidence

- No immutable build-image digest and no byte-identical clean-rebuild test.
- Three required target builds are missing: Windows x86_64, macOS arm64, and
  macOS x86_64. Static packaging was not built; only the minimal shared shape
  was probed.
- No live browser or Coturn/TLS route, no certificate/configuration, no
  UDP/TCP/TLS relay matrix, and no public-service use.
- No same-peer or replacement-peer recovery run, network-change test,
  backpressure/lifecycle deadline measurement, impairment run, sanitizer run,
  RSS/CPU measurement, or provider adapter.
- NICE-MIN's large rolling-host system dependency closure is recorded but not
  immutably vendored, cross-target packaged, or accompanied by a complete
  redistribution notice bundle.
- Consequently every T0 hard gate and every weighted dimension remains
  unassessed; this probe makes no selection.
