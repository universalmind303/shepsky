#!/usr/bin/env node
// Smoke-test a running shepsky-bridge without a phone.
//
//   node scripts/smoke.mjs                      # auth + hello + state summary
//   node scripts/smoke.mjs --watch-pane w1:p1   # ...plus first terminal frame
//   node scripts/smoke.mjs --focus w1:p1        # ...plus a focus_agent round-trip
//
// Env: SHEPSKY_URL (default ws://127.0.0.1:7717/ws),
//      SHEPSKY_TOKEN (default: read ~/.config/shepsky/token)

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const url = process.env.SHEPSKY_URL ?? "ws://127.0.0.1:7717/ws";
const token =
  process.env.SHEPSKY_TOKEN ??
  readFileSync(join(homedir(), ".config/shepsky/token"), "utf8").trim();

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const watchPane = flag("--watch-pane");
const focusPane = flag("--focus");

const want = new Set(["hello", "state"]);
if (watchPane) want.add("terminal_frame");
if (focusPane) want.add("ack");

const ws = new WebSocket(url);
const timeout = setTimeout(() => {
  console.error(`TIMEOUT — still waiting for: ${[...want].join(", ")}`);
  process.exit(1);
}, 15_000);

ws.onopen = () => {
  ws.send(JSON.stringify({ type: "auth", token }));
  if (watchPane) ws.send(JSON.stringify({ type: "watch_pane", pane_id: watchPane }));
  if (focusPane)
    ws.send(JSON.stringify({ type: "focus_agent", pane_id: focusPane, op_id: "smoke-1" }));
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  switch (msg.type) {
    case "hello":
      console.log(`hello: proto=${msg.proto} bridge=${msg.bridge_version} herdr=${msg.herdr_version}`);
      break;
    case "state": {
      const { agents, workspaces, herdr_connected } = msg.state;
      console.log(
        `state: herdr_connected=${herdr_connected} workspaces=${workspaces.length} agents=${agents.length}`,
      );
      for (const a of agents)
        console.log(`  [${a.status}] ${a.label} @ ${a.workspace_label ?? a.workspace_id} (${a.pane_id})`);
      break;
    }
    case "terminal_frame":
      console.log(
        `terminal_frame: pane=${msg.pane_id} seq=${msg.seq} ${msg.frame.cols}x${msg.frame.rows}`,
      );
      if (watchPane) ws.send(JSON.stringify({ type: "unwatch_pane", pane_id: watchPane }));
      break;
    case "ack":
      console.log(`ack: op_id=${msg.op_id}`);
      break;
    case "notify":
      console.log(`notify: ${msg.note.kind} ${msg.note.agent_label}`);
      return; // notifies are a bonus, not a gate
    case "error":
      console.error(`ERROR from bridge: ${msg.code} — ${msg.message}`);
      process.exit(1);
  }
  want.delete(msg.type);
  if (want.size === 0) {
    clearTimeout(timeout);
    console.log("SMOKE OK");
    process.exit(0);
  }
};

ws.onerror = () => {
  console.error(`could not connect to ${url} — is \`shepsky-bridge serve\` running?`);
  process.exit(1);
};
