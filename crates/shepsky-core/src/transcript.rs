//! Agents' own transcripts, read into chat items.
//!
//! Each agent keeps its conversation as JSON lines: Claude Code in
//! `~/.claude/projects/<dir>/<session>.jsonl`, Codex in
//! `~/.codex/sessions/<date>/rollout-…-<session>.jsonl`, pi in
//! `~/.pi/agent/sessions/<dir>/….jsonl`. Every line parses on its own
//! ([`parse_line`]) into zero or more [`Entry`]s, so any slice of a
//! transcript can be read in isolation; a [`Window`] stitches tool results
//! onto their calls, whichever arrives first.
//!
//! These are the agents' private formats, not APIs: unknown records are
//! skipped, never errors.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::protocol::{ChatBody, ChatItem, QuestionView, ToolStatus};

const MAX_TEXT: usize = 24 * 1024;
const MAX_DETAIL: usize = 4 * 1024;
const MAX_OUTPUT: usize = 4 * 1024;
const MAX_SUMMARY: usize = 200;

/// Items per transcript line are numbered `offset * BLOCKS + index`.
const BLOCKS: u64 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Claude,
    Codex,
    Pi,
}

impl Format {
    /// The transcript format for a herdr agent kind.
    pub fn for_kind(kind: &str) -> Option<Self> {
        match kind {
            "claude" | "claude-code" => Some(Format::Claude),
            "codex" => Some(Format::Codex),
            "pi" => Some(Format::Pi),
            _ => None,
        }
    }
}

/// Where paths are shown from: relative to the agent's directory, else
/// with `~` for home.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ctx<'a> {
    pub cwd: Option<&'a str>,
    pub home: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A new item; `call` links a tool call (or question) to its result.
    Item { item: ChatItem, call: Option<String> },
    /// A tool call finished.
    Done { call: String, output: Option<String>, error: bool },
}

/// Parse one transcript line that starts at byte `offset`.
pub fn parse_line(format: Format, offset: u64, line: &str, ctx: &Ctx) -> Vec<Entry> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let mut out = Out { base: offset * BLOCKS, next: 0, entries: Vec::new() };
    match format {
        Format::Claude => claude(&v, &mut out, ctx),
        Format::Codex => codex(&v, &mut out, ctx),
        Format::Pi => pi(&v, &mut out, ctx),
    }
    out.entries
}

struct Out {
    base: u64,
    next: u64,
    entries: Vec<Entry>,
}

impl Out {
    fn item(&mut self, body: ChatBody, call: Option<String>) {
        let id = self.base + self.next.min(BLOCKS - 1);
        self.next += 1;
        self.entries.push(Entry::Item { item: ChatItem { id, body }, call });
    }

    fn text(&mut self, kind: fn(String) -> ChatBody, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.item(kind(clip(text, MAX_TEXT)), None);
        }
    }

    fn notice(&mut self, text: &str) {
        self.item(ChatBody::Notice { text: text.to_string() }, None);
    }

    fn done(&mut self, call: &str, output: &str, error: bool) {
        let output = strip_ansi(output.trim());
        self.entries.push(Entry::Done {
            call: call.to_string(),
            output: (!output.is_empty()).then(|| clip_middle(&output, MAX_OUTPUT)),
            error,
        });
    }
}

fn prompt(text: String) -> ChatBody {
    ChatBody::Prompt { text }
}

fn reply(text: String) -> ChatBody {
    ChatBody::Reply { text }
}

fn tool(name: &str, summary: String, detail: Option<String>) -> ChatBody {
    ChatBody::Tool {
        name: name.to_string(),
        summary: clip(&summary, MAX_SUMMARY),
        detail: detail.filter(|d| !d.trim().is_empty()).map(|d| clip(&d, MAX_DETAIL)),
        output: None,
        status: ToolStatus::Running,
    }
}

// --- Claude Code -----------------------------------------------------------

fn claude(v: &Value, out: &mut Out, ctx: &Ctx) {
    if v["isSidechain"] == true {
        return; // a subagent's own conversation; its Task call stands for it
    }
    let content = &v["message"]["content"];
    match v["type"].as_str() {
        Some("user") => {
            if v["isMeta"] == true {
                return;
            }
            if v["isCompactSummary"] == true {
                return; // the compact_boundary record already said so
            }
            match content {
                Value::String(text) => claude_user_text(text, out),
                Value::Array(blocks) => {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => claude_user_text(str_of(&b["text"]), out),
                            Some("image") => out.text(prompt, "[image]"),
                            Some("tool_result") => {
                                let output = claude_result_text(&b["content"]);
                                out.done(str_of(&b["tool_use_id"]), &output, b["is_error"] == true);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        Some("assistant") => {
            for b in content.as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("thinking") | Some("redacted_thinking") => out.item(
                        ChatBody::Thinking { text: clip(str_of(&b["thinking"]).trim(), MAX_TEXT) },
                        None,
                    ),
                    Some("text") if v["isApiErrorMessage"] == true => out.notice(str_of(&b["text"]).trim()),
                    Some("text") => out.text(reply, str_of(&b["text"])),
                    Some("tool_use") => {
                        let call = Some(str_of(&b["id"]).to_string());
                        let name = str_of(&b["name"]);
                        if name == "AskUserQuestion" {
                            out.item(claude_question(&b["input"]), call);
                        } else {
                            let (summary, detail) = claude_tool(name, &b["input"], ctx);
                            out.item(tool(&tool_name(name), summary, detail), call);
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("system") if v["subtype"] == "compact_boundary" => out.notice("context compacted"),
        // Typed while the agent was busy; delivered with its next step.
        Some("attachment") if v["attachment"]["type"] == "queued_command" && v["attachment"]["isMeta"] != true => {
            out.text(prompt, str_of(&v["attachment"]["prompt"]))
        }
        _ => {}
    }
}

fn claude_user_text(text: &str, out: &mut Out) {
    // Reminders the harness appends to a prompt aren't what you typed.
    let text = text.split("\n<system-reminder>").next().unwrap_or("").trim();
    if let Some(rest) = text.strip_prefix("<command-name>") {
        let command = rest.split("</command-name>").next().unwrap_or("").trim();
        let args = between(text, "<command-args>", "</command-args>").unwrap_or("").trim();
        out.notice(format!("{command} {args}").trim_end());
    } else if text.starts_with("[Request interrupted by user") {
        out.notice("interrupted");
    } else if let Some(command) = between(text, "<bash-input>", "</bash-input>") {
        out.text(prompt, &format!("! {}", command.trim()));
    } else if !text.starts_with('<') {
        out.text(prompt, text);
    }
}

fn claude_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|p| match p["type"].as_str() {
                Some("text") => str_of(&p["text"]).to_string(),
                Some("image") => "[image]".to_string(),
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn claude_question(input: &Value) -> ChatBody {
    let questions = input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| QuestionView {
            header: q["header"].as_str().map(str::to_string),
            question: str_of(&q["question"]).to_string(),
            options: q["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| str_of(&o["label"]).to_string())
                .collect(),
            multi: q["multiSelect"] == true,
        })
        .collect();
    ChatBody::Question { questions, answer: None }
}

fn claude_tool(name: &str, input: &Value, ctx: &Ctx) -> (String, Option<String>) {
    let s = |key: &str| str_of(&input[key]);
    match name {
        "Bash" => shell(s("command")),
        "Read" | "Write" | "NotebookEdit" => {
            (show_path(if name == "NotebookEdit" { s("notebook_path") } else { s("file_path") }, ctx), None)
        }
        "Edit" => (show_path(s("file_path"), ctx), Some(mini_diff(s("old_string"), s("new_string")))),
        "MultiEdit" => {
            let edits = input["edits"].as_array().map_or(0, Vec::len);
            (format!("{} · {edits} edits", show_path(s("file_path"), ctx)), None)
        }
        "Grep" => {
            let place = input["path"].as_str().map(|p| format!(" in {}", show_path(p, ctx))).unwrap_or_default();
            (format!("{}{place}", s("pattern")), None)
        }
        "Glob" => (s("pattern").to_string(), None),
        "WebFetch" => (s("url").to_string(), None),
        "WebSearch" => (s("query").to_string(), None),
        "Task" | "Agent" => (s("description").to_string(), Some(s("prompt").to_string())),
        "TodoWrite" => {
            let todos: Vec<String> = input["todos"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| {
                    let mark = match t["status"].as_str() {
                        Some("completed") => "[x]",
                        Some("in_progress") => "[~]",
                        _ => "[ ]",
                    };
                    format!("{mark} {}", str_of(&t["content"]))
                })
                .collect();
            (format!("{} todos", todos.len()), Some(todos.join("\n")))
        }
        _ => generic_args(input),
    }
}

// --- Codex -----------------------------------------------------------------

fn codex(v: &Value, out: &mut Out, ctx: &Ctx) {
    let payload = &v["payload"];
    if v["type"] != "event_msg" {
        return;
    }
    match payload["type"].as_str() {
        Some("item_completed") => codex_item(&payload["item"], out, ctx),
        Some("turn_aborted") => out.notice("interrupted"),
        _ => {}
    }
}

fn codex_item(it: &Value, out: &mut Out, ctx: &Ctx) {
    let texts = |key: &str| -> String {
        it[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().or_else(|| c["text"].as_str()))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    match it["type"].as_str() {
        Some("UserMessage") => out.text(prompt, &texts("content")),
        Some("AgentMessage") => out.text(reply, &texts("content")),
        Some("Reasoning") => {
            out.item(ChatBody::Thinking { text: clip(texts("summary_text").trim(), MAX_TEXT) }, None)
        }
        Some("CommandExecution") => {
            let words: Vec<&str> = it["command"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            // `zsh -lc "cargo test"` is just `cargo test`.
            let command = match words.as_slice() {
                [_, flag, script] if matches!(*flag, "-lc" | "-c") => script.to_string(),
                _ => words.join(" "),
            };
            let (summary, detail) = shell(&command);
            let failed = it["status"] != "completed" || it["exit_code"].as_i64().is_some_and(|c| c != 0);
            finished(out, tool("shell", summary, detail), str_of(&it["aggregated_output"]), failed);
        }
        Some("FileChange") => {
            let changes = it["changes"].as_object();
            let paths: Vec<String> = changes.into_iter().flatten().map(|(p, _)| show_path(p, ctx)).collect();
            let summary = match paths.as_slice() {
                [one] => one.clone(),
                many => format!("{} files", many.len()),
            };
            let detail = changes
                .into_iter()
                .flatten()
                .map(|(path, change)| {
                    let body = change["unified_diff"].as_str().or(change["content"].as_str()).unwrap_or("");
                    format!("{} ({})\n{}", show_path(path, ctx), str_of(&change["type"]), body.trim_end())
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let failed = it["status"].as_str().is_some_and(|s| s != "completed");
            finished(out, tool("edit", summary, Some(detail)), str_of(&it["stderr"]), failed);
        }
        Some("McpToolCall") => {
            let name = format!("{}.{}", str_of(&it["server"]), str_of(&it["tool"]));
            let (summary, detail) = generic_args(&it["arguments"]);
            let result = &it["result"];
            let output = match result["content"].as_array() {
                Some(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
                None if result.is_null() => String::new(),
                None => result.to_string(),
            };
            let failed = it["status"] == "failed" || result["isError"] == true;
            finished(out, tool(&name, summary, detail), &output, failed);
        }
        Some("ImageView") => finished(out, tool("view_image", show_path(str_of(&it["path"]), ctx), None), "", false),
        Some("ContextCompaction") => out.notice("context compacted"),
        _ => {}
    }
}

/// A tool item that arrives already finished (Codex logs items on completion).
fn finished(out: &mut Out, mut body: ChatBody, output: &str, failed: bool) {
    if let ChatBody::Tool { output: o, status, .. } = &mut body {
        let text = strip_ansi(output.trim());
        *o = (!text.is_empty()).then(|| clip_middle(&text, MAX_OUTPUT));
        *status = if failed { ToolStatus::Error } else { ToolStatus::Ok };
    }
    out.item(body, None);
}

// --- pi --------------------------------------------------------------------

fn pi(v: &Value, out: &mut Out, ctx: &Ctx) {
    if v["type"] != "message" {
        return;
    }
    let m = &v["message"];
    let blocks = m["content"].as_array().into_iter().flatten();
    match m["role"].as_str() {
        Some("user") => {
            let text: Vec<&str> = blocks.filter(|b| b["type"] == "text").map(|b| str_of(&b["text"])).collect();
            out.text(prompt, &text.join("\n"));
        }
        Some("assistant") => {
            for b in blocks {
                match b["type"].as_str() {
                    Some("thinking") => {
                        out.item(ChatBody::Thinking { text: clip(str_of(&b["thinking"]).trim(), MAX_TEXT) }, None)
                    }
                    Some("text") => out.text(reply, str_of(&b["text"])),
                    Some("toolCall") => {
                        let name = str_of(&b["name"]);
                        let args = &b["arguments"];
                        let (summary, detail) = match name {
                            "bash" => shell(str_of(&args["command"])),
                            "read" | "write" | "edit" | "ls" => {
                                let path = args["path"].as_str().or(args["file_path"].as_str()).unwrap_or("");
                                (show_path(path, ctx), None)
                            }
                            _ => generic_args(args),
                        };
                        out.item(tool(name, summary, detail), Some(str_of(&b["id"]).to_string()));
                    }
                    _ => {}
                }
            }
        }
        Some("toolResult") => {
            let text: Vec<&str> = blocks.filter(|b| b["type"] == "text").map(|b| str_of(&b["text"])).collect();
            out.done(str_of(&m["toolCallId"]), &text.join("\n"), m["isError"] == true);
        }
        _ => {}
    }
}

// --- stitching -------------------------------------------------------------

/// The part of a transcript a phone has loaded. Feed it entries in file
/// order, newer or older batches alike; it returns what to send.
#[derive(Debug, Default)]
pub struct Window {
    items: BTreeMap<u64, ChatItem>,
    /// Call id -> the item it belongs to.
    calls: HashMap<String, u64>,
    /// Results read before their call (paging backwards).
    early: HashMap<String, (Option<String>, bool)>,
    retained_bytes: usize,
}

const MAX_RETAINED_ITEMS: usize = 512;
const MAX_RETAINED_BYTES: usize = 2 * 1024 * 1024;

impl Window {
    /// Take parsed entries; returns new items and items that changed.
    pub fn add(&mut self, entries: Vec<Entry>) -> Vec<ChatItem> {
        let mut changed = BTreeMap::new();
        for entry in entries {
            match entry {
                Entry::Item { mut item, call } => {
                    if let Some(call) = call {
                        if let Some((output, error)) = self.early.remove(&call) {
                            finish(&mut item, output, error);
                        }
                        if call.len() <= 256 {
                            if self.calls.len() >= MAX_RETAINED_ITEMS {
                                self.calls.clear();
                            }
                            self.calls.insert(call, item.id);
                        }
                    }
                    changed.insert(item.id, item.clone());
                    self.retained_bytes += item_bytes(&item);
                    if let Some(previous) = self.items.insert(item.id, item) {
                        self.retained_bytes -= item_bytes(&previous);
                    }
                }
                Entry::Done { call, output, error } => {
                    match self.calls.get(&call).and_then(|id| self.items.get_mut(id)) {
                        Some(item) => {
                            self.retained_bytes -= item_bytes(item);
                            finish(item, output, error);
                            self.retained_bytes += item_bytes(item);
                            changed.insert(item.id, item.clone());
                        }
                        None => {
                            if call.len() <= 256 && output.as_ref().is_none_or(|s| s.len() <= 16 * 1024) {
                                if self.early.len() >= 128 {
                                    self.early.clear();
                                }
                                self.early.insert(call, (output, error));
                            }
                        }
                    }
                }
            }
            while self.items.len() > MAX_RETAINED_ITEMS || self.retained_bytes > MAX_RETAINED_BYTES {
                let Some((_, removed)) = self.items.pop_first() else { break };
                self.retained_bytes -= item_bytes(&removed);
            }
            self.calls.retain(|_, id| self.items.contains_key(id));
        }
        changed.into_values().collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn item_bytes(item: &ChatItem) -> usize {
    let bytes = match &item.body {
        ChatBody::Prompt { text } | ChatBody::Reply { text } | ChatBody::Thinking { text } | ChatBody::Notice { text } => text.len(),
        ChatBody::Tool { name, summary, detail, output, .. } => {
            name.len() + summary.len() + detail.as_ref().map_or(0, String::len) + output.as_ref().map_or(0, String::len)
        }
        ChatBody::Question { questions, answer } => {
            answer.as_ref().map_or(0, String::len) + questions.iter().map(|q| {
                std::mem::size_of_val(q) + q.header.as_ref().map_or(0, String::len) + q.question.len()
                    + q.options.iter().map(|s| std::mem::size_of::<String>() + s.len()).sum::<usize>()
            }).sum::<usize>()
        }
    };
    std::mem::size_of_val(item) + bytes
}

fn finish(item: &mut ChatItem, result: Option<String>, error: bool) {
    match &mut item.body {
        ChatBody::Tool { output, status, .. } => {
            *output = result;
            *status = if error { ToolStatus::Error } else { ToolStatus::Ok };
        }
        ChatBody::Question { answer, .. } => *answer = result,
        _ => {}
    }
}

// --- helpers ---------------------------------------------------------------

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// `$ first line`, with the whole script as detail when there's more.
fn shell(command: &str) -> (String, Option<String>) {
    let command = command.trim();
    let first = command.lines().next().unwrap_or("");
    let more = command.len() > first.len() || first.chars().count() > 120;
    (format!("$ {first}"), more.then(|| command.to_string()))
}

/// The first string argument as the summary, all of them as detail.
fn generic_args(args: &Value) -> (String, Option<String>) {
    let Some(map) = args.as_object() else {
        return (String::new(), None);
    };
    let summary = match map.values().find_map(Value::as_str) {
        Some(text) => text.lines().next().unwrap_or("").to_string(),
        None => map.iter().next().map(|(k, v)| format!("{k}: {v}")).unwrap_or_default(),
    };
    let detail = (map.len() > 1 || summary.is_empty()).then(|| {
        map.iter()
            .map(|(k, v)| format!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))
            .collect::<Vec<_>>()
            .join("\n")
    });
    (summary, detail)
}

/// `mcp__github__get_pr` -> `github.get_pr`.
fn tool_name(name: &str) -> String {
    match name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")) {
        Some((server, tool)) => format!("{server}.{tool}"),
        None => name.to_string(),
    }
}

fn mini_diff(old: &str, new: &str) -> String {
    let minus = old.lines().map(|l| format!("- {l}"));
    let plus = new.lines().map(|l| format!("+ {l}"));
    minus.chain(plus).collect::<Vec<_>>().join("\n")
}

fn show_path(path: &str, ctx: &Ctx) -> String {
    if let Some(rest) = ctx.cwd.and_then(|cwd| path.strip_prefix(cwd)).and_then(|r| r.strip_prefix('/')) {
        return rest.to_string();
    }
    match ctx.home.filter(|h| !h.is_empty() && *h != "/") {
        Some(home) if path == home => "~".to_string(),
        Some(home) => match path.strip_prefix(home).filter(|r| r.starts_with('/')) {
            Some(rest) => format!("~{rest}"),
            None => path.to_string(),
        },
        None => path.to_string(),
    }
}

/// At most `max` bytes, cut on a character boundary.
fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// Long output keeps its start and its end: that's where errors are.
fn clip_middle(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut head = max * 2 / 3;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - (max - head);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}\n…\n{}", &text[..head], &text[tail..])
}

/// Drop terminal escape sequences from command output.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte in @..~
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ESC \
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let ctx = Ctx { cwd: Some("/Users/me/app"), home: Some("/Users/me") };
        assert_eq!(show_path("/Users/me/app/src/main.rs", &ctx), "src/main.rs");
        assert_eq!(show_path("/Users/me/.zshrc", &ctx), "~/.zshrc");
        assert_eq!(show_path("/Users/meow", &ctx), "/Users/meow");
        assert_eq!(strip_ansi("\x1b[1;31merror\x1b[0m: no \x1b]0;title\x07x"), "error: no x");
        assert_eq!(shell("cargo test"), ("$ cargo test".to_string(), None));
        assert_eq!(shell("cd x\nmake").1.as_deref(), Some("cd x\nmake"));
        assert_eq!(tool_name("mcp__github__get_pr"), "github.get_pr");
        let long = "é".repeat(5000);
        assert!(clip(&long, 101).len() <= 104);
        let cut = clip_middle(&format!("{}{}", "a".repeat(9000), "END"), 300);
        assert!(cut.starts_with("aaa") && cut.ends_with("END") && cut.contains('…'));
    }

    #[test]
    fn results_attach_to_calls_in_either_order() {
        let call = |id: u64| Entry::Item {
            item: ChatItem { id, body: tool("Bash", "$ ls".into(), None) },
            call: Some(format!("c{id}")),
        };
        let done = |id: u64| Entry::Done { call: format!("c{id}"), output: Some("ok".into()), error: false };

        let mut w = Window::default();
        // Newer page first: the result for call 1 arrives before call 1.
        let sent = w.add(vec![call(2), done(1), done(2)]);
        assert_eq!(sent.len(), 1);
        assert!(matches!(sent[0].body, ChatBody::Tool { status: ToolStatus::Ok, .. }));
        // Then the older page with call 1: it arrives finished.
        let older = w.add(vec![call(1)]);
        assert!(matches!(&older[0].body, ChatBody::Tool { status: ToolStatus::Ok, output: Some(o), .. } if o == "ok"));
        // A call still running, then its result in a later batch: resent.
        w.add(vec![call(3)]);
        let update = w.add(vec![done(3)]);
        assert_eq!(update[0].id, 3);
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn retention_is_bounded_across_pages_and_unmatched_results() {
        let mut window = Window::default();
        for page in 0..20 {
            let entries = (0..100).map(|n| {
                let id = page * 100 + n;
                Entry::Item { item: ChatItem { id, body: tool("Bash", "x".repeat(8192), None) }, call: Some(format!("call-{id}")) }
            }).collect();
            assert_eq!(window.add(entries).len(), 100);
            assert!(window.items.len() <= MAX_RETAINED_ITEMS);
            assert!(window.retained_bytes <= MAX_RETAINED_BYTES);
            assert!(window.calls.len() <= MAX_RETAINED_ITEMS);
        }
        for n in 0..1000 {
            window.add(vec![Entry::Done { call: format!("missing-{n}"), output: Some("x".repeat(8192)), error: false }]);
            assert!(window.early.len() <= 128);
        }
        window.add(vec![Entry::Item { item: ChatItem { id: 9999, body: ChatBody::Reply { text: "x".repeat(MAX_RETAINED_BYTES + 1) } }, call: None }]);
        assert!(window.retained_bytes <= MAX_RETAINED_BYTES);
    }
}
