#!/usr/bin/env node
// Drive the sandboxed demo herdr session: create fake workspaces/panes,
// report fake agents, and (with `scenario`) keep their statuses moving so the
// phone app has something to show. Used by scripts/demo-session.sh.
//
//   node scripts/demo.mjs setup      # create demo workspaces + fake agents
//   node scripts/demo.mjs scenario   # loop: working -> blocked -> done -> ...
//
// Env: HERDR_SOCKET_PATH must point at the DEMO session's socket.

import net from "node:net";

const socketPath = process.env.HERDR_SOCKET_PATH;
if (!socketPath) {
  console.error("HERDR_SOCKET_PATH must point at the demo session socket");
  process.exit(1);
}

let nextId = 1;
function rpc(method, params) {
  return new Promise((resolve, reject) => {
    const conn = net.createConnection(socketPath);
    let buf = "";
    conn.setTimeout(10_000, () => reject(new Error(`${method}: timeout`)));
    conn.on("error", reject);
    conn.on("data", (chunk) => {
      buf += chunk;
      const nl = buf.indexOf("\n");
      if (nl < 0) return;
      conn.end();
      const response = JSON.parse(buf.slice(0, nl));
      if (response.error) reject(new Error(`${method}: ${response.error.code} ${response.error.message}`));
      else resolve(response.result);
    });
    conn.on("connect", () => {
      conn.write(JSON.stringify({ id: `demo:${nextId++}`, method, params }) + "\n");
    });
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// One fake agent per pane: [workspace label, agent kind, title, output flavor]
const CAST = [
  ["api-service", "claude", "Fix flaky auth tests", "test"],
  ["api-service", "codex", "Add rate limiting to /v1/ingest", "build"],
  ["web-app", "claude", "Migrate settings page to new design", "lint"],
];

async function panesByWorkspace() {
  const { snapshot } = await rpc("session.snapshot", {});
  const labels = Object.fromEntries(
    snapshot.workspaces.map((w) => [w.workspace_id, w.label]),
  );
  return snapshot.panes.map((p) => ({
    pane_id: p.pane_id,
    workspace: labels[p.workspace_id],
  }));
}

async function setup() {
  const wanted = [...new Set(CAST.map(([ws]) => ws))];
  const { snapshot } = await rpc("session.snapshot", {});
  const existing = new Set(snapshot.workspaces.map((w) => w.label));
  for (const label of wanted) {
    if (!existing.has(label)) {
      await rpc("workspace.create", { label, cwd: process.env.HOME });
    }
  }
  // Each workspace starts with one pane; add more until each workspace has as
  // many panes as cast members.
  for (const label of wanted) {
    const needed = CAST.filter(([ws]) => ws === label).length;
    let have = (await panesByWorkspace()).filter((p) => p.workspace === label);
    while (have.length < needed) {
      await rpc("pane.split", {
        direction: "right",
        target_pane_id: have[0].pane_id,
      });
      have = (await panesByWorkspace()).filter((p) => p.workspace === label);
    }
  }

  // Match cast to panes, set a title, start fake "work output", report agent.
  const assignments = await assignCast();
  for (const { pane_id, agent, title, flavor } of assignments) {
    const script = fakeOutput(title, flavor);
    await rpc("pane.send_text", { pane_id, text: `${script}\n` });
    await rpc("pane.report_agent", {
      pane_id,
      source: "collie-demo",
      agent,
      state: "working",
    });
  }
  console.log("demo session ready:");
  for (const a of assignments) {
    console.log(`  ${a.pane_id}  ${a.agent}  ${a.title}`);
  }
}

async function assignCast() {
  const panes = await panesByWorkspace();
  const byWs = new Map();
  for (const p of panes) {
    if (!byWs.has(p.workspace)) byWs.set(p.workspace, []);
    byWs.get(p.workspace).push(p);
  }
  return CAST.map(([ws, agent, title, flavor]) => {
    const pane = byWs.get(ws)?.shift();
    if (!pane) throw new Error(`no pane left in ${ws}; run setup first`);
    return { pane_id: pane.pane_id, agent, title, flavor };
  });
}

// A shell loop that looks like an agent working (sets the pane title too).
function fakeOutput(title, flavor) {
  const lines = {
    test: [
      "running test suite (attempt $i)…",
      "  ✓ auth refresh under clock skew",
      "  ✗ token revocation race — retrying",
    ],
    build: [
      "compiling ingest pipeline (pass $i)…",
      "  warning: rate limiter window unset",
      "  wiring middleware + backoff",
    ],
    lint: [
      "checking settings page (round $i)…",
      "  updated 3 components to new tokens",
      "  reflowing responsive layout",
    ],
  }[flavor];
  const echoes = lines.map((l) => `echo "${l}"`).join("; sleep 1; ");
  return (
    `clear; printf '\\033]0;${title}\\007'; i=1; ` +
    `while true; do ${echoes}; i=$((i+1)); sleep 2; done`
  );
}

// working -> blocked -> idle(=done while unseen) -> working…, staggered.
async function scenario() {
  const assignments = await assignCast();
  const phases = ["working", "blocked", "idle", "working"];
  const messages = {
    working: null,
    blocked: "Allow running `cargo test`?",
    idle: null,
  };
  let step = 0;
  console.log("scenario running (ctrl-c to stop)…");
  for (;;) {
    for (let i = 0; i < assignments.length; i++) {
      const { pane_id, agent } = assignments[i];
      const state = phases[(step + i) % phases.length];
      await rpc("pane.report_agent", {
        pane_id,
        source: "collie-demo",
        agent,
        state,
        message: messages[state],
      });
      console.log(`  ${pane_id} -> ${state}`);
    }
    step += 1;
    await sleep(20_000);
  }
}

const mode = process.argv[2] ?? "setup";
try {
  if (mode === "setup") await setup();
  else if (mode === "scenario") await scenario();
  else {
    console.error("usage: demo.mjs [setup|scenario]");
    process.exit(2);
  }
} catch (err) {
  console.error(String(err));
  process.exit(1);
}
