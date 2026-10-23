//! Parse a captured live `session.snapshot` RPC line (herdr 0.9.5) end to end.

use shepsky_core::herdr::{AgentStatus, Response, SnapshotResult};
use shepsky_core::store::Store;

#[test]
fn parses_live_snapshot_fixture() {
    let raw = include_str!("fixtures/snapshot_rpc.json");
    let response: Response = serde_json::from_str(raw).expect("parse RPC line");
    assert!(response.error.is_none());

    let result: SnapshotResult =
        serde_json::from_value(response.result.expect("has result")).expect("parse snapshot");
    let snapshot = result.snapshot;

    assert_eq!(snapshot.version, "0.9.5");
    assert_eq!(snapshot.protocol, 22);
    assert!(!snapshot.workspaces.is_empty());
    assert!(!snapshot.panes.is_empty());
    assert!(!snapshot.agents.is_empty());

    for agent in &snapshot.agents {
        assert!(!agent.pane_id.is_empty());
        assert!(!agent.terminal_id.is_empty());
        assert!(!agent.workspace_id.is_empty());
        assert_ne!(agent.agent_status, AgentStatus::Unknown);
        assert!(!agent.label().is_empty());
    }

    let n_agents = snapshot.agents.len();
    let n_panes = snapshot.panes.len();
    let n_workspaces = snapshot.workspaces.len();

    let store = Store::from_snapshot(snapshot);
    assert_eq!(store.agents.len(), n_agents);
    assert_eq!(store.pane_ids.len(), n_panes);
    assert_eq!(store.workspaces.len(), n_workspaces);
    assert_eq!(store.agents_sorted().len(), n_agents);

    // Every agent's workspace resolves to a label in this fixture.
    for agent in store.agents.values() {
        assert!(store.workspace_label(&agent.workspace_id).is_some());
    }
}
