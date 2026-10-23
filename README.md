# collie

Watch and answer your [herdr](https://herdr.dev) coding agents from your phone.
A collie works the herd so the herder doesn't have to stand in the field.

Your agents run in herdr on your Mac. collie puts them in your pocket:

- **See every agent at a glance** — which are working, which finished, which
  are stuck waiting on you.
- **Get notified** the moment an agent needs input or finishes, even with the
  app closed.
- **Unblock with one tap** — answer permission prompts (1/2/3/Enter/Esc)
  straight from the notification's agent screen.
- **Send new prompts** and **watch the live terminal**, styled and readable.
- **Private by design** — your Mac talks to your phone directly (ideally over
  your tailnet). No cloud, no account, no third server.

```
┌─────────── Mac ───────────┐          ┌───── Android ─────┐
│ herdr ⇄ collie-bridge     │ ws(s)/   │ Rust core (JNI)   │
│         (Rust, :7717)     │ tailnet  │  ⇄ Compose app    │
└───────────────────────────┘          └───────────────────┘
```

## Requirements

- **Mac**: herdr ≥ 0.9.5, Rust (stable). For building the app: a JDK (17–21)
  and the Android SDK (`brew install --cask temurin@21 android-commandlinetools
  android-platform-tools`, plus `cargo install cargo-ndk` and
  `rustup target add aarch64-linux-android`).
- **Phone**: Android 12+. Tailscale on both devices is the recommended
  transport.

## Quick start

**1. Start the bridge on your Mac** (where herdr runs):

```sh
cargo run -p collie-bridge -- serve
```

It binds `127.0.0.1:7717` and prints what it's doing. `collie-bridge status
--watch` gives you the same live view in the terminal, no phone needed.

**2. Expose it to your phone** (pick one):

| Phone is… | URL to pair with | Setup |
|---|---|---|
| Android emulator on the Mac | `ws://10.0.2.2:7717/ws` | nothing |
| On your tailnet (recommended) | `wss://<mac>.<tailnet>.ts.net/ws` | `tailscale serve --bg --https=443 http://127.0.0.1:7717` |
| Same Wi-Fi | `ws://<mac-LAN-IP>:7717/ws` | `collie-bridge serve --bind 0.0.0.0:7717` — token is the only gate, prefer Tailscale |

Never expose the port publicly (`tailscale funnel`, port-forwards).

**3. Build + install the app** (phone in developer mode, USB attached):

```sh
cd android
JAVA_HOME=$(/usr/libexec/java_home -v 21) ./gradlew installDebug
```

**4. Pair.** Run `collie-bridge pair` on the Mac, type the URL + token into
the app, hit Connect. Done — the overview fills in, and notifications arrive
from your agents.

## Try it without real agents

A sandboxed herdr session with fake agents (separate from your real one, no
tokens spent):

```sh
scripts/demo-session.sh up        # demo herdr + fake agents + bridge on :7718
scripts/demo-session.sh scenario  # statuses churn: working → blocked → done
scripts/demo-session.sh down
```

Pair the app with `ws://10.0.2.2:7718/ws` (emulator) and watch it move.

## Repo layout

| path | what |
|---|---|
| `crates/collie-core` | shared Rust: herdr types, session store, notification dedup, protocol, ANSI→styled frames |
| `crates/collie-bridge` | Mac daemon: herdr socket sync + phone WebSocket server (`serve`, `status`, `pair`) |
| `crates/collie-android` | Rust cdylib behind a 3-function JNI surface |
| `android/` | Kotlin + Jetpack Compose app |
| `scripts/` | `smoke.mjs` (fake phone), `demo-session.sh` + `demo.mjs` (fake agents) |
| `docs/PROTOCOL.md` | bridge ↔ phone protocol |
| `docs/herdr-notes.md` | verified herdr socket facts and sharp edges |

## Security model, in one paragraph

herdr's socket accepts input injection from anyone who can open it, so the
bridge is a privileged client: it binds loopback only, requires a bearer token
(`~/.config/collie/token`, created on first run, 0600) as the first message of
every connection, and will only ever call an explicit allowlist of herdr
methods (reads + `agent.prompt`, `agent.send_keys`, `agent.focus`,
`pane.send_keys`, `pane.send_text`). Remote access is meant to ride your
tailnet — Tailscale authenticates the devices and encrypts the transport, and
`tailscale serve` adds real TLS. The app stores the token in app-private
preferences. Cleartext `ws://` is permitted by the app on purpose for the
emulator and in-tailnet use; don't use it over networks you don't trust.

## Status

0.1. Known gaps: terminal is view + key bar (type prompts from the agent
screen), one bridge per herdr session, per-agent mutes and QR pairing not yet
built. Protocol may change before 1.0 (`PROTO_VERSION` guards it).

## License

MIT
