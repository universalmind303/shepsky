//! Chat: an agent's conversation, read from its own transcript.
//!
//! herdr's integrations report which session each agent is in
//! (`agent_session`: an id, or for pi a path). We look that up in the
//! agent's own sessions folder (and only there: the phone names a pane,
//! never a file), send the latest page, follow the file as it grows, and
//! page backwards when asked. See shepsky_core::transcript for the formats.
//!
//! In the demo, fake agents write their transcript to the demo's folder as
//! `<pane>.jsonl` instead (herdr only takes session reports from its
//! official integrations).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use shepsky_core::herdr::Agent;
use shepsky_core::protocol::{ChatItem, ChatMode, ServerMsg};
use shepsky_core::transcript::{Ctx, Entry, Format, Window, parse_line};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::sync::UiState;

const POLL: Duration = Duration::from_millis(400);
/// How often to look again for the transcript of an agent we couldn't
/// place, or whose session may have moved on (Codex starts a new file per
/// conversation).
const RESOLVE_EVERY: Duration = Duration::from_secs(5);
/// Items per page (at least; a page is whole lines).
const PAGE: usize = 60;
const CHUNK: u64 = 128 * 1024;
/// Stop a page at this much reading even if it has few items.
const MAX_PAGE_BYTES: u64 = 8 * 1024 * 1024;
/// A line longer than this can't be paged past.
const MAX_LINE: u64 = 32 * 1024 * 1024;
/// New bytes read per poll; a burst is caught up over a few polls.
const MAX_FOLLOW_BYTES: u64 = 4 * 1024 * 1024;
/// Codex rollouts this recent are candidates when the agent didn't report.
const CODEX_RECENT: Duration = Duration::from_secs(7 * 24 * 3600);

/// One phone connection's watch on one chat.
pub struct ChatWatch {
    task: JoinHandle<()>,
    older: mpsc::Sender<()>,
}

impl ChatWatch {
    pub fn spawn(pane_id: String, ui_rx: watch::Receiver<UiState>, out: mpsc::Sender<ServerMsg>, places: Places) -> Self {
        let (older, older_rx) = mpsc::channel(4);
        let task = tokio::spawn(follow(pane_id, ui_rx, out, places, older_rx));
        Self { task, older }
    }

    /// Ask for one more page of older items.
    pub fn more(&self) {
        let _ = self.older.try_send(());
    }
}

impl Drop for ChatWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Where transcripts may be read from.
#[derive(Debug, Clone)]
pub struct Places {
    pub home: PathBuf,
    /// The demo's folder of fake transcripts (`serve --demo` only).
    pub demo: Option<PathBuf>,
}

impl Places {
    fn root(&self, format: Format) -> PathBuf {
        match format {
            Format::Claude => self.home.join(".claude/projects"),
            Format::Codex => self.home.join(".codex/sessions"),
            Format::Pi => self.home.join(".pi/agent/sessions"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Target {
    format: Format,
    path: PathBuf,
}

async fn follow(
    pane_id: String,
    ui_rx: watch::Receiver<UiState>,
    out: mpsc::Sender<ServerMsg>,
    places: Places,
    mut older_rx: mpsc::Receiver<()>,
) {
    let mut tail: Option<Tail> = None;
    let mut said_unavailable: Option<String> = None;
    let mut resolved_at: Option<Instant> = None;
    let mut ticker = tokio::time::interval(POLL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            asked = older_rx.recv() => {
                if asked.is_none() { return }
                if let Some(t) = tail.as_mut() {
                    let items = tokio::task::block_in_place(|| t.older()).unwrap_or_default();
                    page_log(&pane_id, &items, t.stats);
                    let msg = chat(&pane_id, ChatMode::Older, items, t.start > 0, None);
                    if out.send(msg).await.is_err() { return }
                }
                continue;
            }
        }

        // Which transcript? Re-checked now and then: a new conversation
        // means a new file.
        let due = resolved_at.is_none_or(|at| at.elapsed() >= RESOLVE_EVERY);
        if tail.is_none() || due {
            resolved_at = Some(Instant::now());
            let found = {
                let ui = ui_rx.borrow();
                ui.store.agents.get(&pane_id).cloned()
            }
            .ok_or_else(|| "this agent is gone".to_string())
            .and_then(|agent| resolve(&agent, &pane_id, &places, &ui_rx).map(|t| (t, agent)));
            match found {
                Ok((target, agent)) if tail.as_ref().map(|t| &t.target) != Some(&target) => {
                    match tokio::task::block_in_place(|| Tail::open(target, &agent, &places)) {
                        Ok((t, items)) => {
                            page_log(&pane_id, &items, t.stats);
                            let msg = chat(&pane_id, ChatMode::Reset, items, t.start > 0, None);
                            tail = Some(t);
                            said_unavailable = None;
                            if out.send(msg).await.is_err() { return }
                        }
                        Err(err) => tracing::debug!(%err, pane_id, "couldn't open transcript"),
                    }
                    continue;
                }
                Ok(_) => {}
                Err(reason) => {
                    tail = None;
                    if said_unavailable.as_deref() != Some(reason.as_str()) {
                        let msg = chat(&pane_id, ChatMode::Reset, Vec::new(), false, Some(reason.clone()));
                        said_unavailable = Some(reason);
                        if out.send(msg).await.is_err() { return }
                    }
                    continue;
                }
            }
        }

        let Some(t) = tail.as_mut() else { continue };
        match tokio::task::block_in_place(|| t.newer()) {
            Ok(items) if items.is_empty() => {}
            Ok(items) => {
                let msg = chat(&pane_id, ChatMode::Append, items, t.start > 0, None);
                if out.send(msg).await.is_err() { return }
            }
            // Truncated or replaced: start over on the next tick.
            Err(err) => {
                tracing::debug!(%err, pane_id, "transcript changed under us");
                tail = None;
            }
        }
    }
}

/// What a page cost to read (debug log; no content).
fn page_log(pane_id: &str, items: &[ChatItem], stats: Stats) {
    tracing::debug!(
        pane_id,
        items = items.len(),
        read_ms = stats.read.as_secs_f64() * 1e3,
        parse_ms = stats.parse.as_secs_f64() * 1e3,
        bytes = stats.bytes,
        "chat page"
    );
}

fn chat(pane_id: &str, mode: ChatMode, items: Vec<ChatItem>, has_older: bool, unavailable: Option<String>) -> ServerMsg {
    ServerMsg::Chat { pane_id: pane_id.to_string(), mode, items, has_older, unavailable }
}

// --- finding the transcript --------------------------------------------------

fn resolve(agent: &Agent, pane_id: &str, places: &Places, ui_rx: &watch::Receiver<UiState>) -> Result<Target, String> {
    let kind = agent.agent.as_deref().unwrap_or("agent");
    let format = Format::for_kind(kind).ok_or_else(|| format!("no chat view for {kind} agents yet"))?;

    if let Some(demo) = &places.demo {
        let path = demo.join(format!("{}.jsonl", pane_id.replace(':', "-")));
        return if path.is_file() {
            Ok(Target { format, path })
        } else {
            Err("this demo agent has no transcript".into())
        };
    }

    let root = places.root(format);
    let session = agent.agent_session.as_ref();
    let value = session.and_then(|s| s["value"].as_str()).unwrap_or("");
    let path = match (format, session.and_then(|s| s["kind"].as_str())) {
        (_, Some("path")) => inside(&root, Path::new(value)),
        (Format::Claude, Some("id")) => claude_by_id(&root, value),
        (Format::Codex, Some("id")) => codex_by_id(&root, value),
        (Format::Pi, Some("id")) => pi_by_id(&root, value),
        // herdr's codex integration doesn't always report: go by directory,
        // and when several codex agents share one, by thread name.
        (Format::Codex, _) => {
            let cwd = agent.cwd.as_deref().unwrap_or("");
            let sharing = ui_rx
                .borrow()
                .store
                .agents
                .values()
                .filter(|a| a.agent.as_deref() == Some("codex") && a.cwd.as_deref() == Some(cwd))
                .count();
            let threads = if sharing > 1 {
                let name = codex_thread_name(agent);
                let named = name.as_deref().map(|name| codex_threads_named(&places.home, name));
                tracing::debug!(pane_id, ?name, threads = ?named, "codex agents share a directory");
                match named {
                    Some(ids) if !ids.is_empty() => Some(ids),
                    _ => return Err("more than one codex agent works in this directory, so its conversation can't be told apart".into()),
                }
            } else {
                None
            };
            codex_by_cwd(&root, cwd, threads.as_deref())
        }
        _ => return Err(format!("{kind} hasn't said which conversation it's in yet (is herdr's {kind} integration installed?)")),
    };
    path.map(|path| Target { format, path }).ok_or_else(|| "couldn't find this conversation's transcript".into())
}

/// `path`, if it's a transcript file inside `root` (after resolving links).
fn inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let path = std::fs::canonicalize(path).ok()?;
    (path.starts_with(&root) && path.is_file() && path.extension().is_some_and(|e| e == "jsonl")).then_some(path)
}

/// Session ids are uuid-like; anything else is not looked up.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// `~/.claude/projects/<project>/<id>.jsonl`
fn claude_by_id(root: &Path, id: &str) -> Option<PathBuf> {
    if !valid_id(id) {
        return None;
    }
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|project| project.path().join(format!("{id}.jsonl")))
        .find(|p| p.is_file())
        .and_then(|p| inside(root, &p))
}

/// `~/.codex/sessions/<y>/<m>/<d>/rollout-<time>-<id>.jsonl`, newest days first.
fn codex_by_id(root: &Path, id: &str) -> Option<PathBuf> {
    if !valid_id(id) {
        return None;
    }
    let suffix = format!("-{id}.jsonl");
    codex_days(root)
        .into_iter()
        .flat_map(|day| files(&day))
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rollout-") && n.ends_with(&suffix)))
        .and_then(|p| inside(root, &p))
}

/// The newest recent rollout that started in `cwd` (and, given `threads`,
/// is one of them).
fn codex_by_cwd(root: &Path, cwd: &str, threads: Option<&[String]>) -> Option<PathBuf> {
    if cwd.is_empty() {
        return None;
    }
    let now = SystemTime::now();
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for day in codex_days(root).into_iter().take(8) {
        for path in files(&day) {
            let Some(modified) = path.metadata().and_then(|m| m.modified()).ok() else { continue };
            if now.duration_since(modified).unwrap_or_default() > CODEX_RECENT {
                continue;
            }
            if best.as_ref().is_some_and(|(t, _)| *t >= modified) {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if threads.is_some_and(|ids| !ids.iter().any(|id| name.ends_with(&format!("-{id}.jsonl")))) {
                continue;
            }
            if first_line(&path).is_some_and(|meta| meta["payload"]["cwd"] == cwd) {
                best = Some((modified, path));
            }
        }
    }
    best.and_then(|(_, p)| inside(root, &p))
}

/// Codex titles its terminal `<thread name> | <directory>` (herdr may cut
/// it short with `...`).
fn codex_thread_name(agent: &Agent) -> Option<String> {
    let title = agent.terminal_title_stripped.as_deref().or(agent.terminal_title.as_deref())?;
    let name = title.split(" | ").next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Thread ids with this name, from `~/.codex/session_index.jsonl`.
fn codex_threads_named(home: &Path, name: &str) -> Vec<String> {
    let Ok(index) = std::fs::read_to_string(home.join(".codex/session_index.jsonl")) else {
        return Vec::new();
    };
    let cut = name.strip_suffix("...").or_else(|| name.strip_suffix('…'));
    index
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| {
            let thread = entry["thread_name"].as_str().unwrap_or("");
            match cut {
                Some(prefix) => thread.starts_with(prefix.trim_end()),
                None => thread == name,
            }
        })
        .filter_map(|entry| entry["id"].as_str().map(str::to_string))
        .collect()
}

/// `~/.pi/agent/sessions/<project>/<time>_<id>.jsonl`
fn pi_by_id(root: &Path, id: &str) -> Option<PathBuf> {
    if !valid_id(id) {
        return None;
    }
    let suffix = format!("_{id}.jsonl");
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .flat_map(|project| files(&project.path()))
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&suffix)))
        .and_then(|p| inside(root, &p))
}

/// Codex's `<y>/<m>/<d>` folders, newest first.
fn codex_days(root: &Path) -> Vec<PathBuf> {
    let mut days = Vec::new();
    for year in sorted_dirs(root) {
        for month in sorted_dirs(&year) {
            days.extend(sorted_dirs(&month));
        }
    }
    days.sort();
    days.reverse();
    days
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

fn files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect()
}

fn first_line(path: &Path) -> Option<Value> {
    let mut buf = vec![0u8; 64 * 1024];
    let n = File::open(path).ok()?.read(&mut buf).ok()?;
    let line = buf[..n].split(|b| *b == b'\n').next()?;
    serde_json::from_slice(line).ok()
}

// --- reading -----------------------------------------------------------------

/// An open transcript: what's loaded is the byte range `start..end`.
struct Tail {
    target: Target,
    window: Window,
    cwd: Option<String>,
    home: Option<String>,
    start: u64,
    end: u64,
    /// Time spent on the last page, for the debug log and the speed test.
    stats: Stats,
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    read: Duration,
    parse: Duration,
    bytes: u64,
}

impl Tail {
    /// Open at the end and read the latest page.
    fn open(target: Target, agent: &Agent, places: &Places) -> std::io::Result<(Self, Vec<ChatItem>)> {
        let len = std::fs::metadata(&target.path)?.len();
        let format = match &places.demo {
            Some(_) => sniff(&target.path).unwrap_or(target.format),
            None => target.format,
        };
        let mut tail = Tail {
            target: Target { format, path: target.path },
            window: Window::default(),
            cwd: agent.cwd.clone(),
            home: places.home.to_str().map(str::to_string),
            start: len,
            end: len,
            stats: Stats::default(),
        };
        // A line still being written isn't ours yet.
        tail.end = tail.last_newline_before(len)?;
        tail.start = tail.end;
        let items = tail.older()?;
        Ok((tail, items))
    }

    /// One page of items before `start`.
    fn older(&mut self) -> std::io::Result<Vec<ChatItem>> {
        self.stats = Stats::default();
        let mut file = File::open(&self.target.path)?;
        let mut items = Vec::new();
        let mut read = 0;
        while items.len() < PAGE && self.start > 0 && read < MAX_PAGE_BYTES {
            let Some((lines_start, buf_from, buf)) = self.lines_before(&mut file)? else {
                self.start = 0; // a line too long to page past
                break;
            };
            let body = &buf[(lines_start - buf_from) as usize..];
            let started = Instant::now();
            items.extend(self.window.add(self.parse(lines_start, body)));
            self.stats.parse += started.elapsed();
            read += self.start - lines_start;
            self.start = lines_start;
        }
        items.sort_by_key(|i| i.id);
        Ok(items)
    }

    /// The bytes before `start` back to a line start, read once: a line
    /// longer than a chunk grows the read further back instead of starting
    /// over. Returns `(lines_start, buf_start, buf)`.
    fn lines_before(&mut self, file: &mut File) -> std::io::Result<Option<(u64, u64, Vec<u8>)>> {
        let mut from = self.start;
        let mut step = CHUNK;
        let mut buf = Vec::new();
        loop {
            let to = from;
            from = from.saturating_sub(step);
            let started = Instant::now();
            let mut head = read_range(file, from, to)?;
            self.stats.read += started.elapsed();
            self.stats.bytes += head.len() as u64;
            head.extend_from_slice(&buf);
            buf = head;
            if from == 0 {
                return Ok(Some((0, 0, buf)));
            }
            // The last byte is the newline that ends the line before
            // `start`; the boundary we need is an earlier one. (A chunk that
            // begins exactly on a line start just leaves that line for the
            // next page.)
            if let Some(i) = buf[..buf.len() - 1].iter().position(|b| *b == b'\n') {
                return Ok(Some((from + i as u64 + 1, from, buf)));
            }
            if self.start - from >= MAX_LINE {
                return Ok(None);
            }
            step *= 2;
        }
    }

    /// Items written since the last look (new ones, and ones that changed).
    fn newer(&mut self) -> std::io::Result<Vec<ChatItem>> {
        let len = std::fs::metadata(&self.target.path)?.len();
        if len < self.end {
            return Err(std::io::Error::other("transcript got shorter"));
        }
        if len == self.end {
            return Ok(Vec::new());
        }
        let mut file = File::open(&self.target.path)?;
        let upto = len.min(self.end + MAX_FOLLOW_BYTES);
        let buf = read_range(&mut file, self.end, upto)?;
        let Some(last) = buf.iter().rposition(|b| *b == b'\n') else {
            return Ok(Vec::new()); // a line still being written
        };
        let body = &buf[..=last];
        let entries = self.parse(self.end, body);
        self.end += body.len() as u64;
        Ok(self.window.add(entries))
    }

    fn parse(&self, offset: u64, body: &[u8]) -> Vec<Entry> {
        let ctx = Ctx { cwd: self.cwd.as_deref(), home: self.home.as_deref() };
        let mut entries = Vec::new();
        let mut pos = 0usize;
        for line in body.split_inclusive(|b| *b == b'\n') {
            if let Ok(text) = std::str::from_utf8(line) {
                entries.extend(parse_line(self.target.format, offset + pos as u64, text, &ctx));
            }
            pos += line.len();
        }
        entries
    }

    fn last_newline_before(&self, len: u64) -> std::io::Result<u64> {
        let mut file = File::open(&self.target.path)?;
        let from = len.saturating_sub(MAX_FOLLOW_BYTES);
        let buf = read_range(&mut file, from, len)?;
        Ok(match buf.iter().rposition(|b| *b == b'\n') {
            Some(i) => from + i as u64 + 1,
            None => from,
        })
    }
}

fn read_range(file: &mut File, from: u64, to: u64) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; (to - from) as usize];
    file.seek(SeekFrom::Start(from))?;
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Tell a transcript's format by its first line (the demo's fake agents
/// all write Claude's).
fn sniff(path: &Path) -> Option<Format> {
    let first = first_line(path)?;
    match first["type"].as_str()? {
        "session_meta" => Some(Format::Codex),
        "session" => Some(Format::Pi),
        _ => Some(Format::Claude),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shepsky_core::protocol::ChatBody;
    use std::io::Write;

    fn line(text: &str) -> String {
        let record = serde_json::json!({
            "type": "assistant",
            "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] },
        });
        format!("{record}\n")
    }

    fn texts(items: &[ChatItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| match &i.body {
                ChatBody::Reply { text } => text.clone(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shepsky-chat-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pages_backwards_then_follows() {
        let dir = temp("tail");
        let path = dir.join("t.jsonl");
        let mut file = File::create(&path).unwrap();
        for n in 0..(PAGE * 3) {
            // Padding makes lines straddle chunk edges; every 50th is longer
            // than a whole chunk (Claude logs multi-MB lines).
            let pad = if n % 50 == 7 { CHUNK as usize * 3 } else { n * 37 % 3000 };
            file.write_all(line(&format!("{n} {}", "x".repeat(pad))).as_bytes()).unwrap();
        }
        file.write_all(b"{\"type\":\"assistant\",\"partial").unwrap(); // still being written
        file.flush().unwrap();

        let places = Places { home: dir.clone(), demo: None };
        let target = Target { format: Format::Claude, path: path.clone() };
        let (mut tail, first) = Tail::open(target, &Agent::default(), &places).unwrap();
        let mut seen: Vec<String> = texts(&first);
        assert!(seen.last().unwrap().starts_with(&format!("{} ", PAGE * 3 - 1)));
        while tail.start > 0 {
            let mut page = texts(&tail.older().unwrap());
            page.extend(seen);
            seen = page;
        }
        let numbers: Vec<usize> = seen.iter().map(|t| t.split(' ').next().unwrap().parse().unwrap()).collect();
        assert_eq!(numbers, (0..PAGE * 3).collect::<Vec<_>>(), "every line exactly once, in order");

        // The partial line completes, and another arrives.
        file.write_all(b"\"}\n").unwrap(); // finishes a junk line: skipped
        file.write_all(line("fresh").as_bytes()).unwrap();
        file.flush().unwrap();
        assert_eq!(texts(&tail.newer().unwrap()), ["fresh"]);
        assert!(tail.newer().unwrap().is_empty());
    }

    /// A made-up Claude transcript shaped like a long real one: big tool
    /// outputs, base64 screenshots, multi-MB request blobs that carry no chat
    /// items, redacted thinking with long signatures. Never a real file.
    pub(super) fn synthetic_transcript(path: &Path, target_bytes: usize) {
        let mut out = std::io::BufWriter::new(File::create(path).unwrap());
        let mut seed: u64 = 7;
        let mut rand = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n
        };
        let text = |len: usize| "lorem ipsum dolor sit amet consectetur ".repeat(len / 39 + 1)[..len].to_string();
        let mut written = 0usize;
        let mut turn = 0;
        while written < target_bytes {
            turn += 1;
            let mut lines = vec![
                serde_json::json!({"type": "user", "message": {"role": "user", "content": format!("prompt {turn}: {}", text(200))}}),
                serde_json::json!({"type": "attachment", "attachment": {"type": "total_tokens_reminder", "text": text(300)}}),
                serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "thinking", "thinking": "", "signature": "S".repeat(1500)}]}}),
                serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text(600)}]}}),
            ];
            for call in 0..(3 + rand(5)) {
                let id = format!("toolu_{turn}_{call}");
                if rand(10) == 0 {
                    // A screenshot read back: ~800 KB of base64.
                    lines.push(serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": "Read", "input": {"file_path": "/tmp/shot.png"}}]}}));
                    lines.push(serde_json::json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBOR".repeat(160_000)}}]}]}}));
                } else {
                    let size = if rand(8) == 0 { 200_000 } else { 2_000 + rand(30_000) };
                    lines.push(serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": format!("cargo test --all {call}"), "description": "run"}}]}}));
                    lines.push(serde_json::json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": text(size), "is_error": false}]}}));
                }
            }
            if rand(3) == 0 {
                // What Claude Code logs about each API request: no chat items.
                lines.push(serde_json::json!({"type": "api-request-blob", "blob": text(1_000_000 + rand(1_500_000))}));
            }
            for line in lines {
                let line = format!("{line}\n");
                written += line.len();
                out.write_all(line.as_bytes()).unwrap();
            }
        }
    }

    /// How long a big transcript takes to open and page through:
    ///   cargo test --release -p shepsky-bridge chat_speed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn chat_speed() {
        let dir = temp("speed");
        let path = dir.join("big.jsonl");
        synthetic_transcript(&path, 50 << 20);
        let mb = std::fs::metadata(&path).unwrap().len() as f64 / 1e6;
        println!("synthetic transcript: {mb:.1} MB");
        // To try it in the app: put it where a demo agent's transcript goes.
        if let Some(out) = std::env::var_os("SHEPSKY_SYNTHETIC_OUT") {
            std::fs::copy(&path, out).unwrap();
        }

        let places = Places { home: dir.clone(), demo: None };
        let target = Target { format: Format::Claude, path: path.clone() };
        let started = Instant::now();
        let (mut tail, first) = Tail::open(target, &Agent::default(), &places).unwrap();
        let open = started.elapsed();
        let report = |label: &str, items: &[ChatItem], total: Duration, stats: Stats| {
            let json = serde_json::to_string(&chat("w1:p1", ChatMode::Older, items.to_vec(), true, None)).unwrap();
            println!(
                "{label:>10}: {:>4} items  {:>6.1} KB json  {:>6.1} ms total  (read {:>5.1} ms, parse {:>6.1} ms, {:>6.2} MB read)",
                items.len(),
                json.len() as f64 / 1024.0,
                total.as_secs_f64() * 1e3,
                stats.read.as_secs_f64() * 1e3,
                stats.parse.as_secs_f64() * 1e3,
                stats.bytes as f64 / 1e6,
            );
        };
        report("first page", &first, open, tail.stats);
        for n in 1..=5 {
            let started = Instant::now();
            let page = tail.older().unwrap();
            report(&format!("older {n}"), &page, started.elapsed(), tail.stats);
        }
        // The whole file, for a throughput figure.
        let started = Instant::now();
        let text = std::fs::read_to_string(&path).unwrap();
        let mut offset = 0u64;
        let mut items = 0;
        for line in text.split_inclusive('\n') {
            items += parse_line(Format::Claude, offset, line, &Ctx::default()).len();
            offset += line.len() as u64;
        }
        let secs = started.elapsed().as_secs_f64();
        println!("whole file: {items} items in {:.0} ms ({:.0} MB/s)", secs * 1e3, mb / secs);
    }

    #[test]
    fn transcripts_are_only_read_inside_their_folder() {
        let home = temp("home");
        let projects = home.join(".claude/projects/-Users-demo-app");
        std::fs::create_dir_all(&projects).unwrap();
        let id = "0b7e2a9c-1111-2222-3333-444455556666";
        std::fs::write(projects.join(format!("{id}.jsonl")), line("hi")).unwrap();
        std::fs::write(home.join("secret.jsonl"), "{}\n").unwrap();
        let root = home.join(".claude/projects");

        assert!(claude_by_id(&root, id).is_some());
        assert!(claude_by_id(&root, "../../secret").is_none(), "ids can't be paths");
        assert!(inside(&root, &home.join("secret.jsonl")).is_none());
        assert!(inside(&root, &projects.join("../../../secret.jsonl")).is_none());
    }

    #[test]
    fn codex_by_directory_picks_the_newest_rollout_there() {
        let root = temp("codex").join("sessions");
        let day = root.join("2026/10/23");
        std::fs::create_dir_all(&day).unwrap();
        let meta = |cwd: &str| format!("{}\n", serde_json::json!({"type": "session_meta", "payload": {"cwd": cwd}}));
        std::fs::write(day.join("rollout-a-1.jsonl"), meta("/w/app")).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(day.join("rollout-b-2.jsonl"), meta("/w/other")).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(day.join("rollout-c-3.jsonl"), meta("/w/app")).unwrap();

        let found = codex_by_cwd(&root, "/w/app", None).unwrap();
        assert!(found.ends_with("rollout-c-3.jsonl"));
        assert!(codex_by_cwd(&root, "/w/none", None).is_none());
        // Two agents in /w/app: the thread name picks the older one.
        let only_1 = ["1".to_string()];
        assert!(codex_by_cwd(&root, "/w/app", Some(&only_1)).unwrap().ends_with("rollout-a-1.jsonl"));
        assert!(codex_by_id(&root, "2").unwrap().ends_with("rollout-b-2.jsonl"));

        let home = root.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex/session_index.jsonl"),
            "{\"id\":\"1\",\"thread_name\":\"Fix the parser\"}\n{\"id\":\"3\",\"thread_name\":\"Ship it\"}\n",
        )
        .unwrap();
        assert_eq!(codex_threads_named(&home, "Fix the parser"), ["1"]);
        assert_eq!(codex_threads_named(&home, "Fix the p..."), ["1"]);
        let agent = Agent { terminal_title_stripped: Some("Ship it | app".into()), ..Agent::default() };
        assert_eq!(codex_thread_name(&agent).as_deref(), Some("Ship it"));
    }
}
