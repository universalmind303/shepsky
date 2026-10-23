#!/usr/bin/env node
// A pretend coding agent for the shepsky demo. Run INSIDE a herdr pane: it
// reports its own state to herdr (like a real integration), prints a fake
// work log, periodically asks for permission, and reacts to what you send
// from the phone:
//
//   blocked  -> 1 / 2 / Enter approve, 3 / Esc deny
//   idle     -> type a prompt + Enter: it "works on" it
//   working  -> prompts are queued for next
//
//   node scripts/fake-agent.mjs --kind claude --task "Fix flaky auth tests" --flavor test
//   node scripts/fake-agent.mjs --kind codex --idle   # wait for a prompt first
//   node scripts/fake-agent.mjs --history 200         # start with an earlier log
//
// With --transcripts <dir> it also keeps a transcript in Claude Code's
// format at <dir>/<pane>.jsonl, which the demo bridge serves to the phone's
// chat view.
//
// No network, no model, no tokens. Ctrl-C to quit.

import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import net from "node:net";
import { join } from "node:path";

// --- args ------------------------------------------------------------------
const args = process.argv.slice(2);
const arg = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const KIND = arg("kind", "claude");
const TASK = arg("task", "Refactor the thing");
const FLAVOR = arg("flavor", "test");
// Lines of an earlier session's log to start with, so there's scrollback.
const HISTORY = Number(arg("history", "0"));
const SOCKET = process.env.HERDR_SOCKET_PATH;
const PANE = process.env.HERDR_PANE_ID;
const TRANSCRIPTS = arg("transcripts", null);

// --- terminal helpers --------------------------------------------------------
const c = {
  dim: (s) => `\x1b[2m${s}\x1b[0m`,
  bold: (s) => `\x1b[1m${s}\x1b[0m`,
  green: (s) => `\x1b[32m${s}\x1b[0m`,
  yellow: (s) => `\x1b[33m${s}\x1b[0m`,
  red: (s) => `\x1b[31m${s}\x1b[0m`,
  cyan: (s) => `\x1b[36m${s}\x1b[0m`,
  magenta: (s) => `\x1b[35m${s}\x1b[0m`,
};
const out = (s = "") => process.stdout.write(`${s}\n`);
const stamp = () => c.dim(new Date().toTimeString().slice(0, 8));
const setTitle = (t) => process.stdout.write(`\x1b]0;${t}\x07`);

// --- herdr reporting ---------------------------------------------------------
// herdr ignores a report unless its seq beats the last one for this pane, and
// a relaunched agent shares the pane with its predecessor: count from the
// clock, not from zero.
let seq = Date.now();
function report(state, message) {
  if (!SOCKET || !PANE) return;
  const conn = net.createConnection(SOCKET);
  conn.on("error", () => {}); // demo: never crash on reporting
  conn.on("connect", () => {
    seq += 1;
    conn.write(
      JSON.stringify({
        id: `fake:${PANE}:${seq}`,
        method: "pane.report_agent",
        params: {
          pane_id: PANE,
          source: "shepsky-demo",
          agent: KIND,
          state,
          seq,
          ...(message ? { message } : {}),
        },
      }) + "\n",
    );
  });
  conn.on("data", () => conn.end());
  conn.setTimeout(3000, () => conn.destroy());
}

// --- transcript (Claude Code's JSONL shape, enough for the chat view) -------
const TRANSCRIPT = TRANSCRIPTS && PANE ? join(TRANSCRIPTS, `${PANE.replace(":", "-")}.jsonl`) : null;
if (TRANSCRIPT) {
  mkdirSync(TRANSCRIPTS, { recursive: true });
  writeFileSync(TRANSCRIPT, ""); // a fresh agent, a fresh conversation
}
let toolSeq = 0;
function record(type, content, extra = {}) {
  if (!TRANSCRIPT) return;
  const role = type === "assistant" ? "assistant" : "user";
  const line = { type, timestamp: new Date().toISOString(), message: { role, content }, ...extra };
  try {
    appendFileSync(TRANSCRIPT, JSON.stringify(line) + "\n");
  } catch {} // demo: never crash on the transcript
}
const say = (text) => record("assistant", [{ type: "text", text }]);
const think = () => record("assistant", [{ type: "thinking", thinking: "", signature: "demo" }]);
function toolUse(name, input) {
  const id = `toolu_demo_${++toolSeq}`;
  record("assistant", [{ type: "tool_use", id, name, input }]);
  return id;
}
const toolResult = (id, text, isError = false) =>
  record("user", [{ type: "tool_result", tool_use_id: id, content: text, is_error: isError }]);
const promptRecord = (text) => record("user", text);

/** A permission like "run `cargo test -p auth`" as the tool call it asks for. */
function permissionCall(action) {
  const command = action.match(/`([^`]+)`/);
  if (command) return toolUse("Bash", { command: command[1] });
  return toolUse("Edit", { file_path: action.replace(/^edit /, ""), old_string: "…", new_string: "…" });
}

// --- fake work ---------------------------------------------------------------
const WORK = {
  test: [
    "running test suite",
    "✓ auth refresh under clock skew",
    "✓ session cookie rotation",
    "✗ token revocation race — investigating",
    "patching revocation lock ordering",
    "✓ token revocation race",
  ],
  build: [
    "compiling ingest pipeline",
    "warning: rate limiter window unset",
    "adding token-bucket middleware",
    "wiring per-tenant limits",
    "✓ ingest builds clean",
  ],
  lint: [
    "scanning settings page components",
    "migrating 6 components to new tokens",
    "reflowing responsive layout",
    "✓ visual snapshot diff: 0 regressions",
  ],
};
const PERMISSIONS = {
  test: "run `cargo test -p auth`",
  build: "edit src/middleware/rate_limit.rs",
  lint: "run `npm run lint -- --fix`",
};

function colorLine(line) {
  if (line.startsWith("✓")) return c.green(line);
  if (line.startsWith("✗")) return c.red(line);
  if (line.startsWith("warning")) return c.yellow(line);
  return line;
}

// --- state machine -----------------------------------------------------------
let state = "idle";
let workTimer = null;
let queued = [];

function startWork(what) {
  state = "working";
  report("working");
  setTitle(what);
  out();
  out(`${stamp()} ${c.cyan("●")} ${c.bold(what)}`);
  think();
  say(`On it: ${what.charAt(0).toLowerCase()}${what.slice(1)}.`);
  const steps = WORK[FLAVOR] ?? WORK.test;
  let i = 0;
  // Ask permission halfway through every task, like a real agent would.
  const askAt = Math.floor(steps.length / 2);
  const tick = () => {
    if (i === askAt) {
      askPermission();
      return;
    }
    if (i >= steps.length) {
      finish(what);
      return;
    }
    out(`${stamp()}   ${colorLine(steps[i])}`);
    if (/^[✓✗]/.test(steps[i])) say(steps[i]);
    else toolResult(toolUse("Bash", { command: steps[i] }), "ok");
    i += 1;
    workTimer = setTimeout(tick, 2500 + Math.random() * 2500);
  };
  workTimer = setTimeout(tick, 1500);

  resumeWork = () => {
    i += 1; // skip past the ask
    workTimer = setTimeout(tick, 1200);
  };
}
let resumeWork = () => {};

let pendingCall = null;
function askPermission() {
  state = "blocked";
  const action = PERMISSIONS[FLAVOR] ?? PERMISSIONS.test;
  pendingCall = permissionCall(action);
  report("blocked", `Allow ${KIND} to ${action}?`);
  // Full pane width, like real agents' permission boxes (so the phone gets
  // realistically wide frames).
  const inner = Math.max(30, (process.stdout.columns || 80) - 4);
  const row = (text = "", styled = text) =>
    `${c.yellow("│")} ${styled}${" ".repeat(Math.max(0, inner - text.length))} ${c.yellow("│")}`;
  out();
  out(c.yellow(`╭${"─".repeat(inner + 2)}╮`));
  out(row(`${KIND} wants to ${action}`, c.bold(`${KIND} wants to ${action}`)));
  out(row());
  out(row("1. Yes", `${c.bold("1.")} Yes`));
  out(row("2. Yes, and don't ask again for this session", `${c.bold("2.")} Yes, and don't ask again for this session`));
  const no = `3. No, tell ${KIND} what to do differently (esc)`;
  out(row(no, `${c.bold("3.")} No, tell ${KIND} what to do differently ${c.dim("(esc)")}`));
  out(c.yellow(`╰${"─".repeat(inner + 2)}╯`));
  out(c.dim("  waiting for you…"));
}

function answerPermission(choice) {
  if (state !== "blocked") return;
  if (choice === "deny") {
    out(`${stamp()} ${c.red("✗ denied")} — okay, I'll skip that step`);
    if (pendingCall) toolResult(pendingCall, "The user doesn't want to proceed with this tool use.", true);
  } else {
    out(`${stamp()} ${c.green(choice === "always" ? "✓ approved (always)" : "✓ approved")}`);
    if (pendingCall) toolResult(pendingCall, "ok");
  }
  pendingCall = null;
  state = "working";
  report("working");
  resumeWork();
}

function finish(what) {
  state = "idle";
  report("idle");
  setTitle(`${what} ✓`);
  out(`${stamp()} ${c.green("●")} ${c.bold("done")} — ${what}`);
  say(`Done: ${what.charAt(0).toLowerCase()}${what.slice(1)}. Anything else?`);
  if (queued.length > 0) {
    const next = queued.shift();
    setTimeout(() => startWork(next), 1500);
    return;
  }
  out(c.dim("  (send me a prompt from your phone to keep going)"));
}

function onPrompt(text) {
  if (!text) return;
  out(`${stamp()} ${c.magenta("›")} ${text}`);
  promptRecord(text);
  if (state === "idle") {
    setTimeout(() => startWork(text), 800);
  } else if (state === "blocked") {
    // A prompt while blocked = "No, do this instead".
    out(`${stamp()} ${c.yellow("↪")} skipping that step; will do this next`);
    queued.unshift(text);
    answerPermission("deny");
  } else {
    queued.push(text);
    out(`${stamp()} ${c.dim(`queued (${queued.length} waiting)`)}`);
  }
}

// --- input -------------------------------------------------------------------
// Raw mode so single keys (1/2/3/esc) work like a real agent's prompt, while
// typed or pasted text + Enter still arrives as a prompt.
let line = "";
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  if (chunk === "\x03") quit(); // ctrl-c
  if (chunk === "\x1b") {
    // A lone ESC (arrow keys etc. arrive as longer sequences).
    if (state === "blocked") answerPermission("deny");
    line = "";
    return;
  }
  // Drop bracketed-paste markers and other CSI sequences.
  const text = chunk.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "");
  for (const ch of text) {
    if (state === "blocked" && line === "" && "123".includes(ch)) {
      answerPermission({ 1: "once", 2: "always", 3: "deny" }[ch]);
      continue;
    }
    if (ch === "\r" || ch === "\n") {
      if (state === "blocked" && line === "") {
        answerPermission("once"); // Enter = default option
      } else {
        const prompt = line.trim();
        line = "";
        onPrompt(prompt);
      }
      continue;
    }
    if (ch === "\x7f" || ch === "\b") {
      line = line.slice(0, -1);
      continue;
    }
    if (ch >= " ") line += ch;
  }
});

function quit() {
  clearTimeout(workTimer);
  report("idle");
  out();
  out(c.dim("fake agent exiting"));
  if (process.stdin.isTTY) process.stdin.setRawMode(false);
  process.exit(0);
}
process.on("SIGTERM", quit);

// --- go ----------------------------------------------------------------------
process.stdout.write("\x1b[2J\x1b[H");
out(c.bold(`${KIND} (fake)`) + c.dim(" — shepsky demo agent · ctrl-c to quit"));
// An earlier session's log, numbered so scrolling back is easy to follow.
if (HISTORY > 0) {
  const steps = Object.values(WORK).flat();
  const start = Date.now() - HISTORY * 20_000;
  for (let i = 0; i < HISTORY; i++) {
    const time = new Date(start + i * 20_000).toTimeString().slice(0, 8);
    const step = steps[i % steps.length];
    out(`${c.dim(time)}   ${c.dim(`#${String(i + 1).padStart(3, "0")}`)} ${colorLine(step)}`);
  }
  out(c.dim(`  (${HISTORY} lines of earlier work above)`));
}
if (args.includes("--idle")) {
  // Freshly started, like an agent the phone just launched.
  report("idle");
  setTitle("ready");
} else {
  // Stagger agents so their statuses don't all move in lockstep.
  promptRecord(TASK);
  setTimeout(() => startWork(TASK), 1000 + Math.random() * 6000);
}
