# bridge ↔ phone protocol (proto 2)

One WebSocket (`/ws` on the bridge), JSON text messages, tagged by `"type"`.
Both sides ignore unknown fields. The canonical types live in
`crates/shepsky-core/src/protocol.rs`; the Kotlin mirrors live in
`android/app/src/main/java/dev/shepsky/core/Protocol.kt`. Changing a shape is a
protocol break: bump `PROTO_VERSION` in both places.

## Model

- The bridge pushes the **full `state`** on every change (it's a few KB).
  No deltas: reconnect = `auth`, then the next `state` replaces everything.
- `notify` and `terminal_frame` are side streams; missing one is harmless
  (frames are full screens, notifications re-derive from transitions).
- Writes are fire-and-ack: client attaches an optional `op_id`, bridge answers
  `ack` or `error` with the same `op_id`.

## Handshake

1. client connects and sends `{"type":"auth","token":"<hex>"}` within 10 s
2. bridge replies `hello`, then an immediate full `state`
3. bad/missing auth → `error{code:"unauthorized"}` + close

## phone → bridge

| message | fields | notes |
|---|---|---|
| `auth` | `token` | must be first |
| `watch_pane` | `pane_id` | start `terminal_frame`s (≤ 8 watches/conn) |
| `unwatch_pane` | `pane_id` | |
| `watch_chat` | `pane_id` | start `chat` pushes: the agent's conversation (≤ 4 watches/conn) |
| `unwatch_chat` | `pane_id` | |
| `chat_older` | `pane_id` | one more page before the oldest item sent |
| `prompt` | `pane_id`, `text`, `op_id?` | submit a prompt to the agent (≤ 8 KiB) |
| `send_keys` | `pane_id`, `keys[]`, `op_id?` | e.g. `["1"]`, `["enter"]`, `["esc"]` (≤ 32 keys) |
| `focus_agent` | `pane_id`, `op_id?` | marks looked-at; clears `done` |
| `rename_agent` | `pane_id`, `name`, `op_id?` | herdr agent name and the pane's label |
| `close_agent` | `pane_id`, `op_id?` | close the agent's pane, ending it (agent panes only) |
| `start_agent` | `kind`, `workspace_id?`, `cwd?`, `name?`, `label?`, `op_id?` | start an agent; `ack` carries its `pane_id` |
| `ping` | | → `pong` |

## bridge → phone

| message | fields |
|---|---|
| `hello` | `proto`, `bridge_version`, `herdr_version`, `herdr_theme?: {name, dark?, light?}`, `start_kinds?` |
| `state` | `state: {herdr_version, herdr_connected, workspaces[], agents[], places[]}` |
| `notify` | `note: {kind: needs_input\|finished, pane_id, agent_label, title?, workspace_label?}` |
| `terminal_frame` | `pane_id`, `seq`, `frame: {cols, rows, lines: [[span]]}` |
| `watch_error` | `pane_id`, `message` (watch stays alive; pane may be closing) |
| `chat` | `pane_id`, `mode: reset\|append\|older`, `items[]`, `has_older`, `unavailable?` |
| `ack` | `op_id?`, `pane_id?` |
| `error` | `code`, `message`, `op_id?` |
| `pong` | |

`herdr_theme` is herdr's `[theme]` from its config.toml, by canonical name
(`rose-pine`, `catppuccin-latte`…); `dark`/`light` are set when herdr's
`auto_switch` is on. Re-read on every connection.

`places[]` are directories worth offering for a new workspace: where your
workspaces and agents are, most-used first, home as `~`.

`agents[]` is pre-sorted needs-attention-first:
`{pane_id, terminal_id, status: idle|working|blocked|done|unknown, label,
kind?, title?, workspace_id, workspace_label?, cwd?, focused, state_change_seq}`
(`state_change_seq`: herdr's counter at the agent's last state change; bigger
is more recent). `cwd` shows
the Mac's home directory as `~`.

Terminal spans: `{t, fg?, bg?, b?, i?, u?, r?}`. A color is either a
**number** 0–15 (an ANSI color, drawn in the phone's theme) or a `#rrggbb`
**string** (the 256-color cube, grayscale ramp and truecolor are exact);
absent means the theme's default fg/bg. `r` is reverse video: the phone swaps
fg and bg after resolving them. Flags absent = false. Lines are pre-trimmed
of unstyled trailing blanks.

## Chat

The chat view is the agent's conversation read from its own transcript
(Claude Code, Codex and pi keep JSONL logs), not from the screen. herdr's
integrations report which session each agent is in (`agent_session`); the
bridge looks that up only inside the agent's own folder (`~/.claude/projects`,
`~/.codex/sessions`, `~/.pi/agent/sessions`). The phone names a pane, never a
file. Codex often doesn't report, so the bridge falls back to the newest
recent rollout started in the agent's directory, told apart by thread name
when codex agents share one. With nothing to go on, `chat` comes back as a
`reset` with no items and an `unavailable` reason; the terminal still works.

`chat` modes: `reset` replaces what the phone holds (the latest page, or a
new conversation), `append` adds items or replaces ones with the same `id`
(a tool finishing), `older` goes in front. Items are ordered by `id`.

An item is `{id, kind, ...}`:

| kind | fields |
|---|---|
| `prompt` | `text` |
| `reply` | `text` (markdown-ish) |
| `thinking` | `text?` (absent when the agent keeps it private) |
| `tool` | `name`, `summary`, `detail?`, `output?`, `status: running\|ok\|error` |
| `question` | `questions: [{header?, question, options[], multi?}]`, `answer?` |
| `notice` | `text` (compaction, interrupts, slash commands) |

Long text is trimmed (output keeps its start and end).

## Renaming and closing agents

`rename_agent` sets herdr's agent name (`agent.rename`) and the pane's label
(`pane.rename`) to the same name; names follow herdr's rule (see below).
`close_agent` closes the agent's pane (`pane.close`), which ends the agent and
whatever it was doing. Both refuse a pane that doesn't host an agent
(`not_found`); for a close, herdr is asked again (`agent.get`) right before
it, so a pane whose agent just exited and left a shell is never closed.
Closing a tab's last pane closes the tab; a workspace's last pane, the
workspace.

## Starting agents

`start_agent` opens a new tab in `workspace_id`, or (without one) a new
workspace at `cwd` named `label` (default: the directory's name), and starts
a `kind` agent there with herdr's `agent.start`. The bridge's guardrails:

- `kind` must be one of `hello.start_kinds` (`claude`, `codex`, `pi`);
- no command-line arguments, so agents keep their usual permission prompts;
- `cwd` (`~`, `~/…` or absolute) must be an existing directory inside the
  Mac user's home, after resolving symlinks;
- `name` follows herdr's rule (lowercase letter, then lowercase letters,
  digits, `-`, `_`; ≤ 32); absent, the bridge picks `claude`, `claude-2`…

If the agent doesn't start, the workspace/tab made for it is closed again.
With `serve --demo --demo-agent <cmd>`, the bridge types `<cmd> --kind <kind>`
into the new pane instead (the demo's fake agents).

Proto 2 (theme-aware colors and `r`) replaced proto 1's fixed xterm hex for
ANSI 0–15 and pre-swapped reverse video.

## Error codes

Bridge: `unauthorized`, `bad_message`, `not_found`, `too_many_watches`,
`too_large`, `bad_keys`, and for `start_agent`: `unsupported_kind`,
`bad_directory`, `invalid_agent_name` (also for `rename_agent`).

herdr's own codes pass through on failed writes, notably `agent_blocked`
(prompt refused while the agent waits on a question), `agent_not_ready` (the
pane's foreground process isn't the agent, so nothing was typed) and
`agent_not_found`; for `start_agent`, `agent_name_taken` and
`agent_pane_busy`; for `rename_agent`, `agent_name_taken` and
`agent_launch_pending` (an agent just started can't be renamed until herdr
hears from its integration). Anything else without a herdr code is `herdr_error`.

The phone's native core answers `not_connected` (with the command's `op_id`)
while the link is down, and reports a rejected token as the final connection
phase `unauthorized` instead of retrying.

## Trying it

```sh
cargo run --release -p shepsky-bridge -- serve &
node scripts/smoke.mjs                      # hello + state
node scripts/smoke.mjs --watch-pane w1:p1   # + first terminal frame
```
