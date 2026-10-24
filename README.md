<p align="center"><img src="docs/images/icon.png" width="124" alt="shepsky: a pixel-art black shepsky with green eyes and its tongue out"></p>

<h1 align="center">shepsky</h1>

<p align="center">
Watch and answer your <a href="https://herdr.dev">herdr</a> coding agents from your phone.<br>
Half shepherd, half husky: a shepsky keeps an eye on the herd and runs the news back to you.
</p>

<p align="center">
  <img src="docs/images/demo.gif" width="250" alt="An agent asks for permission; the phone gets a notification, opens the agent, answers 1, and the agent goes back to work.">
  <img src="docs/images/overview.png" width="250" alt="The overview: the agent that needs you pinned on top, the rest by workspace, with a sort line.">
  <img src="docs/images/chat.png" width="250" alt="An agent's conversation: the prompt, its replies, and tool calls with their results.">
</p>

Your agents run in herdr on your Mac. shepsky puts them in your pocket:

- **See every agent at a glance.** Which are working, which finished, which
  are stuck waiting on you. Agents that need you are pinned on top; the rest
  sort by workspace, name or most recent.
- **Get notified** the moment an agent needs input or finishes, even with the
  app closed (and after a reboot).
- **Unblock with one tap.** A notification opens the agent with its question
  on screen, exactly as the agent drew it, so you read it before answering
  1/2/3/Enter/Esc.
- **Read the conversation, not just the screen.** Prompts, replies, tool calls
  and their results, from each agent's own transcript (Claude Code, Codex,
  pi), with the whole history a scroll away.
- **Watch the live terminal** one tab over, and scroll back through its
  history.
- **Send prompts**, and **start, rename or close agents**: claude, codex or pi,
  in a new workspace or a new tab of one you have.
- **In your herdr theme**, or any of herdr's 18.
- **Private by design.** Your Mac talks to your phone directly (ideally over
  your tailnet). No cloud, no account, no third server.

<p align="center">
  <img src="docs/images/agent.png" width="250" alt="An agent waiting on a permission question: its screen at the bottom of the chat, and the quick-reply keys lit up.">
  <img src="docs/images/terminal.png" width="250" alt="The terminal tab scrolled back through herdr's history, with a scrollback bar and a [live] button.">
  <img src="docs/images/notifications.png" width="250" alt="Needs-input notifications in the Android shade.">
</p>

```
┌─────────── Mac ───────────┐          ┌───── Android ─────┐
│ herdr ⇄ shepsky-bridge    │ ws(s)/   │ Rust core (JNI)   │
│         (Rust, :7717)     │ tailnet  │  ⇄ Compose app    │
└───────────────────────────┘          └───────────────────┘
```

## Try it on your Mac (no phone, no real agents)

```sh
scripts/try-it.sh
```

This boots an Android emulator on your Mac (creating it the first time),
builds and installs the app, and starts a **demo**: three fake agents in a
sandboxed herdr session, separate from your real one, that spend no tokens.
The app opens with pairing filled in; tap **Connect**. Then:

- tap an agent that **needs input** and answer it with **1 / 2 / 3**;
- send an idle agent a prompt and watch its conversation;
- pull past the top of the **terminal** tab to scroll back through its log;
- tap **[+]** to start another (fake) agent, or long-press one to rename or
  close it;
- leave the app: notifications arrive when an agent needs you.

`scripts/try-it.sh down` stops the demo and the emulator. The script installs
the missing Android SDK packages, `cargo-ndk` and the Rust Android target
itself; you need herdr, Rust, Node and a JDK (see below). The first run
downloads a few GB (SDK, emulator image, Gradle); later runs take seconds.

## Requirements

- **Mac**: herdr ≥ 0.9.5 (with its Claude Code, Codex and pi integrations
  installed: `herdr integration status`), Rust (stable), Node.js (for the
  demo only).
- **Building the app** (`try-it.sh` sets up everything but the JDK):
  - a JDK 17–21: `brew install --cask temurin@21`
  - the Android SDK with `platform-tools`, `platforms;android-35`,
    `build-tools;35.0.0` and an NDK (`ndk;27.3.13750724`), via Android Studio
    or `brew install --cask android-commandlinetools`. Gradle finds it through
    `ANDROID_HOME` or `android/local.properties` (`sdk.dir=…`).
  - `cargo install cargo-ndk` and `rustup target add aarch64-linux-android`
  - build with the bundled `android/gradlew`, not Homebrew's `gradle` (Gradle 9
    can't run this Android plugin).
- **Phone**: Android 12+. Tailscale on both devices is the recommended
  transport.

## Quick start

**1. Start the bridge on your Mac** (where herdr runs):

```sh
cargo install --path crates/shepsky-bridge   # puts shepsky-bridge on your PATH
shepsky-bridge serve
```

(or straight from the repo: `cargo run --release -p shepsky-bridge -- serve`).
It binds `127.0.0.1:7717` and logs what it's doing. `shepsky-bridge status
--watch` shows the same agents in the terminal, no phone needed.

**2. Expose it to your phone** (pick one):

| Phone is… | URL to pair with | Setup |
|---|---|---|
| Android emulator on the Mac | `ws://10.0.2.2:7717/ws` | nothing |
| On your tailnet (recommended) | `wss://<mac>.<tailnet>.ts.net:7443/ws` | `tailscale serve --bg --https=7443 http://127.0.0.1:7717` |
| Same Wi-Fi | `ws://<mac-LAN-IP>:7717/ws` | `shepsky-bridge serve --bind 0.0.0.0:7717`: the token is the only gate, so prefer Tailscale |

Never expose the port publicly (`tailscale funnel`, port-forwards). Port 7443
keeps the bridge tailnet-only even if you already serve other things: Funnel
only works on 443, 8443 and 10000, so a bridge on 7443 can't end up public by
accident (check with `tailscale serve status`).

**3. Build + install the app** (phone in developer mode, USB attached):

```sh
cd android
JAVA_HOME=$(/usr/libexec/java_home -v 21) ./gradlew installRelease
```

The release build is about 4 MB. It's signed with Android's debug key, so it
installs over a debug build in place; publishing it anywhere would need a real
signing key. `installDebug` still works for development.

**4. Pair.** Run `shepsky-bridge pair` on the Mac, type the URL + token into
the app, hit Connect. Done: the overview fills in, and notifications arrive
from your agents.

<p align="center">
  <img src="docs/images/pair.png" width="250" alt="Settings: the pairing form and the theme picker.">
  <img src="docs/images/new-agent.png" width="250" alt="Starting an agent: claude, codex or pi; a new workspace in a directory, or a new tab in one you have; an optional name.">
  <img src="docs/images/actions.png" width="250" alt="Long-press an agent: rename or close.">
</p>

Opening `shepsky://pair?url=<url-encoded URL>&token=<token>` on the phone fills
in the pairing screen for you (it never connects until you tap Connect).

## Using it

- **The overview** pins agents that need you on top. The sort line orders the
  rest by workspace (`space`, the default), `name`, or `recent` (the last
  state change herdr saw). Long-press an agent to rename or close it; **[+]**
  starts a new one.
- **An agent** opens on its **chat**: the conversation from the agent's own
  transcript, with tool calls folded to one line (tap for the full input and
  output) and thinking folded too. While it waits on you, its question sits at
  the bottom, exactly as it drew it, and the number keys light up. A finished
  or idle agent takes a prompt from the line at the bottom.
- **The terminal tab** mirrors the agent's pane. Pull past its top to scroll
  back through herdr's history and past its bottom to come forward; **[live]**
  jumps back. That scroll is herdr's own, so the pane on your Mac scrolls with
  it; the app puts it back at the bottom when you leave the tab or the agent,
  or the app goes to the background.

## Themes

shepsky looks like what it is, a herdr client: a terminal, not a Material app.
It follows herdr's theme (the bridge reads `[theme]` from herdr's
`config.toml`, auto-switch included), or you can pick any of herdr's 18
built-in themes in Settings. Terminal output is drawn with the theme's own 16
colors.

<p align="center">
  <img src="docs/images/themes.png" width="760" alt="The same question in dracula, rose-pine and rose-pine-dawn.">
</p>

## The demo, piece by piece

`try-it.sh` is a wrapper around `scripts/demo-session.sh`, which you can run on
its own (e.g. with your own emulator or phone):

```sh
scripts/demo-session.sh up     # demo herdr session + fake agents + bridge on :7718
scripts/demo-session.sh down
```

- The demo runs its own herdr session (`shepsky-demo`) and its own bridge on
  port 7718, so your real session and a real bridge on 7717 are untouched.
- The fake agents (`scripts/fake-agent.mjs`) report their state to herdr like
  a real integration. They work through a task, stop to ask permission
  (answer with 1/2/3/Enter/Esc), take new prompts when idle, start with an
  earlier log to scroll back through, and keep a Claude-style transcript for
  the chat view.
- Agents you start from the app in the demo are fake ones too (the bridge's
  `--demo-agent`).
- The demo bridge runs with `--demo`. herdr only delivers agent input to a
  **real** agent process (see Security), which a Node script isn't, so the demo
  bridge types into the demo panes directly instead. It still refuses prompts
  while an agent is asking a question, like herdr does. Never use `--demo` on
  your real session.
- If an emulator with the app is running, `up` fills in its pairing screen
  (`ANDROID_SERIAL` picks one when several run). Otherwise pair with
  `ws://10.0.2.2:7718/ws` and the token it prints.

## Repo layout

| path | what |
|---|---|
| `crates/shepsky-core` | shared Rust: herdr types, session store, notification dedup, protocol, ANSI→styled frames, agent transcripts → chat |
| `crates/shepsky-bridge` | Mac daemon: herdr socket sync + phone WebSocket server (`serve`, `status`, `pair`) |
| `crates/shepsky-client` | the phone's connection runtime (WebSocket, auth, reconnects) |
| `crates/shepsky-android` | Rust cdylib behind a 3-function JNI surface |
| `android/` | Kotlin + Jetpack Compose app |
| `scripts/` | `try-it.sh` (emulator + app + demo), `demo-session.sh` + `demo.mjs` + `fake-agent.mjs` (the demo), `smoke.mjs` (fake phone) |
| `docs/PROTOCOL.md` | bridge ↔ phone protocol |
| `docs/herdr-notes.md` | verified herdr socket facts and sharp edges |
| `docs/images/` | README screenshots and the icon |
| `assets/logo/` | the logo's pixel grid and the script that exports every icon |
| `third_party/` | licenses for bundled assets (Fira Code) |

## Security model

- **The bridge is the gatekeeper.** herdr's socket accepts input injection
  from anyone who can open it, so the bridge binds loopback only, requires a
  bearer token (`~/.config/shepsky/token`, created on first run, 0600) as the
  first message of every connection, and only ever calls an explicit
  allowlist of herdr methods (`ALLOWED_METHODS` in
  `crates/shepsky-bridge/src/herdr_client.rs`): reads, plus
  - input: `agent.prompt`, `agent.send_keys`, `agent.focus` (and
    `pane.send_keys`/`pane.send_text`, used only by `--demo`);
  - starting agents: `workspace.create`, `tab.create`, `agent.start` (plus
    `workspace.close`/`tab.close`, only to undo a start that failed);
  - managing agents: `agent.rename`, `pane.rename`, `pane.close`;
  - scrollback: `pane.scroll`.
- **Anyone with your token can drive your agents.** That's the point, but it
  means: send prompts and keys, start agents, rename and close them, and
  scroll their panes. Guard the token like an SSH key.
- **shepsky can't type into a shell.** Prompts go through herdr's
  `agent.prompt` and keys through `agent.send_keys`. herdr only delivers them
  when the pane's foreground process really is that agent, and refuses a
  prompt while the agent is waiting on a question. If the agent has exited
  and left a shell behind, nothing is typed; the app says so. The pane-level
  methods are used only by `--demo`, for the sandboxed demo session.
- **Agent panes only.** Prompts, keys, focus, rename, close and scroll are
  refused for any pane that doesn't host an agent. Closing asks herdr once
  more right before, so a pane whose agent just exited is never closed, and
  the app asks you first.
- **Starting agents is fenced in.** The phone can start only claude, codex
  or pi, with no command-line arguments (so no flags that skip their
  permission prompts), and only in an existing directory inside your home
  folder. The bridge checks all of this; the app only asks.
- **Conversations come from the agents' own logs.** The chat view reads the
  transcript herdr's integration points to, and only inside that agent's
  folder (`~/.claude/projects`, `~/.codex/sessions`, `~/.pi/agent/sessions`);
  the phone asks for a pane, never a file. Transcripts hold more than the
  screen (full tool output, the agent's thinking when it shares it), so the
  token guards that too.
- **Remote access rides your tailnet.** Tailscale authenticates the devices
  and encrypts the transport, and `tailscale serve` adds real TLS. Never use
  `tailscale funnel` or a port-forward.
- **On the phone**, the token lives in app-private preferences. Cleartext
  `ws://` is allowed on purpose for the emulator and in-tailnet use; don't use
  it over networks you don't trust. A pairing link only fills in the form:
  check the URL before you tap Connect.

shepsky was called collie before 0.1: the bridge copies an old
`~/.config/collie/token` to `~/.config/shepsky/token` on first run.

## Status

0.1. Known gaps:

- **Notifications are live only.** If an agent finishes or starts asking
  while your phone is offline, the overview catches up when it reconnects,
  but there's no notification for what you missed.
- **The terminal tab isn't a full terminal.** It mirrors the pane, and its
  scrollback moves herdr's own view, so your Mac scrolls with it while you
  read.
- **Chat covers Claude Code, Codex and pi**; other agents get the terminal.
  Codex doesn't tell herdr which conversation a pane is in, so the bridge
  works it out from the directory, the thread name and what's on screen; a
  codex that hasn't started a conversation yet shows the terminal.
- **Renaming a just-started agent** waits until its herdr integration
  reports in. A pi run under a session supervisor
  may never report, and closing its pane can leave that pi process running.
- **Opening a *finished* agent** marks it seen, which also switches herdr on
  your Mac to that agent's tab.
- One bridge per herdr session; per-agent mutes and QR pairing aren't built
  yet. The protocol may change before 1.0 (`PROTO_VERSION` guards it).

## Credits

- [Fira Code](https://github.com/tonsky/FiraCode) by Nikita Prokopov, bundled
  under the SIL Open Font License (`third_party/fira-code/OFL.txt`).
- Terminal palettes from
  [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes)
  (MIT) and the themes' own authors; accents from herdr.
- The logo's green and cream are [Dracula](https://draculatheme.com)'s
  (MIT).

## License

MIT
