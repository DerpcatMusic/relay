# RELAY plugin

Truce 7.0 shell around `relay-session`. The host audio callback only copies into preallocated rings and renders playback. A worker thread owns UDP sockets.

Home-network sharing uses **5 ms uncompressed stereo PCM** on LAN (no Opus, no FEC lookahead). That is the lowest delay this path can do: one 5 ms packet plus your DAW buffer. It is not zero — nothing in a DAW insert can be — but it is the blazing LAN path.

## You can test now

Installed:

- CLAP: `~/.clap/RELAY.clap`
- VST3: `~/.vst3/RELAY.vst3`
- Standalone: `apps/plugin/target/release/relay-plugin-standalone`
- Public listen: https://relay.matari-audio.com/`<session-name>`

Rescan plugins in the DAW.

## Home LAN (use this)

1. Both machines on the same Wi-Fi / Ethernet.
2. Instance A: **Connect Host**, Port `17492`, set a **Session name**, **Link** on.
3. Instance B: **Connect Join**, Peer = that **session name** (or `192.168.x.x:17492`), **Link** on.
4. Monitor **Remote** or **Mix**. Header: `Connected`.

Loopback (one instance): Product **Loopback**, **Link** on, feed the insert audio.

## Browser listen

https://relay.matari-audio.com/`<session-name>` — click **Listen**. This is a fan-out listen page (higher delay). Musicians on the same house network should use the plugin LAN path, not the browser.

## Products

| Product | What it does |
|---|---|
| Connect Host | Bind UDP 17492, announce the session name on LAN |
| Connect Join | Join `host:port` **or** a LAN session name |
| Stream Hub / Publish / Listen | Local unpaid fan-out (also LAN PCM) |
| Loopback | One instance self-test |
| Web Link | Host + claim/upload to relay.matari-audio.com |

Paid TURN / subscriptions are not implemented. Cross-NAT without a port forward will fail; same LAN does not need TURN.
