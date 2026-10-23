//! Serde types for the subset of herdr's socket API that collie uses.
//!
//! herdr speaks newline-delimited JSON over a unix socket, one request per
//! connection. These types are hand-written against a live 0.9.5 server (see
//! `docs/herdr-notes.md`); unknown fields are deliberately ignored so newer
//! herdr versions keep parsing.

use serde::{Deserialize, Serialize};

/// One RPC request line. `params` must be present even when empty, and `id`
/// must be a string — herdr rejects anything else as `invalid_request`.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// One RPC response line: exactly one of `result` / `error` is set.
#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Deserialize, thiserror::Error)]
#[error("herdr error {code}: {message}")]
pub struct RpcError {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// `ping` result.
#[derive(Debug, Clone, Deserialize)]
pub struct Pong {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub protocol: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    /// Finished and not yet looked at. Flips back to `Idle` *silently* (no
    /// event) when any client views the tab — herdr's per-pane status
    /// subscription polling is what catches that.
    Done,
    #[default]
    Unknown,
}

impl AgentStatus {
    /// Sort key: things that need the user first.
    pub fn attention_rank(self) -> u8 {
        match self {
            AgentStatus::Blocked => 0,
            AgentStatus::Done => 1,
            AgentStatus::Working => 2,
            AgentStatus::Idle => 3,
            AgentStatus::Unknown => 4,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
        }
    }
}

/// `session.snapshot` result: `{"type":"...","snapshot":{...}}`.
#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotResult {
    #[serde(rename = "type", default)]
    pub kind: String,
    pub snapshot: Snapshot,
}

/// Full session state. ~7 KB / ~1 ms on a live server, so re-reading it is the
/// cheap, canonical way to resync; events are only invalidation signals.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub protocol: u64,
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub agent_status: Option<AgentStatus>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub tab_count: Option<u64>,
    #[serde(default)]
    pub pane_count: Option<u64>,
    #[serde(default)]
    pub worktree: Option<Worktree>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Worktree {
    #[serde(default)]
    pub repo_name: Option<String>,
    #[serde(default)]
    pub checkout_path: Option<String>,
    #[serde(default)]
    pub is_linked_worktree: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub agent_status: Option<AgentStatus>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub pane_count: Option<u64>,
}

/// Pane record. Only the identity fields are guaranteed; agent fields appear
/// once herdr detects an agent in the pane.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub tab_id: String,
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub agent_status: Option<AgentStatus>,
    #[serde(default)]
    pub focused: bool,
}

/// Agent record: a pane where herdr detected (or was told about) an agent.
///
/// Guaranteed fields per live observation: `terminal_id`, `agent_status`,
/// `workspace_id`, `tab_id`, `pane_id`, `focused`, `revision`,
/// `state_change_seq`. Everything else is omitted when empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub pane_id: String,
    #[serde(default)]
    pub terminal_id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub tab_id: String,
    #[serde(default)]
    pub agent_status: AgentStatus,
    /// Agent kind, e.g. "claude", "codex".
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub revision: u64,
    /// Server-global counter, bumped on every agent state change; resets on
    /// server restart.
    #[serde(default)]
    pub state_change_seq: u64,
    /// `state_change_seq` at this agent's last "finished work" transition.
    /// `(terminal_id, completion_seq)` dedups "finished" notifications.
    #[serde(default)]
    pub completion_seq: Option<u64>,
    /// Shapes not pinned down yet — carried opaquely.
    #[serde(default)]
    pub tokens: Option<serde_json::Value>,
    #[serde(default)]
    pub agent_session: Option<serde_json::Value>,
    #[serde(default)]
    pub state_labels: Option<serde_json::Value>,
}

impl Agent {
    /// Best human label for this agent.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .or(self.display_agent.as_deref())
            .or(self.agent.as_deref())
            .unwrap_or("agent")
    }

    /// Best one-line description of what it's doing.
    pub fn title_line(&self) -> Option<&str> {
        self.title
            .as_deref()
            .or(self.terminal_title_stripped.as_deref())
            .or(self.terminal_title.as_deref())
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// One pushed line on an `events.subscribe` connection (after the ack).
///
/// Two shapes arrive: lifecycle events use snake_case names
/// (`{"event":"pane_created","data":{"type":"pane_created","pane":{...}}}`)
/// and per-pane subscription events use dotted names
/// (`{"event":"pane.agent_status_changed","data":{...}}`).
#[derive(Debug, Clone, Deserialize)]
pub struct EventLine {
    pub event: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

/// Payload of `pane.agent_status_changed`. Fires on title/label-only changes
/// too, so consumers must de-duplicate.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentStatusChanged {
    pub pane_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// An agent's status (or title) changed — patch the store immediately.
    Status,
    /// The pane set changed — the status subscription is stale; resubscribe
    /// and re-snapshot.
    Topology,
    /// Anything else — treat as an invalidation signal.
    Other,
}

/// Classify an event name, accepting both the snake_case lifecycle spelling
/// and the dotted subscription spelling.
pub fn classify_event(name: &str) -> EventKind {
    let normalized = name.replace('.', "_");
    match normalized.as_str() {
        "pane_agent_status_changed" => EventKind::Status,
        "pane_created" | "pane_closed" | "pane_moved" | "pane_exited" | "pane_agent_detected" => {
            EventKind::Topology
        }
        _ => EventKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_both_spellings() {
        assert_eq!(classify_event("pane.agent_status_changed"), EventKind::Status);
        assert_eq!(classify_event("pane_created"), EventKind::Topology);
        assert_eq!(classify_event("pane.moved"), EventKind::Topology);
        assert_eq!(classify_event("workspace_renamed"), EventKind::Other);
    }

    #[test]
    fn status_roundtrip() {
        for s in ["idle", "working", "blocked", "done", "unknown"] {
            let parsed: AgentStatus = serde_json::from_value(serde_json::json!(s)).unwrap();
            assert_eq!(parsed.as_str(), s);
        }
    }
}
