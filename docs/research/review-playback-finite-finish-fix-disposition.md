# Playback finite-finish fix disposition

**Disposition: PASS**

## Finding count

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 0 |
| Medium | 0 |

No release-blocking or acceptance-evidence finding remains in the reviewed playback
finite-finish scope.

## H1 fix disposition: closed

The callback-facing correction at `crates/relay-audio/src/playback.rs:1021-1056`
now distinguishes producer abandonment from terminal drain completion:

- `ReadState::Disconnected` becomes `RenderState::Disconnected` only when the
  **post-read** readable count is zero (`:1043-1045`).
- If an abandoned producer still has queued samples and the callback was filled, the
  callback reports `Complete` (`:1046-1048`).
- If an abandoned producer still has queued samples after a short read, the callback
  reports `Underrun` (`:1049`).

This is sound for the SPSC lifecycle: once `AudioConsumer::read` has observed the
producer abandoned, no producer can publish more data, so its post-read readable
count can only represent the remaining buffered tail. The callback still zeroes the
whole destination first and the ring read overwrites its leading prefix; therefore a
short final callback retains every rendered tail sample and zeroes only the missing
suffix.

The committed regression
`playback::tests::disconnected_is_terminal_only_after_the_post_read_queue_is_empty`
(`crates/relay-audio/src/playback.rs:1672-1726`) reproduces the prior abandoned-producer
case with more than a four-sample callback queued. It:

1. collects an uninterrupted reference tail;
2. drops the subject producer with that tail buffered;
3. drains in four-sample callbacks;
4. rejects `Disconnected` while any post-read samples remain;
5. accepts terminal state only at queue zero; and
6. compares the complete rendered tail sample-for-sample with the reference.

It passed independently in debug and release. Inspection also confirms that the
terminal callback's `rendered_samples` prefix is not discarded when the request is
larger than the remaining tail.

## Required committed evidence

### Real 44.1 kHz / 20 ms finite path

`crates/relay-audio/tests/playback_finite_finish.rs:64-174` is a real public-path
regression rather than a synthetic `PcmFrame` test. It proves:

- 1,000 stereo source frames at 44.1 kHz complete through `FiniteTxWorker` with
  `ZeroPad` as two Opus packets;
- the authoritative TX manifest is exactly 129 valid plus 831 zero-padded media
  frames, totaling one 960-frame packet;
- RX preserves one-packet lookahead: the first tick produces no output, the next
  tick supplies only the ordinary first frame;
- the final frame is withheld from ordinary playback, returned by `RxWorker::drain`
  once, passed to `FinitePlaybackEnd::Final` with
  `tx_report.final_valid_media_frames`, and a second drain returns `None`;
- the final input consumption is exactly the manifest's 129-frame valid prefix;
- retained finish output reaches zero pending frames; and
- after producer loss, callback collection preserves every queued sample and reports
  terminal state only at post-read queue zero.

The test passes in debug and release.

### Lifecycle, retention, and accounting

The focused unit regressions at `crates/relay-audio/src/playback.rs:1728-1837` prove:

- a second `Final` is rejected with `InvalidTransition` and does not change metrics or
  repeat terminal conversion;
- renderer loss after a real partial finish publication returns
  `RendererDisconnected`, preserves the exact unpublished pending count, changes the
  worker to `Faulted`, and remains sticky on `Continue`; and
- for 0, 1, 2, and 7 prior streaming transactions, collected playback satisfies the
  strong `collected - L == S + G - L - T` relation while all samples remain finite.

The broader finite matrix continues to cover all supported playback rates, all
5/10/20 ms durations, and full/partial valid prefixes. Ring-backpressured retry output
still matches a spacious one-shot reference without loss, duplication, or drop.

### Allocation

`crates/relay-audio/tests/playback_finish_allocation.rs:36-131` uses the same real
44.1 kHz TX/RX setup and distinct ordinary/withheld frames. After prewarming and
resetting the counter, ordinary playback, `Final`, idempotent `Continue`, and render
perform zero allocations. Both normal and finish workspace pointers/capacities remain
stable. This test passes in debug and release.

The callback path itself remains bounded: fixed zero-fill plus bounded SPSC copy and
constant-time state checks; no allocation, lock, wait, logging, I/O, codec, or SRC is
introduced on the audio thread.

## Validation

Executed from `/mnt/Windows11/DEV_PROJECTS/Repos/relay` without modifying production
or test source and without creating temporary test artifacts.

| Command | Result |
|---|---|
| `cargo test -p relay-audio --lib --all-features --locked disconnected_is_terminal_only_after_the_post_read_queue_is_empty -- --nocapture` | **PASS** — 1 focused H1 regression. |
| Release form of the focused H1 command | **PASS** — 1 focused H1 regression. |
| `cargo test -p relay-audio --test playback_finite_finish --all-features --locked -- --nocapture` | **PASS** — real TX/RX/playback regression. |
| Release form of the real finite-path command | **PASS**. |
| `cargo test -p relay-audio --all-targets --all-features --locked` | **PASS** — 85 tests across all relay-audio targets. |
| `cargo test --release -p relay-audio --all-targets --all-features --locked` | **PASS** — same 85 tests. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | **PASS**. |
| `cargo deny --locked check licenses advisories sources bans` | **PASS** — advisories, bans, licenses, and sources all `ok`; only the three configured unused-license allowances produced non-failing `license-not-encountered` warnings. |
| `cargo fmt --all -- --check` | **PASS**. |

## Scope limitation: libopus pre-skip

This acceptance proves declared source/media accounting, explicit TX zero-padding,
RX lookahead/drain identity, adaptive SRC leading/trailing trim, retained publication,
and callback drain completion. It does **not** prove or claim libopus encoder-lookahead
or container pre-skip compensation. The current `relay-opus` API exposes no container
pre-skip contract; any product requirement for codec pre-skip compensation needs a
separate codec-level design and gate.

## Final disposition

**PASS — C0 / H0 / M0.** The prior premature `Disconnected` defect is corrected, the
buffered final callback tail is rendered completely, and the formerly missing real
manifest/lifecycle/accounting/allocation regressions are present and passing in both
debug and release.
