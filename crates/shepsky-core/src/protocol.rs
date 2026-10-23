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

pub const PROTO_VERSION: u32 = 2;

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
    /// Start `chat` pushes: the agent's conversation, read from its own
    /// transcript (the latest page first, then what's new).
    WatchChat { pane_id: String },
    UnwatchChat { pane_id: String },
    /// One more page of older items for a watched chat.
    ChatOlder { pane_id: String },
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
    /// Scroll herdr's view of an agent's pane: `lines` up (+) or down (-),
    /// or back to the bottom. The scroll is herdr's, so the Mac sees it too.
    ScrollPane {
        pane_id: String,
        #[serde(default)]
        lines: i64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        to_bottom: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    /// Rename an agent: its herdr agent name and its pane's label.
    RenameAgent {
        pane_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    /// Close the pane an agent runs in, ending the agent. Only panes that
    /// host an agent; never a plain shell.
    CloseAgent {
        pane_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    /// Start an agent: in a new tab of `workspace_id`, or else in a new
    /// workspace at `cwd` (named `label`, default: the directory's name). The
    /// `Ack` carries the new agent's `pane_id`.
    StartAgent {
        /// One of `Hello::start_kinds`.
        kind: String,
        /// herdr agent name; the bridge picks a free one (`claude-2`…) if absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        /// A directory under the Mac's home; `~/…` works.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
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
        /// herdr's configured theme, so the phone can match it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        herdr_theme: Option<HerdrTheme>,
        /// Agent kinds `StartAgent` accepts (empty: starting isn't offered).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        start_kinds: Vec<String>,
    },
    /// Full state; replaces everything the client holds.
    State { state: StateView },
    Notify { note: NoteView },
    TerminalFrame {
        pane_id: String,
        seq: u64,
        frame: Frame,
        /// Where herdr's view of the pane is scrolled (absent: unknown).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scroll: Option<PaneScroll>,
    },
    /// A terminal watch hit an error (pane closing, read failed…).
    WatchError { pane_id: String, message: String },
    /// Conversation items for a watched chat. `reset` replaces what the
    /// phone holds, `append` adds new items or replaces ones with the same
    /// id (a tool finishing), `older` goes in front.
    Chat {
        pane_id: String,
        mode: ChatMode,
        items: Vec<ChatItem>,
        /// More items exist before the oldest one sent so far.
        #[serde(default)]
        has_older: bool,
        /// Why there's no conversation to show (e.g. the agent never
        /// reported its session); only with `reset` and no items.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unavailable: Option<String>,
    },
    Ack {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
        /// For `StartAgent`: the new agent's pane.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane_id: Option<String>,
    },
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<String>,
    },
    Pong,
}

/// herdr's theme (`[theme]` in herdr's config.toml), by canonical name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HerdrTheme {
    /// e.g. "rose-pine". One of herdr's built-in themes.
    pub name: String,
    /// With herdr's `auto_switch`: the themes for dark and light appearance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
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
    /// Where a new workspace might start: the directories your workspaces
    /// and agents are in, most-used first, with `~` for home.
    #[serde(default)]
    pub places: Vec<String>,
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
    /// herdr's counter at this agent's last state change: bigger = more
    /// recently active. Only state changes count (herdr has no output
    /// events), so an agent busy printing without changing state doesn't
    /// move; it resets when herdr restarts.
    #[serde(default)]
    pub state_change_seq: u64,
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
        let home = std::env::var("HOME").ok();
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
                cwd: a.cwd.as_deref().map(|cwd| tildify(cwd, home.as_deref())),
                focused: a.focused,
                state_change_seq: a.state_change_seq,
            })
            .collect();
        let mut uses: std::collections::BTreeMap<String, usize> = Default::default();
        let worktrees = store.workspaces.values().filter_map(|w| w.worktree.as_ref()?.checkout_path.as_deref());
        let agent_dirs = store.agents.values().filter_map(|a| a.cwd.as_deref());
        for dir in worktrees.chain(agent_dirs) {
            *uses.entry(tildify(dir, home.as_deref())).or_default() += 1;
        }
        let mut places: Vec<(String, usize)> = uses.into_iter().collect();
        places.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        StateView {
            herdr_version: store.herdr_version.clone(),
            herdr_connected,
            workspaces,
            agents,
            places: places.into_iter().map(|(dir, _)| dir).collect(),
        }
    }
}

/// `/Users/me/src/app` -> `~/src/app`: shorter on a phone screen, and keeps
/// the Mac's user name out of screenshots.
fn tildify(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty() && *h != "/") {
        Some(home) if path == home => "~".to_string(),
        Some(home) => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        },
        None => path.to_string(),
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
    /// Foreground (None = the theme's default foreground).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<Color>,
    /// Background (None = the theme's default background).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<Color>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub b: bool, // bold
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub i: bool, // italic
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub u: bool, // underline
    /// Reverse video: the phone swaps fg and bg after resolving them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub r: bool,
}

/// How far herdr's view of a pane is scrolled back, in lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneScroll {
    pub offset_from_bottom: u64,
    pub max_offset_from_bottom: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMode {
    Reset,
    Append,
    Older,
}

/// One entry of an agent's conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatItem {
    /// Position in the transcript: later items have bigger ids. An item sent
    /// again with the same id replaces the old one.
    pub id: u64,
    #[serde(flatten)]
    pub body: ChatBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatBody {
    /// Something you (or another session) sent the agent.
    Prompt { text: String },
    /// The agent talking: markdown-ish text.
    Reply { text: String },
    /// The agent thinking; `text` is empty when the agent keeps it private.
    Thinking {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
    },
    /// A tool call: `summary` is one line (`$ npm test`, a file path…),
    /// `detail` the full input, `output` what came back (both trimmed).
    Tool {
        name: String,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        status: ToolStatus,
    },
    /// The agent asking you to choose (Claude's AskUserQuestion).
    Question {
        questions: Vec<QuestionView>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answer: Option<String>,
    },
    /// Context compacted, turn interrupted, a slash command…
    Notice { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Running,
    Ok,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestionView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi: bool,
}

/// A terminal color: one of the 16 theme colors, or an exact RGB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Color {
    /// ANSI 0–15, drawn in the phone's theme (JSON: a number).
    Ansi(u8),
    /// `#rrggbb`: 256-color cube, grayscale ramp and truecolor.
    Rgb(String),
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
    fn places_are_most_used_first() {
        let agent = |pane: &str, cwd: &str| Agent {
            pane_id: pane.into(),
            workspace_id: "w1".into(),
            cwd: Some(cwd.into()),
            ..Agent::default()
        };
        let snapshot = Snapshot {
            workspaces: vec![Workspace {
                workspace_id: "w1".into(),
                worktree: Some(crate::herdr::Worktree {
                    checkout_path: Some("/srv/b".into()),
                    ..Default::default()
                }),
                ..Workspace::default()
            }],
            agents: vec![agent("w1:p1", "/srv/a"), agent("w1:p2", "/srv/b"), agent("w1:p3", "/srv/c")],
            ..Snapshot::default()
        };
        let state = StateView::project(&Store::from_snapshot(snapshot), true);
        assert_eq!(state.places, ["/srv/b", "/srv/a", "/srv/c"]);
    }

    #[test]
    fn tildify_only_whole_home_prefix() {
        let home = Some("/Users/me");
        assert_eq!(tildify("/Users/me", home), "~");
        assert_eq!(tildify("/Users/me/src/app", home), "~/src/app");
        assert_eq!(tildify("/Users/meow/src", home), "/Users/meow/src");
        assert_eq!(tildify("/opt/x", home), "/opt/x");
        assert_eq!(tildify("/opt/x", None), "/opt/x");
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

        // Theme colors are numbers, exact colors strings.
        let span = Span {
            t: "x".into(),
            fg: Some(Color::Ansi(1)),
            bg: Some(Color::Rgb("#0a141e".into())),
            r: true,
            ..Span::default()
        };
        assert_eq!(
            serde_json::to_string(&span).unwrap(),
            r##"{"t":"x","fg":1,"bg":"#0a141e","r":true}"##
        );

        let start: ClientMsg = serde_json::from_str(
            r#"{"type":"start_agent","kind":"claude","cwd":"~/src/app","op_id":"2"}"#,
        )
        .unwrap();
        assert!(matches!(start, ClientMsg::StartAgent { ref kind, workspace_id: None, .. } if kind == "claude"));
        let rename: ClientMsg = serde_json::from_str(r#"{"type":"rename_agent","pane_id":"w1:p1","name":"fixer","op_id":"3"}"#).unwrap();
        assert!(matches!(rename, ClientMsg::RenameAgent { ref name, .. } if name == "fixer"));
        let close: ClientMsg = serde_json::from_str(r#"{"type":"close_agent","pane_id":"w1:p1"}"#).unwrap();
        assert!(matches!(close, ClientMsg::CloseAgent { op_id: None, .. }));
        let ack = ServerMsg::Ack { op_id: Some("2".into()), pane_id: Some("w3:p1".into()) };
        assert_eq!(serde_json::to_string(&ack).unwrap(), r#"{"type":"ack","op_id":"2","pane_id":"w3:p1"}"#);

        let chat = ServerMsg::Chat {
            pane_id: "w1:p1".into(),
            mode: ChatMode::Append,
            items: vec![ChatItem {
                id: 7,
                body: ChatBody::Tool {
                    name: "Bash".into(),
                    summary: "$ ls".into(),
                    detail: None,
                    output: None,
                    status: ToolStatus::Running,
                },
            }],
            has_older: true,
            unavailable: None,
        };
        assert_eq!(
            serde_json::to_string(&chat).unwrap(),
            r#"{"type":"chat","pane_id":"w1:p1","mode":"append","items":[{"id":7,"kind":"tool","name":"Bash","summary":"$ ls","status":"running"}],"has_older":true}"#
        );

        // Unknown fields must not break parsing (forward compat).
        let msg: ServerMsg =
            serde_json::from_str(r#"{"type":"pong","future_field":42}"#).unwrap();
        assert!(matches!(msg, ServerMsg::Pong));
    }
}
