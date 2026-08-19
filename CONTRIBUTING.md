# Contributing

RELAY is open source under the [Mozilla Public License 2.0](LICENSE).

## Build the plugin

From `apps/plugin`:

```bash
cargo truce install
```

That installs every format declared in this crate's default features:

| Format | Feature | Where it lands |
|---|---|---|
| CLAP | `clap` | `~/.clap/RELAY.clap` |
| VST3 | `vst3` | `~/.vst3/RELAY.vst3` |
| VST2 | `vst2` | `~/.vst/RELAY.so` (Linux) / `~/Library/Audio/Plug-Ins/VST/` (macOS) |
| LV2 | `lv2` | `~/.lv2/relay.lv2` |
| AU v2 | `au` | `~/Library/Audio/Plug-Ins/Components/` (macOS only) |
| AU v3 | `au` | container app via `cargo truce install --au3` (macOS + Xcode) |
| Standalone | `standalone` | `apps/plugin/target/release/relay-plugin-standalone` |

AAX is opt-in (`--features aax` / `cargo truce install --aax`) and needs the Avid AAX SDK. VST2 uses Truce's clean-room shim — no Steinberg SDK.

Subset:

```bash
cargo truce install --clap --vst3 --lv2
cargo truce install --vst2
cargo truce install --au2          # macOS
cargo truce install --au3          # macOS, Xcode
```

Workspace checks live at the repo root (`just check`, `just test`).

## License

By contributing you agree that your changes are licensed under MPL-2.0, same as the rest of the tree. Keep the SPDX identifier `MPL-2.0` on new crates and `package.json` files.
