#!/usr/bin/env node
// Populate the sandboxed demo herdr session with fake agents
// (scripts/fake-agent.mjs). Used by scripts/demo-session.sh; safe to re-run.
//
//   HERDR_SOCKET_PATH=<demo session socket> node scripts/demo.mjs
//
// Each fake agent runs in its own pane, reports its own state to herdr, and
// reacts to prompts / quick replies from the phone. No model, no tokens.

import net from "node:net";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const socketPath = process.env.HERDR_SOCKET_PATH;
if (!socketPath) {
  console.error("HERDR_SOCKET_PATH must point at the demo session socket");
  process.exit(1);
}
const FAKE_AGENT = join(dirname(fileURLToPath(import.meta.url)), "fake-agent.mjs");

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

// [workspace label, agent kind, task, work flavor]
const CAST = [
  ["api-service", "claude", "Fix flaky auth tests", "test"],
  ["api-service", "codex", "Add rate limiting to /v1/ingest", "build"],
  ["web-app", "claude", "Migrate settings page to new design", "lint"],
];

async function panesByWorkspace() {
  const { snapshot } = await rpc("session.snapshot", {});
  const labels = Object.fromEntries(snapshot.workspaces.map((w) => [w.workspace_id, w.label]));
  return snapshot.panes.map((p) => ({ pane_id: p.pane_id, workspace: labels[p.workspace_id] }));
}

async function ensureLayout() {
  const wanted = [...new Set(CAST.map(([ws]) => ws))];
  const { snapshot } = await rpc("session.snapshot", {});
  const existing = new Set(snapshot.workspaces.map((w) => w.label));
  for (const label of wanted) {
    if (!existing.has(label)) await rpc("workspace.create", { label, cwd: process.env.HOME });
  }
  for (const label of wanted) {
    const needed = CAST.filter(([ws]) => ws === label).length;
    let have = (await panesByWorkspace()).filter((p) => p.workspace === label);
    while (have.length < needed) {
      await rpc("pane.split", { direction: "right", target_pane_id: have[0].pane_id });
      have = (await panesByWorkspace()).filter((p) => p.workspace === label);
    }
  }
}

async function assignCast() {
  const byWs = new Map();
  for (const p of await panesByWorkspace()) {
    if (!byWs.has(p.workspace)) byWs.set(p.workspace, []);
    byWs.get(p.workspace).push(p);
  }
  return CAST.map(([ws, kind, task, flavor]) => {
    const pane = byWs.get(ws)?.shift();
    if (!pane) throw new Error(`no free pane in ${ws}`);
    return { pane_id: pane.pane_id, kind, task, flavor };
  });
}

const shq = (s) => `'${String(s).replaceAll("'", "'\\''")}'`;

try {
  await ensureLayout();
  const cast = await assignCast();
  for (const { pane_id, kind, task, flavor } of cast) {
    // Stop whatever ran there before (a previous demo agent, a shell loop).
    await rpc("pane.send_keys", { pane_id, keys: ["ctrl+c"] }).catch(() => {});
    await sleep(300);
    const transcripts = process.env.SHEPSKY_DEMO_TRANSCRIPTS ? ["--transcripts", process.env.SHEPSKY_DEMO_TRANSCRIPTS] : [];
    const cmd = [process.execPath, FAKE_AGENT, "--kind", kind, "--task", task, "--flavor", flavor, ...transcripts]
      .map(shq)
      .join(" ");
    await rpc("pane.send_text", { pane_id, text: `clear; ${cmd}\n` });
  }
  console.log("fake agents launched:");
  for (const a of cast) console.log(`  ${a.pane_id.padEnd(6)} ${a.kind.padEnd(7)} ${a.task}`);
} catch (err) {
  console.error(String(err));
  process.exit(1);
}
