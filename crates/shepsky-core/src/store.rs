//! The session store: shepsky's cached view of herdr state.
//!
//! Model: a full `session.snapshot` is truth and replaces everything; events
//! are low-latency patches / invalidation signals in between. Both the bridge
//! and the phone runtime hold one of these.

use std::collections::{BTreeMap, BTreeSet};

use crate::herdr::{Agent, AgentStatus, AgentStatusChanged, Snapshot, Tab, Workspace};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Store {
    pub herdr_version: String,
    pub protocol: u64,
    pub focused_workspace_id: Option<String>,
    pub focused_tab_id: Option<String>,
    pub focused_pane_id: Option<String>,
    pub workspaces: BTreeMap<String, Workspace>,
    pub tabs: BTreeMap<String, Tab>,
    /// Every live pane id — this is what the status subscription must cover.
    pub pane_ids: BTreeSet<String>,
    /// Agents keyed by pane id.
    pub agents: BTreeMap<String, Agent>,
}

/// Result of applying a status-changed event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Patch {
    /// Applied and something actually changed.
    Changed,
    /// Applied but nothing differed (the event also fires for label churn).
    Unchanged,
    /// We don't know this pane — caller should re-snapshot.
    UnknownPane,
}

impl Store {
    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        let mut store = Store::default();
        store.apply_snapshot(snapshot);
        store
    }

    /// Replace the whole store with a fresh snapshot.
    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.herdr_version = snapshot.version;
        self.protocol = snapshot.protocol;
        self.focused_workspace_id = snapshot.focused_workspace_id;
        self.focused_tab_id = snapshot.focused_tab_id;
        self.focused_pane_id = snapshot.focused_pane_id;
        self.workspaces = snapshot
            .workspaces
            .into_iter()
            .map(|w| (w.workspace_id.clone(), w))
            .collect();
        self.tabs = snapshot
            .tabs
            .into_iter()
            .map(|t| (t.tab_id.clone(), t))
            .collect();
        self.pane_ids = snapshot.panes.iter().map(|p| p.pane_id.clone()).collect();
        self.agents = snapshot
            .agents
            .into_iter()
            .map(|a| (a.pane_id.clone(), a))
            .collect();
    }

    /// Low-latency patch from a `pane.agent_status_changed` event. The next
    /// snapshot remains the source of truth.
    pub fn patch_agent_status(&mut self, ev: &AgentStatusChanged) -> Patch {
        let Some(agent) = self.agents.get_mut(&ev.pane_id) else {
            // Either a pane we've never seen or one without an agent record
            // yet (fresh detection) — both mean our view is stale.
            return Patch::UnknownPane;
        };
        let mut changed = false;
        if agent.agent_status != ev.agent_status {
            agent.agent_status = ev.agent_status;
            changed = true;
        }
        if ev.title.is_some() && agent.title != ev.title {
            agent.title = ev.title.clone();
            changed = true;
        }
        if ev.agent.is_some() && agent.agent != ev.agent {
            agent.agent = ev.agent.clone();
            changed = true;
        }
        if ev.display_agent.is_some() && agent.display_agent != ev.display_agent {
            agent.display_agent = ev.display_agent.clone();
            changed = true;
        }
        if changed { Patch::Changed } else { Patch::Unchanged }
    }

    /// Agents sorted for display: needs-attention first, then by workspace
    /// number, then pane id for stability.
    pub fn agents_sorted(&self) -> Vec<&Agent> {
        let mut agents: Vec<&Agent> = self.agents.values().collect();
        agents.sort_by_key(|a| {
            let ws_number = self
                .workspaces
                .get(&a.workspace_id)
                .and_then(|w| w.number)
                .unwrap_or(u64::MAX);
            (a.agent_status.attention_rank(), ws_number, a.pane_id.clone())
        });
        agents
    }

    pub fn workspace_label(&self, workspace_id: &str) -> Option<&str> {
        self.workspaces
            .get(workspace_id)
            .and_then(|w| w.label.as_deref())
    }

    /// Count of agents per status, for summary lines.
    pub fn status_counts(&self) -> BTreeMap<AgentStatus, usize> {
        let mut counts = BTreeMap::new();
        for agent in self.agents.values() {
            *counts.entry(agent.agent_status).or_insert(0) += 1;
        }
        counts
    }
}

impl std::cmp::Ord for AgentStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.attention_rank().cmp(&other.attention_rank())
    }
}

impl std::cmp::PartialOrd for AgentStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::Pane;

    fn agent(pane: &str, status: AgentStatus) -> Agent {
        Agent {
            pane_id: pane.to_string(),
            terminal_id: format!("term_{pane}"),
            workspace_id: "w1".to_string(),
            tab_id: "w1:t1".to_string(),
            agent_status: status,
            agent: Some("claude".to_string()),
            ..Agent::default()
        }
    }

    fn snapshot_with(agents: Vec<Agent>) -> Snapshot {
        Snapshot {
            version: "0.9.5".to_string(),
            protocol: 22,
            panes: agents
                .iter()
                .map(|a| Pane {
                    pane_id: a.pane_id.clone(),
                    workspace_id: a.workspace_id.clone(),
                    tab_id: a.tab_id.clone(),
                    ..Pane::default()
                })
                .collect(),
            agents,
            ..Snapshot::default()
        }
    }

    #[test]
    fn snapshot_replaces_everything() {
        let mut store = Store::from_snapshot(snapshot_with(vec![
            agent("w1:p1", AgentStatus::Working),
            agent("w1:p2", AgentStatus::Idle),
        ]));
        assert_eq!(store.agents.len(), 2);
        assert_eq!(store.pane_ids.len(), 2);

        store.apply_snapshot(snapshot_with(vec![agent("w1:p3", AgentStatus::Blocked)]));
        assert_eq!(store.agents.len(), 1);
        assert!(store.agents.contains_key("w1:p3"));
        assert_eq!(store.pane_ids.iter().collect::<Vec<_>>(), vec!["w1:p3"]);
    }

    #[test]
    fn patch_updates_status_and_detects_noops() {
        let mut store = Store::from_snapshot(snapshot_with(vec![agent("w1:p1", AgentStatus::Working)]));
        let ev = AgentStatusChanged {
            pane_id: "w1:p1".to_string(),
            workspace_id: None,
            agent_status: AgentStatus::Blocked,
            agent: None,
            display_agent: None,
            title: None,
        };
        assert_eq!(store.patch_agent_status(&ev), Patch::Changed);
        assert_eq!(store.agents["w1:p1"].agent_status, AgentStatus::Blocked);
        assert_eq!(store.patch_agent_status(&ev), Patch::Unchanged);

        let unknown = AgentStatusChanged { pane_id: "w9:p9".to_string(), ..ev };
        assert_eq!(store.patch_agent_status(&unknown), Patch::UnknownPane);
    }

    #[test]
    fn sort_puts_attention_first() {
        let store = Store::from_snapshot(snapshot_with(vec![
            agent("w1:p1", AgentStatus::Idle),
            agent("w1:p2", AgentStatus::Blocked),
            agent("w1:p3", AgentStatus::Done),
            agent("w1:p4", AgentStatus::Working),
        ]));
        let order: Vec<&str> = store
            .agents_sorted()
            .iter()
            .map(|a| a.pane_id.as_str())
            .collect();
        assert_eq!(order, vec!["w1:p2", "w1:p3", "w1:p4", "w1:p1"]);
    }
}
