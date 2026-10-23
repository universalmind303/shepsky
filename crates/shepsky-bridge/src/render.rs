//! Terminal rendering for `shepsky-bridge status [--watch]`.

use std::time::Duration;

use shepsky_core::herdr::AgentStatus;
use shepsky_core::notify::{Kind, Notification};

use crate::sync::UiState;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";

fn status_cell(status: AgentStatus) -> String {
    match status {
        AgentStatus::Blocked => format!("{BOLD}{RED}● blocked{RESET}"),
        AgentStatus::Done => format!("{GREEN}● done{RESET}   "),
        AgentStatus::Working => format!("{YELLOW}● working{RESET}"),
        AgentStatus::Idle => format!("{DIM}● idle{RESET}   "),
        AgentStatus::Unknown => format!("{DIM}● ?{RESET}      "),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", taken.trim_end())
    } else {
        taken
    }
}

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(len)))
}

pub fn render(state: &UiState, since_update: Option<Duration>) -> String {
    let store = &state.store;
    let mut out = String::new();

    // Header.
    let counts = store.status_counts();
    let summary: Vec<String> = counts
        .iter()
        .map(|(status, n)| format!("{n} {}", status.as_str()))
        .collect();
    let connection = if state.connected {
        format!("{GREEN}connected{RESET}")
    } else {
        format!("{BOLD}{RED}reconnecting…{RESET}")
    };
    out.push_str(&format!(
        "{BOLD}shepsky{RESET} · herdr {} · {connection} · {}\n\n",
        if store.herdr_version.is_empty() { "?" } else { &store.herdr_version },
        if summary.is_empty() { "no agents".to_string() } else { summary.join(" · ") },
    ));

    // Agent table.
    if store.agents.is_empty() {
        out.push_str(&format!("{DIM}  (no agents detected in any pane){RESET}\n"));
    } else {
        let agents = store.agents_sorted();
        let label_width = agents
            .iter()
            .map(|a| a.label().chars().count())
            .max()
            .unwrap_or(5)
            .clamp(5, 20);
        let ws_width = agents
            .iter()
            .map(|a| {
                store
                    .workspace_label(&a.workspace_id)
                    .unwrap_or(&a.workspace_id)
                    .chars()
                    .count()
            })
            .max()
            .unwrap_or(9)
            .clamp(9, 24);

        out.push_str(&format!(
            "  {DIM}{} {} {} {}{RESET}\n",
            pad("STATUS", 9),
            pad("AGENT", label_width),
            pad("WORKSPACE", ws_width),
            "TITLE",
        ));
        for agent in agents {
            let workspace = store
                .workspace_label(&agent.workspace_id)
                .unwrap_or(&agent.workspace_id);
            let title = agent.title_line().unwrap_or("");
            out.push_str(&format!(
                "  {} {} {CYAN}{}{RESET} {}\n",
                status_cell(agent.agent_status),
                pad(&truncate(agent.label(), label_width), label_width),
                pad(&truncate(workspace, ws_width), ws_width),
                truncate(title, 64),
            ));
        }
    }

    // Recent notifications.
    if !state.recent_notes.is_empty() {
        out.push('\n');
        out.push_str(&format!("{DIM}  recent:{RESET}\n"));
        for note in state.recent_notes.iter().rev().take(4) {
            out.push_str(&format!("    {}\n", format_note(note)));
        }
    }

    if let Some(elapsed) = since_update {
        out.push_str(&format!(
            "\n{DIM}  updated {}s ago · ctrl-c to quit{RESET}\n",
            elapsed.as_secs()
        ));
    }
    out
}

pub fn format_note(note: &Notification) -> String {
    let (icon, verb) = match note.kind {
        Kind::NeedsInput => (format!("{BOLD}{RED}■{RESET}"), "needs input"),
        Kind::Finished => (format!("{GREEN}■{RESET}"), "finished"),
    };
    let workspace = note.workspace_label.as_deref().unwrap_or("?");
    match note.title.as_deref() {
        Some(title) if !title.is_empty() => format!(
            "{icon} {} {verb} in {workspace} — {}",
            note.agent_label,
            truncate(title, 48)
        ),
        _ => format!("{icon} {} {verb} in {workspace}", note.agent_label),
    }
}
