# herdr protocol notes (verified against 0.9.5, protocol 22)

Facts the bridge is built on, confirmed live on 2026-10-22. Re-verify on herdr
upgrades (`ping` returns `{version, protocol, capabilities}`; an unknown method
error lists every supported method).

## Socket
- `~/.config/herdr/herdr.sock` (mode 0600). Named sessions:
  `~/.config/herdr/sessions/<name>/herdr.sock`. Resolution order:
  `--session` flag → `HERDR_SOCKET_PATH` → `HERDR_SESSION` → default.
- Newline-delimited JSON. **No auth** — file permissions are the only gate, and
  the socket allows input injection into panes. The bridge is therefore a
  privileged client: it authenticates phones itself and keeps a hard-coded
  method allowlist (`herdr_client.rs`).
- **One request per connection**: server reads one line, answers once, closes.
- Request: `{"id":"<string>","method":"dotted.name","params":{...}}`.
  `params` required even when empty; `id` must be a string.
- Limits: request line ≤ 1 MiB within 5 s; server write timeout 5 s with
  ~8 KiB socket buffers → subscription readers must drain promptly.
- Ordinary requests have **no server-side timeout**; we set our own (10 s).
- A launchd-started bridge won't inherit `HERDR_SOCKET_PATH` (only processes
  inside herdr panes get it) — use the default path or `--socket`.
- Stale 0.9.2 binary lives at `~/.cargo/bin/herdr`; the real one is
  `~/.local/bin/herdr`. Never shell out to herdr from the bridge anyway.

## Snapshot
- `session.snapshot` → `{"type":"session_snapshot","snapshot":{...}}`-ish
  (`result.type` + `result.snapshot`). ~7 KB / ~1 ms. Cheap; treat as truth.
- Agent guaranteed fields: `terminal_id`, `agent_status`, `workspace_id`,
  `tab_id`, `pane_id`, `focused`, `revision`, `state_change_seq`. Everything
  else (`name`, `agent`, `title`, `terminal_title*`, `tokens`,
  `agent_session`, `completion_seq`, `cwd`, `foreground_cwd`, `state_labels`)
  is omitted when empty → `Option` in `shepsky-core`.
- IDs (`w15`, `w15:t1`, `w15:p7`) are opaque, persisted across restarts. A
  pane moved across workspaces gets a **new pane id** (`pane.moved` carries
  `previous_pane_id`). `terminal_id` survives moves, not cold restarts.
- `revision` counts title/metadata changes, not output.

## Agent status
- `idle | working | blocked | done | unknown`.
- `done` = finished and not yet viewed. Viewing the tab on **any** client, or
  `agent.focus`, flips `done → idle` **silently (no event)**. herdr's own
  100 ms polling of status-subscribed panes is what catches it.
- `state_change_seq`: one server-global counter, bumped on every agent state
  change; resets on server restart.
- `completion_seq`: the counter value at the agent's last "finished work"
  transition. `(terminal_id, completion_seq)` dedups Finished notifications
  (`shepsky-core::notify`).
- `blocked` detection for Claude/Codex is screen-heuristic based.

## Events (`events.subscribe`)
- First line back is the ack: `{"id":..,"result":{"type":"subscription_started"}}`.
- **Never write to the connection after the request line, and never
  half-close it.** One stray newline or `shutdown(Write)` silently ends the
  stream (observed: dead within 0.11 s, no error line).
- Delivery is batched every 100 ms. Server keeps the last 512 events across
  all types; falling behind yields an `events_lost` error line (with the
  subscribe request's id) and the connection closes → reconnect + resnapshot.
- No replay of events from before the subscription; snapshots and events share
  no sequence number. Events are invalidation signals, not state.
- Lifecycle events need no filter and are delivered with **snake_case** names
  (`{"event":"pane_created","data":{"type":"pane_created","pane":{...}}}`).
- `pane.agent_status_changed` needs **one entry per pane** (one request can
  hold many); delivered with the dotted name and payload
  `{pane_id, workspace_id, agent_status, agent, title, display_agent,
  state_labels}`. Fires on title/label-only churn too → de-duplicate.
- Entries for panes that close go silent (no error); moved panes' entries go
  stale → resubscribe on `pane_created` / `pane_closed` / `pane_moved`.
- If **any** pane in the request doesn't exist, the whole subscription is
  rejected → treat as "stale snapshot", re-snapshot, retry.
- Each subscribed pane makes herdr poll that pane every 100 ms. Fine at ~5
  panes.
- There are **no output events**. `pane.output_changed` cannot be subscribed.

## Reading pane content (M4)
- `pane.read` params: `pane_id`, `source`
  (`visible|recent|recent_unwrapped|detection`), `lines` (≤1000, default 80
  for recent sources), `format` (`text|ansi`), `strip_ansi` (default true).
- **Side effect:** `format:"text"` + source `recent`/`recent_unwrapped` on an
  idle full-screen agent makes herdr scroll the pane via injected mouse-wheel
  input to collect history, then scroll back.
- To stay read-only: `source:"visible"` (or `"detection"`) or `format:"ansi"`.
  Avoid `agent.read`. The terminal mirror uses
  `{source:"visible", format:"ansi", strip_ansi:false}` + frame hashing.

## Write methods the bridge allows
`agent.prompt` (can submit and wait: optional `wait{until,timeout_ms}`),
`agent.send_keys`, `agent.focus` (clears `done`), `pane.send_keys`,
`pane.send_text`. Key strings like `enter`, `esc`, `ctrl+h` are accepted;
`prefix+` bindings are not. For starting agents (`start.rs`):
`workspace.create {cwd, label, focus}` and `tab.create {workspace_id, cwd?,
focus}` both answer with `root_pane.pane_id`; `agent.start {name, kind,
pane_id}` types the kind's executable into that pane's shell and returns at
once. A brand-new pane answers `agent_pane_busy`/`agent_pane_unavailable` until
its shell is at a prompt, so the bridge retries for a few seconds. Names are
unique per session (`agent_name_taken`). `workspace.close {workspace_id}` and
`tab.close {tab_id}` undo a failed start. Renaming and closing agents:
`agent.rename {target, name}` (null clears it; `agent_name_taken`,
`invalid_agent_name`, `agent_not_found` for a pane without an agent, and
`agent_launch_pending` for an agent herdr started whose integration hasn't
reported yet: a pi under a session supervisor may never report, so it stays
pending), `pane.rename {pane_id, label}` (null clears), `pane.close {pane_id}`
(ends the process; the tab's last pane closes the tab, the workspace's last
pane closes the workspace, no error either way). Names and labels persist per
pane. Nothing else is callable — see `ALLOWED_METHODS`.
