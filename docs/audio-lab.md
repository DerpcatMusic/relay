# RELAY audio lab

`relay-audio-lab` is the Phase-1 headless integration executable. It requires no
audio device, socket, async runtime, thread, sleep, or wall clock. The command
runs the public production composition path:

```text
synthetic stereo PCM -> fixed capture SRC -> Opus TX -> bounded deterministic
network -> RX reorder / real Opus FEC-or-PLC / PLC -> scheduled-playout-only
adaptive SRC -> SPSC playback ring -> renderer -> off-thread final diagnostics
```

The renderer is called with caller-owned buffers and never prints. Only `main`
formats the final snapshot after all RX lookahead and ring audio have drained.

## Deterministic headless proof

```sh
cargo run --locked -p relay-audio-lab -- \
  --capture-rate 44100 --playback-rate 96000 \
  --packet-ms 10 --duration-ms 500 --profile clean --seed 1 --json
```

Supported capture/playback rates are 44100, 48000, 96000, and 192000 Hz.
Packet duration is 5, 10, or 20 ms. Duration must be a multiple of 10 ms in
`50..=10000`. `clean` uses ordered virtual delivery. `impaired` adds one early
duplicate plus fixed-seed bounded delay/reorder and loss:

```sh
cargo run --locked -p relay-audio-lab -- \
  --duration-ms 500 --packet-ms 20 --profile impaired --seed 7
```

The summary reports actual captured/rendered frames, encoded and emitted RX
frames, the observed final lookahead drain, scheduler drops/duplicate requests/
scheduled duplicate copies, RX-accepted packets and observed duplicate
rejections, honest Opus `FEC-or-PLC` attempts and explicit PLC, SPSC
drop/underrun/high-water values, configured **nominal** rates, playback frame
accounting, and a rendered-sample checksum. Scheduler requests and RX-observed
duplicates are deliberately separate. `FEC-or-PLC` is not labeled "FEC
recovered" because libopus does not expose proof that LBRR data was present.
Worker/publication faults are fatal and produce a nonzero process exit rather
than a success-only counter. The synchronous headless resources leave scope
before output, but no native host callback acknowledgement exists, so the
summary makes no clean-shutdown claim.

Packet delivery uses only `NetworkTime`. Playback media position is reconstructed
from extended sequence and the fixed RTP epoch, and its local position from
scheduled device frames; arrival time never enters clock recovery.

## Device mode

`--device` exits nonzero with an explicit unavailable message. No native device
dependency is added merely to make headless CI green. A physical-device adapter,
host callback acknowledgement, deadline measurements, and manual device smoke
remain external platform gates. Endpoint and metrics destruction must occur on
the control thread after a future host stops and acknowledges the callback.

## Validation

```sh
cargo fmt --all -- --check
cargo test --locked -p relay-audio-lab --all-targets
cargo test --release --locked -p relay-audio-lab --all-targets
cargo clippy --locked -p relay-audio-lab --all-targets -- -D warnings
cargo run --locked -p relay-audio-lab -- --json --duration-ms 100
```

The integration tests parse and repeat identical JSON, exercise the complete
4-by-4 capture/playback-rate cross-product at every packet duration, assert
packet/emission/final-drain identities, run both advertised duration boundaries, distinguish
scheduler and RX duplicate facts, validate human labels, and require unsupported,
missing, device, and out-of-range configurations to fail without panic.
