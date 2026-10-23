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

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use shepsky_core::herdr::Agent;
use shepsky_core::protocol::{ChatItem, ChatMode, ServerMsg};
use shepsky_core::transcript::{Ctx, Entry, Format, Window, parse_line};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::herdr_client::HerdrClient;
use crate::secure_file;
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
const MAX_LINE: u64 = 8 * 1024 * 1024;
/// New bytes read per poll; a burst is caught up over a few polls.
const MAX_FOLLOW_BYTES: u64 = 4 * 1024 * 1024;
/// Codex rollouts this recent are candidates when the agent didn't report.
const CODEX_RECENT: Duration = Duration::from_secs(7 * 24 * 3600);
/// At most this many codex conversations are compared with the screen.
const CODEX_CANDIDATES: usize = 8;
/// How much of each candidate's end is compared with the screen.
const CODEX_TAIL: u64 = 2 * 1024 * 1024;
/// How long a screen-matched choice stands before it's checked again (you
/// can switch conversations inside codex).
const CODEX_RECHECK: Duration = Duration::from_secs(30);

/// One phone connection's watch on one chat.
pub struct ChatWatch {
    task: JoinHandle<()>,
    older: mpsc::Sender<()>,
}

impl ChatWatch {
    pub fn spawn(
        pane_id: String,
        client: HerdrClient,
        ui_rx: watch::Receiver<UiState>,
        out: mpsc::Sender<ServerMsg>,
        places: Places,
    ) -> Self {
        let (older, older_rx) = mpsc::channel(4);
        let task = tokio::spawn(follow(pane_id, client, ui_rx, out, places, older_rx));
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
    client: HerdrClient,
    ui_rx: watch::Receiver<UiState>,
    out: mpsc::Sender<ServerMsg>,
    places: Places,
    mut older_rx: mpsc::Receiver<()>,
) {
    let mut tail: Option<Tail> = None;
    let mut said_unavailable: Option<String> = None;
    let mut resolved_at: Option<Instant> = None;
    let mut metas = CodexMetas::default();
    let mut choice: Option<Choice> = None;
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
        // Every few seconds (a new conversation means a new file), and right
        // away after the transcript went missing; not on every poll while
        // there's nothing to show.
        let due = resolved_at.is_none_or(|at| at.elapsed() >= RESOLVE_EVERY);
        if due {
            resolved_at = Some(Instant::now());
            let agent = ui_rx.borrow().store.agents.get(&pane_id).cloned();
            let resolved = match &agent {
                Some(agent) => tokio::task::block_in_place(|| resolve(agent, &pane_id, &places, &ui_rx, &mut metas)),
                None => Err("this agent is gone".to_string()),
            };
            let found = match resolved {
                Ok(Resolved::One(target)) => Ok(target),
                Ok(Resolved::Codex { candidates, lone }) => choose(&client, &pane_id, candidates, lone, &mut choice, &places.root(Format::Codex))
                    .await
                    .map(|path| Target { format: Format::Codex, path }),
                Err(reason) => Err(reason),
            }
            .map(|target| (target, agent.unwrap_or_default()));
            match found {
                Ok((target, agent)) if tail.as_ref().map(|t| &t.target) != Some(&target) => {
                    tracing::debug!(pane_id, path = %target.path.display(), "chat transcript");
                    match tokio::task::block_in_place(|| Tail::open(target, &agent, &places)) {
                        Ok((t, items)) => {
                            page_log(&pane_id, &items, t.stats);
                            let msg = chat(&pane_id, ChatMode::Reset, items, t.start > 0, None);
                            tail = Some(t);
                            said_unavailable = None;
                            if out.send(msg).await.is_err() { return }
                        }
                        Err(err) => {
                            tracing::debug!(%err, pane_id, "couldn't open transcript");
                            tail = None;
                        }
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
                resolved_at = None;
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

/// What a pane's transcript turned out to be.
enum Resolved {
    One(Target),
    /// Several codex conversations could be this agent's; what's on its
    /// screen decides. `lone`: it's the only codex agent in its directory,
    /// so the newest is a fair guess if the screen doesn't say.
    Codex { candidates: Vec<PathBuf>, lone: bool },
}

fn resolve(
    agent: &Agent,
    pane_id: &str,
    places: &Places,
    ui_rx: &watch::Receiver<UiState>,
    metas: &mut CodexMetas,
) -> Result<Resolved, String> {
    let kind = agent.agent.as_deref().unwrap_or("agent");
    let format = Format::for_kind(kind).ok_or_else(|| format!("no chat view for {kind} agents yet"))?;

    if let Some(demo) = &places.demo {
        let path = demo.join(format!("{}.jsonl", pane_id.replace(':', "-")));
        return if path.is_file() {
            Ok(Resolved::One(Target { format, path }))
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
        // herdr's codex integration often can't report (codex hooks run in
        // its shared app-server, which doesn't know the pane): the
        // conversations started in the agent's directory, narrowed by the
        // thread name in its title; the screen picks among what's left.
        (Format::Codex, _) => {
            let cwd = agent.cwd.as_deref().unwrap_or("");
            let lone = ui_rx
                .borrow()
                .store
                .agents
                .values()
                .filter(|a| a.agent.as_deref() == Some("codex") && a.cwd.as_deref() == Some(cwd))
                .count()
                <= 1;
            let name = codex_thread_name(agent);
            let named = name.as_deref().map(|n| codex_threads_named(&places.home, n)).filter(|ids| !ids.is_empty());
            let mut candidates = codex_candidates(&root, cwd, named.as_deref(), metas);
            if candidates.is_empty() && named.is_some() {
                candidates = codex_candidates(&root, cwd, None, metas);
            }
            tracing::debug!(pane_id, ?name, candidates = candidates.len(), "codex conversations");
            return match candidates.len() {
                0 => Err("couldn't find this conversation's transcript".into()),
                1 => Ok(Resolved::One(Target { format, path: candidates.remove(0) })),
                _ => Ok(Resolved::Codex { candidates, lone }),
            };
        }
        _ => return Err(format!("{kind} hasn't said which conversation it's in yet (is herdr's {kind} integration installed?)")),
    };
    path.map(|path| Resolved::One(Target { format, path }))
        .ok_or_else(|| "couldn't find this conversation's transcript".into())
}

/// A screen-matched pick among codex conversations, kept for a while.
struct Choice {
    candidates: Vec<PathBuf>,
    chosen: Option<PathBuf>,
    at: Instant,
}

/// Which of several codex conversations is on this pane's screen: the one
/// whose end shares the most six-word runs with the visible text, if it's
/// clearly ahead. Reading the visible screen is read-only.
async fn choose(
    client: &HerdrClient,
    pane_id: &str,
    candidates: Vec<PathBuf>,
    lone: bool,
    cache: &mut Option<Choice>,
    root: &Path,
) -> Result<PathBuf, String> {
    let cached = cache.as_ref().filter(|c| c.candidates == candidates && c.at.elapsed() < CODEX_RECHECK);
    let chosen = match cached {
        Some(c) => c.chosen.clone(),
        None => {
            let read = client
                .call("pane.read", serde_json::json!({ "pane_id": pane_id, "source": "visible", "format": "text" }))
                .await;
            let screen = read.ok().and_then(|r| r["read"]["text"].as_str().map(str::to_string)).unwrap_or_default();
            let chosen = tokio::task::block_in_place(|| best_match(&screen, &candidates, root));
            tracing::debug!(pane_id, chosen = ?chosen.as_ref().map(|p| p.display().to_string()), "codex conversation by screen");
            *cache = Some(Choice { candidates: candidates.clone(), chosen: chosen.clone(), at: Instant::now() });
            chosen
        }
    };
    match chosen {
        Some(path) => Ok(path),
        // The newest, when it's the only codex agent here.
        None if lone => Ok(candidates[0].clone()),
        None => Err("several codex conversations could be this agent's, and its screen doesn't say which".into()),
    }
}

/// The candidate whose end best matches `screen`, if any clearly does.
fn best_match(screen: &str, candidates: &[PathBuf], root: &Path) -> Option<PathBuf> {
    let wanted: HashSet<u64> = runs(screen).collect();
    if wanted.is_empty() {
        return None;
    }
    let mut scores: Vec<(usize, &PathBuf)> = candidates
        .iter()
        .map(|path| {
            let tail = secure_file::open_under(root, path)
                .and_then(|mut f| {
                    let len = f.metadata()?.len();
                    read_range(&mut f, len.saturating_sub(CODEX_TAIL), len)
                })
                .unwrap_or_default();
            let text = String::from_utf8_lossy(&tail);
            let found: HashSet<u64> = runs(&text).filter(|r| wanted.contains(r)).collect();
            (found.len(), path)
        })
        .collect();
    scores.sort_by_key(|s| std::cmp::Reverse(s.0));
    let (best, path) = scores[0];
    let second = scores.get(1).map_or(0, |s| s.0);
    (best >= 8 && best > 2 * second).then(|| path.clone())
}

/// Six-word runs of `text` (words of 3+ letters/digits), hashed. JSON escapes
/// count as breaks, so a terminal line and its transcript text agree however
/// either one wrapped.
fn runs(text: &str) -> impl Iterator<Item = u64> + '_ {
    let mut words: Vec<&str> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut start: Option<usize> = None;
    while i < bytes.len() {
        let b = bytes[i];
        let word = b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
        if word {
            start.get_or_insert(i);
            i += 1;
            continue;
        }
        if let Some(s) = start.take()
            && i - s >= 3
        {
            words.push(&text[s..i]);
        }
        // `\n`, `\"`…: the escaped character is a break too (a whole
        // character, so a multi-byte one isn't split).
        i += 1;
        if b == b'\\'
            && let Some(c) = text[i..].chars().next()
        {
            i += c.len_utf8();
        }
    }
    if let Some(s) = start
        && bytes.len() - s >= 3
    {
        words.push(&text[s..]);
    }
    (0..words.len().saturating_sub(5)).map(move |i| {
        let mut h = DefaultHasher::new();
        words[i..i + 6].hash(&mut h);
        h.finish()
    })
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

/// What a rollout's first line says: where it started, and whether it's an
/// interactive conversation (not a headless `codex exec` run or a sub-agent).
type CodexMetas = HashMap<PathBuf, Option<(String, bool)>>;

/// Recent interactive rollouts that started in `cwd` (and, given `threads`,
/// are one of them), newest first.
fn codex_candidates(root: &Path, cwd: &str, threads: Option<&[String]>, metas: &mut CodexMetas) -> Vec<PathBuf> {
    if metas.len() >= 512 {
        metas.clear();
    }
    if cwd.is_empty() {
        return Vec::new();
    }
    let now = SystemTime::now();
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    for day in codex_days(root).into_iter().take(8) {
        for path in files(&day) {
            let Some(modified) = path.metadata().and_then(|m| m.modified()).ok() else { continue };
            if now.duration_since(modified).unwrap_or_default() > CODEX_RECENT {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if threads.is_some_and(|ids| !ids.iter().any(|id| name.ends_with(&format!("-{id}.jsonl")))) {
                continue;
            }
            if metas.len() >= 512 {
                metas.clear();
            }
            let meta = metas.entry(path.clone()).or_insert_with(|| {
                let mut file = secure_file::open_under(root, &path).ok()?;
                let first = first_line(&mut file)?;
                let p = &first["payload"];
                let interactive = p["source"] != "exec"
                    && p["source"].get("subagent").is_none()
                    && p["parent_thread_id"].as_str().is_none_or(str::is_empty);
                Some((p["cwd"].as_str()?.to_string(), interactive))
            });
            if meta.as_ref().is_some_and(|(dir, interactive)| *interactive && dir == cwd) {
                found.push((modified, path));
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().take(CODEX_CANDIDATES).filter_map(|(_, p)| inside(root, &p)).collect()
}

/// Codex titles its terminal `<thread name> | <directory>` (herdr may cut
/// it short with `...`); a thread without a name yet is just `<directory>`.
fn codex_thread_name(agent: &Agent) -> Option<String> {
    let title = agent.terminal_title_stripped.as_deref().or(agent.terminal_title.as_deref())?;
    let (name, _dir) = title.split_once(" | ")?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Thread ids with this name, from `~/.codex/session_index.jsonl`.
fn codex_threads_named(home: &Path, name: &str) -> Vec<String> {
    let mut index = String::new();
    let path = home.join(".codex/session_index.jsonl");
    let Ok(file) = secure_file::open_under(&home.join(".codex"), &path) else {
        return Vec::new();
    };
    if file.take(4 * 1024 * 1024 + 1).read_to_string(&mut index).is_err() || index.len() > 4 * 1024 * 1024 {
        return Vec::new();
    }
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

fn first_line(file: &mut File) -> Option<Value> {
    let mut buf = vec![0u8; 64 * 1024];
    file.seek(SeekFrom::Start(0)).ok()?;
    let n = file.read(&mut buf).ok()?;
    let line = buf[..n].split(|b| *b == b'\n').next()?;
    serde_json::from_slice(line).ok()
}

// --- reading -----------------------------------------------------------------

/// An open transcript: what's loaded is the byte range `start..end`.
struct Tail {
    target: Target,
    file: File,
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
        let root = places.demo.clone().unwrap_or_else(|| places.root(target.format));
        let mut file = secure_file::open_under(&root, &target.path)?;
        let len = file.metadata()?.len();
        let format = match &places.demo {
            Some(_) => sniff(&mut file).unwrap_or(target.format),
            None => target.format,
        };
        let mut tail = Tail {
            target: Target { format, path: target.path },
            file,
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
        let mut file = self.current_file()?;
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
            step = (step * 2).min(MAX_LINE - (self.start - from));
        }
    }

    /// Items written since the last look (new ones, and ones that changed).
    fn newer(&mut self) -> std::io::Result<Vec<ChatItem>> {
        let mut file = self.current_file()?;
        let len = file.metadata()?.len();
        if len < self.end {
            return Err(std::io::Error::other("transcript got shorter"));
        }
        if len == self.end {
            return Ok(Vec::new());
        }
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
        let mut file = self.current_file()?;
        let from = len.saturating_sub(MAX_FOLLOW_BYTES);
        let buf = read_range(&mut file, from, len)?;
        Ok(match buf.iter().rposition(|b| *b == b'\n') {
            Some(i) => from + i as u64 + 1,
            None => from,
        })
    }

    fn current_file(&self) -> std::io::Result<File> {
        let opened = self.file.metadata()?;
        let current = std::fs::symlink_metadata(&self.target.path)?;
        if !current.is_file() || opened.dev() != current.dev() || opened.ino() != current.ino() {
            return Err(std::io::Error::other("transcript was replaced"));
        }
        self.file.try_clone()
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
fn sniff(file: &mut File) -> Option<Format> {
    let first = first_line(file)?;
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

        let places = Places { home: dir.clone(), demo: Some(dir.clone()) };
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

        let places = Places { home: dir.clone(), demo: Some(dir.clone()) };
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
    fn replacement_cannot_redirect_a_validated_or_open_transcript() {
        use std::os::unix::fs::symlink;
        let home = temp("replacement");
        let root = home.join(".claude/projects");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("session.jsonl");
        let outside = home.join("outside.jsonl");
        std::fs::write(&path, line("inside")).unwrap();
        std::fs::write(&outside, line("outside")).unwrap();
        let checked = inside(&root, &path).unwrap();
        let places = Places { home: home.clone(), demo: None };
        let target = Target { format: Format::Claude, path: checked };
        let (mut opened, initial) = Tail::open(target.clone(), &Agent::default(), &places).unwrap();
        assert_eq!(texts(&initial), ["inside"]);
        std::fs::rename(&path, root.join("original.jsonl")).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(Tail::open(target, &Agent::default(), &places).is_err());
        assert!(opened.older().is_err());
        assert!(opened.newer().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn codex_candidates_are_interactive_conversations_in_the_directory() {
        let root = temp("codex").join("sessions");
        let day = root.join("2026/10/23");
        std::fs::create_dir_all(&day).unwrap();
        let meta = |cwd: &str, source: &str, parent: &str| {
            format!("{}\n", serde_json::json!({"type": "session_meta", "payload": {"cwd": cwd, "source": source, "parent_thread_id": parent}}))
        };
        let write = |name: &str, body: String| {
            std::fs::write(day.join(name), body).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        };
        write("rollout-a-1.jsonl", meta("/w/app", "vscode", ""));
        write("rollout-b-2.jsonl", meta("/w/other", "cli", ""));
        write("rollout-c-3.jsonl", meta("/w/app", "cli", ""));
        write("rollout-d-4.jsonl", meta("/w/app", "exec", "")); // headless: never a pane's
        write("rollout-e-5.jsonl", meta("/w/app", "cli", "3")); // a sub-agent's thread
        let guardian = serde_json::json!({"type": "session_meta", "payload": {"cwd": "/w/app", "source": {"subagent": {"other": "guardian"}}}});
        write("rollout-f-6.jsonl", format!("{guardian}\n")); // a reviewer sub-agent

        let mut metas = CodexMetas::default();
        let names = |paths: Vec<PathBuf>| -> Vec<String> {
            paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
        };
        assert_eq!(names(codex_candidates(&root, "/w/app", None, &mut metas)), ["rollout-c-3.jsonl", "rollout-a-1.jsonl"]);
        assert!(codex_candidates(&root, "/w/none", None, &mut metas).is_empty());
        let only_1 = ["1".to_string()];
        assert_eq!(names(codex_candidates(&root, "/w/app", Some(&only_1), &mut metas)), ["rollout-a-1.jsonl"]);
        assert!(codex_by_id(&root, "2").unwrap().ends_with("rollout-b-2.jsonl"));

        // The screen picks the older conversation when that's what it shows.
        let reply = |text: &str| format!("{}\n", serde_json::json!({"type": "event_msg", "payload": {"type": "item_completed", "item": {"type": "AgentMessage", "content": [{"type": "text", "text": text}]}}}));
        let shown = "The replay service now streams frames from the recorder, and the viewer seeks by timestamp without buffering the whole episode first";
        std::fs::write(day.join("rollout-a-1.jsonl"), meta("/w/app", "vscode", "") + &reply(shown)).unwrap();
        std::fs::write(day.join("rollout-c-3.jsonl"), meta("/w/app", "cli", "") + &reply("Something else entirely about release notes and version bumps for the mobile client build")).unwrap();
        let screen = "• The replay service now streams frames from the\n  recorder, and the viewer seeks by timestamp without\n  buffering the whole episode first\n\n› ";
        let candidates = vec![day.join("rollout-c-3.jsonl"), day.join("rollout-a-1.jsonl")];
        assert_eq!(best_match(screen, &candidates, &root), Some(day.join("rollout-a-1.jsonl")));
        // A screen that matches nothing decides nothing.
        assert_eq!(best_match("OpenAI Codex (v0.164.0) model: gpt directory: ~/w/app", &candidates, &root), None);
        // Odd escapes never split a character.
        assert_eq!(runs("C:\\é one two three four five six seven \\").count(), 2);

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
        let unnamed = Agent { terminal_title_stripped: Some("worktree-brave-stone-...".into()), ..Agent::default() };
        assert_eq!(codex_thread_name(&unnamed), None);
    }
}
