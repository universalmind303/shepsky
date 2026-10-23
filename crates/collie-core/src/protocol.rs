//! The bridge <-> phone protocol: versioned JSON over one WebSocket.
//!
//! Documented in `docs/PROTOCOL.md`; the Kotlin app mirrors these shapes with
//! kotlinx.serialization. Both sides ignore unknown fields. `Hello.proto`
//! carries the protocol version (currently 1).
//!
//! Model: the bridge pushes the **full** `State` on every change (it's tiny —
//! a handful of KB) instead of deltas. Reconnect = auth, then the next `State`
//! replaces everything. Notifications and terminal frames are push-only
//! streams on the side.

use serde::{Deserialize, Serialize};

use crate::herdr::AgentStatus;
use crate::notify::{Kind, Notification};
use crate::store::Store;

pub const PROTO_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// phone -> bridge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Must be the first message on the socket.
    Auth { token: String },
    /// Start receiving `TerminalFrame`s for a pane.
    WatchPane { pane_id: String },
    UnwatchPane { pane_id: String },
    /// Submit a prompt to the agent in `pane_id`.
    Prompt {
        pane_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    /// Send key presses (e.g. `["1"]`, `["enter"]`, `["esc"]`) to the agent.
    SendKeys {
        pane_id: String,
        keys: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    /// Mark an agent as looked-at (clears `done`).
    FocusAgent {
        pane_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    Ping,
}

// ---------------------------------------------------------------------------
// bridge -> phone
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// First reply after a successful `Auth`.
    Hello {
        proto: u32,
        bridge_version: String,
        herdr_version: String,
    },
    /// Full state; replaces everything the client holds.
    State { state: StateView },
    Notify { note: NoteView },
    TerminalFrame {
        pane_id: String,
        seq: u64,
        frame: Frame,
    },
    /// A terminal watch hit an error (pane closing, read failed…).
    WatchError { pane_id: String, message: String },
    Ack {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    Pong,
}

/// Phone-facing projection of the session: only what the UI renders.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StateView {
    pub herdr_version: String,
    /// False while the bridge is reconnecting to herdr.
    pub herdr_connected: bool,
    pub workspaces: Vec<WorkspaceView>,
    /// Sorted needs-attention first (same order the UI shows).
    pub agents: Vec<AgentView>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceView {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub status: Option<AgentStatus>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentView {
    pub pane_id: String,
    pub terminal_id: String,
    pub status: AgentStatus,
    /// Best human label ("claude", a custom name…).
    pub label: String,
    /// Agent kind when known ("claude", "codex"…).
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub workspace_id: String,
    #[serde(default)]
    pub workspace_label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteView {
    pub kind: NoteKind,
    pub pane_id: String,
    pub agent_label: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub workspace_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    NeedsInput,
    Finished,
}

impl From<&Notification> for NoteView {
    fn from(note: &Notification) -> Self {
        NoteView {
            kind: match note.kind {
                Kind::NeedsInput => NoteKind::NeedsInput,
                Kind::Finished => NoteKind::Finished,
            },
            pane_id: note.pane_id.clone(),
            agent_label: note.agent_label.clone(),
            title: note.title.clone(),
            workspace_label: note.workspace_label.clone(),
        }
    }
}

impl StateView {
    pub fn project(store: &Store, herdr_connected: bool) -> Self {
        let workspaces = store
            .workspaces
            .values()
            .map(|w| WorkspaceView {
                workspace_id: w.workspace_id.clone(),
                label: w.label.clone(),
                number: w.number,
                status: w.agent_status,
                focused: w.focused,
            })
            .collect();
        let agents = store
            .agents_sorted()
            .into_iter()
            .map(|a| AgentView {
                pane_id: a.pane_id.clone(),
                terminal_id: a.terminal_id.clone(),
                status: a.agent_status,
                label: a.label().to_string(),
                kind: a.agent.clone(),
                title: a.title_line().map(str::to_string),
                workspace_id: a.workspace_id.clone(),
                workspace_label: store
                    .workspace_label(&a.workspace_id)
                    .map(str::to_string),
                cwd: a.cwd.clone(),
                focused: a.focused,
            })
            .collect();
        StateView {
            herdr_version: store.herdr_version.clone(),
            herdr_connected,
            workspaces,
            agents,
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal frames
// ---------------------------------------------------------------------------

/// One rendered terminal screen: rows of styled spans, already trimmed of
/// trailing blanks. The phone renders this as text — no emulator needed there.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub lines: Vec<Vec<Span>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Text.
    pub t: String,
    /// Foreground color as `#rrggbb` (None = terminal default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    /// Background color as `#rrggbb` (None = terminal default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub b: bool, // bold
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub i: bool, // italic
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub u: bool, // underline
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::{Agent, Pane, Snapshot, Workspace};

    #[test]
    fn projection_carries_labels_and_order() {
        let snapshot = Snapshot {
            version: "0.9.5".into(),
            workspaces: vec![Workspace {
                workspace_id: "w1".into(),
                label: Some("myrepo".into()),
                number: Some(1),
                ..Workspace::default()
            }],
            panes: vec![
                Pane { pane_id: "w1:p1".into(), ..Pane::default() },
                Pane { pane_id: "w1:p2".into(), ..Pane::default() },
            ],
            agents: vec![
                Agent {
                    pane_id: "w1:p1".into(),
                    terminal_id: "term_a".into(),
                    workspace_id: "w1".into(),
                    agent: Some("claude".into()),
                    agent_status: AgentStatus::Idle,
                    ..Agent::default()
                },
                Agent {
                    pane_id: "w1:p2".into(),
                    terminal_id: "term_b".into(),
                    workspace_id: "w1".into(),
                    agent: Some("codex".into()),
                    agent_status: AgentStatus::Blocked,
                    ..Agent::default()
                },
            ],
            ..Snapshot::default()
        };
        let state = StateView::project(&Store::from_snapshot(snapshot), true);
        assert_eq!(state.agents.len(), 2);
        // Blocked first.
        assert_eq!(state.agents[0].pane_id, "w1:p2");
        assert_eq!(state.agents[0].workspace_label.as_deref(), Some("myrepo"));
        assert_eq!(state.agents[1].label, "claude");
    }

    #[test]
    fn wire_shapes_are_stable() {
        // These exact strings are what the Kotlin side parses — changing them
        // is a protocol break and must bump PROTO_VERSION.
        let auth: ClientMsg = serde_json::from_str(r#"{"type":"auth","token":"t"}"#).unwrap();
        assert!(matches!(auth, ClientMsg::Auth { .. }));

        let prompt = ClientMsg::Prompt {
            pane_id: "w1:p1".into(),
            text: "hi".into(),
            op_id: Some("1".into()),
        };
        assert_eq!(
            serde_json::to_string(&prompt).unwrap(),
            r#"{"type":"prompt","pane_id":"w1:p1","text":"hi","op_id":"1"}"#
        );

        let note = ServerMsg::Notify {
            note: NoteView {
                kind: NoteKind::NeedsInput,
                pane_id: "w1:p1".into(),
                agent_label: "claude".into(),
                title: None,
                workspace_label: None,
            },
        };
        let json = serde_json::to_string(&note).unwrap();
        assert!(json.contains(r#""type":"notify""#));
        assert!(json.contains(r#""kind":"needs_input""#));

        // Unknown fields must not break parsing (forward compat).
        let msg: ServerMsg =
            serde_json::from_str(r#"{"type":"pong","future_field":42}"#).unwrap();
        assert!(matches!(msg, ServerMsg::Pong));
    }
}
