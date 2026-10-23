//! Turning store updates into user-facing notifications, with dedup.
//!
//! Pure state machine: feed it each new `Store`, get back the notifications
//! that should fire. Used by the bridge (push to phones) and by the Android
//! runtime (post local notifications) so both sides dedup identically.

use std::collections::{BTreeMap, BTreeSet};

use crate::herdr::AgentStatus;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Agent is waiting on the user (approval / question / permission UI).
    NeedsInput,
    /// Agent finished work that nobody has looked at yet.
    Finished,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub kind: Kind,
    pub pane_id: String,
    pub terminal_id: String,
    pub agent_label: String,
    pub title: Option<String>,
    pub workspace_label: Option<String>,
}

/// Per-terminal memory of what we've already notified about.
#[derive(Debug, Default)]
pub struct NotifyState {
    last_status: BTreeMap<String, AgentStatus>,
    /// `(terminal_id, completion_seq)` pairs already announced as Finished.
    seen_completions: BTreeSet<(String, u64)>,
}

impl NotifyState {
    /// Observe a new store; return the notifications to fire.
    ///
    /// `mutes` holds lowercased agent labels to silence.
    pub fn update(&mut self, store: &Store, mutes: &BTreeSet<String>) -> Vec<Notification> {
        let mut out = Vec::new();
        let mut live_terminals = BTreeSet::new();

        for agent in store.agents.values() {
            if agent.terminal_id.is_empty() {
                continue;
            }
            live_terminals.insert(agent.terminal_id.clone());
            let prev = self.last_status.get(&agent.terminal_id).copied();
            let status = agent.agent_status;

            let muted = mutes.contains(&agent.label().to_lowercase());

            match status {
                AgentStatus::Blocked if prev != Some(AgentStatus::Blocked) => {
                    if !muted {
                        out.push(self.notification(store, agent, Kind::NeedsInput));
                    }
                }
                AgentStatus::Done if prev != Some(AgentStatus::Done) => {
                    // Dedup via completion_seq when present: a `done` agent can
                    // bounce done -> idle -> done across resyncs without new
                    // work; the seq only moves on a real completion.
                    let fresh = match agent.completion_seq {
                        Some(seq) => self
                            .seen_completions
                            .insert((agent.terminal_id.clone(), seq)),
                        None => true,
                    };
                    if fresh && !muted {
                        out.push(self.notification(store, agent, Kind::Finished));
                    }
                }
                _ => {}
            }
            self.last_status.insert(agent.terminal_id.clone(), status);
        }

        // Drop memory of terminals that no longer exist.
        self.last_status.retain(|t, _| live_terminals.contains(t));
        self.seen_completions
            .retain(|(t, _)| live_terminals.contains(t));

        out
    }

    fn notification(
        &self,
        store: &Store,
        agent: &crate::herdr::Agent,
        kind: Kind,
    ) -> Notification {
        Notification {
            kind,
            pane_id: agent.pane_id.clone(),
            terminal_id: agent.terminal_id.clone(),
            agent_label: agent.label().to_string(),
            title: agent.title_line().map(str::to_string),
            workspace_label: store
                .workspace_label(&agent.workspace_id)
                .map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::{Agent, Pane, Snapshot};

    fn store_with(statuses: &[(&str, AgentStatus, Option<u64>)]) -> Store {
        let agents: Vec<Agent> = statuses
            .iter()
            .map(|(pane, status, completion_seq)| Agent {
                pane_id: pane.to_string(),
                terminal_id: format!("term_{pane}"),
                workspace_id: "w1".to_string(),
                tab_id: "w1:t1".to_string(),
                agent_status: *status,
                agent: Some("claude".to_string()),
                completion_seq: *completion_seq,
                ..Agent::default()
            })
            .collect();
        let panes = agents
            .iter()
            .map(|a| Pane {
                pane_id: a.pane_id.clone(),
                ..Pane::default()
            })
            .collect();
        Store::from_snapshot(Snapshot {
            agents,
            panes,
            ..Snapshot::default()
        })
    }

    #[test]
    fn blocked_fires_once_per_transition() {
        let mut state = NotifyState::default();
        let mutes = BTreeSet::new();

        let working = store_with(&[("p1", AgentStatus::Working, None)]);
        assert!(state.update(&working, &mutes).is_empty());

        let blocked = store_with(&[("p1", AgentStatus::Blocked, None)]);
        let notes = state.update(&blocked, &mutes);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].kind, Kind::NeedsInput);

        // Same store again (e.g. the 5s safety-net resync): no repeat.
        assert!(state.update(&blocked, &mutes).is_empty());

        // Unblock then block again: fires again.
        assert!(state.update(&working, &mutes).is_empty());
        assert_eq!(state.update(&blocked, &mutes).len(), 1);
    }

    #[test]
    fn done_dedups_by_completion_seq() {
        let mut state = NotifyState::default();
        let mutes = BTreeSet::new();

        state.update(&store_with(&[("p1", AgentStatus::Working, None)]), &mutes);
        let done = store_with(&[("p1", AgentStatus::Done, Some(41))]);
        assert_eq!(state.update(&done, &mutes).len(), 1);

        // done -> idle (someone glanced at it) -> done with the SAME seq:
        // not new work, no notification.
        state.update(&store_with(&[("p1", AgentStatus::Idle, Some(41))]), &mutes);
        assert!(state.update(&done, &mutes).is_empty());

        // A new completion seq is real new work.
        state.update(&store_with(&[("p1", AgentStatus::Working, Some(41))]), &mutes);
        let done_again = store_with(&[("p1", AgentStatus::Done, Some(55))]);
        assert_eq!(state.update(&done_again, &mutes).len(), 1);
    }

    #[test]
    fn first_observation_of_blocked_fires() {
        // Bridge restart while an agent is already blocked: the user still
        // wants to hear about it.
        let mut state = NotifyState::default();
        let notes = state.update(
            &store_with(&[("p1", AgentStatus::Blocked, None)]),
            &BTreeSet::new(),
        );
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn mutes_silence_by_label() {
        let mut state = NotifyState::default();
        let mutes: BTreeSet<String> = ["claude".to_string()].into();
        let notes = state.update(&store_with(&[("p1", AgentStatus::Blocked, None)]), &mutes);
        assert!(notes.is_empty());
    }

    #[test]
    fn vanished_terminals_are_forgotten() {
        let mut state = NotifyState::default();
        let mutes = BTreeSet::new();
        state.update(&store_with(&[("p1", AgentStatus::Blocked, None)]), &mutes);
        // Pane goes away entirely...
        state.update(&store_with(&[]), &mutes);
        // ...and comes back blocked (new herdr session reusing ids): fires.
        let notes = state.update(&store_with(&[("p1", AgentStatus::Blocked, None)]), &mutes);
        assert_eq!(notes.len(), 1);
    }
}
