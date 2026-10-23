# bridge ↔ phone protocol (proto 1)

One WebSocket (`/ws` on the bridge), JSON text messages, tagged by `"type"`.
Both sides ignore unknown fields. The canonical types live in
`crates/collie-core/src/protocol.rs`; the Kotlin mirrors live in
`android/app/src/main/java/dev/collie/core/Protocol.kt`. Changing a shape is a
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
| `prompt` | `pane_id`, `text`, `op_id?` | submit a prompt to the agent (≤ 8 KiB) |
| `send_keys` | `pane_id`, `keys[]`, `op_id?` | e.g. `["1"]`, `["enter"]`, `["esc"]` (≤ 32 keys) |
| `focus_agent` | `pane_id`, `op_id?` | marks looked-at; clears `done` |
| `ping` | | → `pong` |

## bridge → phone

| message | fields |
|---|---|
| `hello` | `proto`, `bridge_version`, `herdr_version` |
| `state` | `state: {herdr_version, herdr_connected, workspaces[], agents[]}` |
| `notify` | `note: {kind: needs_input\|finished, pane_id, agent_label, title?, workspace_label?}` |
| `terminal_frame` | `pane_id`, `seq`, `frame: {cols, rows, lines: [[span]]}` |
| `watch_error` | `pane_id`, `message` (watch stays alive; pane may be closing) |
| `ack` | `op_id?` |
| `error` | `code`, `message`, `op_id?` |
| `pong` | |

`agents[]` is pre-sorted needs-attention-first:
`{pane_id, terminal_id, status: idle|working|blocked|done|unknown, label,
kind?, title?, workspace_id, workspace_label?, cwd?, focused}`.

Terminal spans: `{t, fg?, bg?, b?, i?, u?}` — colors are `#rrggbb`, absent
means the client's default fg/bg; flags absent = false. Lines are pre-trimmed
of unstyled trailing blanks.

## Error codes

`unauthorized`, `bad_message`, `not_found`, `too_many_watches`, `too_large`,
`bad_keys`, `herdr_error` (message carries herdr's own code + text).

## Trying it

```sh
cargo run -p collie-bridge -- serve &
node scripts/smoke.mjs                      # hello + state
node scripts/smoke.mjs --watch-pane w1:p1   # + first terminal frame
```
