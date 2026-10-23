//! Starting agents from the phone: a new workspace (or a new tab in an
//! existing one) with an agent in it.
//!
//! This is the one place the phone can make herdr run something new, so the
//! guardrails live here in the bridge, not in the app:
//! - only the kinds in [`KINDS`];
//! - no extra command-line arguments (agents keep asking permission before
//!   they use tools);
//! - the directory must exist inside the Mac's home.
//!
//! With `--demo`, the "agent" is a fake one (scripts/fake-agent.mjs), so the
//! sandboxed demo never starts a real agent or spends tokens.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use shepsky_core::store::Store;

use crate::herdr_client::HerdrClient;

/// Agent kinds the phone may start (herdr's names).
pub const KINDS: &[&str] = &["claude", "codex", "pi"];

/// A new pane's shell takes a moment to come up; herdr only starts an agent
/// at a shell prompt.
const SHELL_WAIT: Duration = Duration::from_secs(8);
const SHELL_POLL: Duration = Duration::from_millis(250);

pub struct Request {
    pub kind: String,
    pub name: Option<String>,
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    pub label: Option<String>,
}

/// A request that passed the guardrails.
#[derive(Debug, PartialEq)]
pub struct Plan {
    kind: String,
    name: String,
    /// We picked the name, so we may pick another if it's taken by the time
    /// herdr starts the agent (two starts at once).
    auto_name: bool,
    target: Target,
}

#[derive(Debug, PartialEq)]
enum Target {
    NewWorkspace { cwd: PathBuf, label: String },
    NewTab { workspace_id: String, cwd: Option<PathBuf> },
}

/// Why a request was turned down: `(code, message)` for the phone.
pub type Refusal = (&'static str, String);

/// Check a request against the guardrails and the current session.
pub fn plan(req: Request, store: &Store, home: &Path) -> Result<Plan, Refusal> {
    if !KINDS.contains(&req.kind.as_str()) {
        return Err(("unsupported_kind", format!("can't start {:?} from the phone", req.kind)));
    }
    let requested = req.name.filter(|n| !n.trim().is_empty());
    let auto_name = requested.is_none();
    let name = match requested {
        Some(name) if valid_name(&name) => name,
        Some(_) => {
            return Err((
                "invalid_agent_name",
                "names are lowercase letters, digits, - or _ (start with a letter, up to 32)".into(),
            ));
        }
        None => free_name(&req.kind, store),
    };
    let cwd = req.cwd.filter(|c| !c.trim().is_empty()).map(|c| resolve_dir(c.trim(), home)).transpose()?;
    let target = match req.workspace_id {
        Some(workspace_id) => {
            if !store.workspaces.contains_key(&workspace_id) {
                return Err(("not_found", format!("no workspace {workspace_id}")));
            }
            Target::NewTab { workspace_id, cwd }
        }
        None => {
            let cwd = cwd.ok_or(("bad_directory", "pick a directory for the new workspace".to_string()))?;
            let label = req
                .label
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            Target::NewWorkspace { cwd, label }
        }
    };
    Ok(Plan { kind: req.kind, name, auto_name, target })
}

/// Carry out a plan; returns the new agent's pane id. If the agent doesn't
/// start, the workspace or tab made for it is closed again.
pub async fn run(client: &HerdrClient, plan: Plan, demo_agent: Option<&str>) -> Result<String> {
    let created = match &plan.target {
        Target::NewWorkspace { cwd, label } => {
            client
                .call("workspace.create", json!({ "cwd": cwd, "label": label, "focus": false }))
                .await?
        }
        Target::NewTab { workspace_id, cwd } => {
            let mut params = json!({ "workspace_id": workspace_id, "focus": false });
            if let Some(cwd) = cwd {
                params["cwd"] = json!(cwd);
            }
            client.call("tab.create", params).await?
        }
    };
    let undo = match &plan.target {
        Target::NewWorkspace { .. } => created
            .pointer("/workspace/workspace_id")
            .map(|id| ("workspace.close", json!({ "workspace_id": id }))),
        Target::NewTab { .. } => created.pointer("/tab/tab_id").map(|id| ("tab.close", json!({ "tab_id": id }))),
    };
    let started = match created.pointer("/root_pane/pane_id").and_then(Value::as_str) {
        Some(pane_id) => start_in(client, plan, pane_id, demo_agent).await.map(|()| pane_id.to_string()),
        None => Err(anyhow::anyhow!("herdr didn't say which pane it created")),
    };
    if started.is_err()
        && let Some((method, params)) = undo
        && let Err(err) = client.call(method, params).await
    {
        tracing::warn!(%err, "couldn't close the workspace/tab of an agent that didn't start");
    }
    started
}

async fn start_in(client: &HerdrClient, plan: Plan, pane_id: &str, demo_agent: Option<&str>) -> Result<()> {
    match demo_agent {
        // The demo: a fake agent typed into the new (sandboxed) pane's shell.
        Some(command) => {
            tokio::time::sleep(Duration::from_millis(500)).await; // let the shell start
            let text = format!("{command} --kind {}\n", plan.kind);
            client.call("pane.send_text", json!({ "pane_id": pane_id, "text": text })).await?;
            Ok(())
        }
        None => start_when_ready(client, plan, pane_id).await,
    }
}

/// `agent.start`, retried while the new pane's shell is still starting (and
/// with the next name, if ours was taken meanwhile).
async fn start_when_ready(client: &HerdrClient, mut plan: Plan, pane_id: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + SHELL_WAIT;
    loop {
        let params = json!({ "name": plan.name, "kind": plan.kind, "pane_id": pane_id });
        let err = match client.call("agent.start", params).await {
            Ok(_) => return Ok(()),
            Err(err) => err,
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(err);
        }
        match herdr_code(&err) {
            Some("agent_pane_busy" | "agent_pane_unavailable") => tokio::time::sleep(SHELL_POLL).await,
            Some("agent_name_taken") if plan.auto_name => plan.name = next_name(&plan.name, &plan.kind),
            _ => return Err(err),
        }
    }
}

fn herdr_code(err: &anyhow::Error) -> Option<&str> {
    err.chain()
        .find_map(|e| e.downcast_ref::<shepsky_core::herdr::RpcError>())
        .map(|rpc| rpc.code.as_str())
}

/// herdr's rule for agent names.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z'))
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
}

/// `claude`, then `claude-2`, `claude-3`… whichever isn't taken.
fn free_name(kind: &str, store: &Store) -> String {
    let taken = |name: &str| store.agents.values().any(|a| a.name.as_deref() == Some(name));
    if !taken(kind) {
        return kind.to_string();
    }
    (2..).map(|n| format!("{kind}-{n}")).find(|n| !taken(n)).expect("some number is free")
}

/// `claude` → `claude-2`, `claude-2` → `claude-3`.
fn next_name(name: &str, kind: &str) -> String {
    let n = name
        .strip_prefix(kind)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(1);
    format!("{kind}-{}", n + 1)
}

/// `~`, `~/…` or an absolute path, which must exist, be a directory, and
/// (after resolving symlinks) sit inside `home`.
fn resolve_dir(input: &str, home: &Path) -> Result<PathBuf, Refusal> {
    let path = match input {
        "~" => home.to_path_buf(),
        _ => match input.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(input),
        },
    };
    if !path.is_absolute() {
        return Err(("bad_directory", format!("{input}: use ~/… or a full path")));
    }
    let dir = std::fs::canonicalize(&path).map_err(|_| ("bad_directory", format!("{input} doesn't exist on your Mac")))?;
    if !dir.is_dir() {
        return Err(("bad_directory", format!("{input} isn't a directory")));
    }
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    if !dir.starts_with(&home) {
        return Err(("bad_directory", format!("{input} is outside your home folder")));
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shepsky_core::herdr::{Agent, Snapshot, Workspace};

    fn store_with(names: &[&str]) -> Store {
        Store::from_snapshot(Snapshot {
            workspaces: vec![Workspace { workspace_id: "w1".into(), ..Workspace::default() }],
            agents: names
                .iter()
                .enumerate()
                .map(|(i, n)| Agent {
                    pane_id: format!("w1:p{i}"),
                    workspace_id: "w1".into(),
                    name: Some(n.to_string()),
                    ..Agent::default()
                })
                .collect(),
            ..Snapshot::default()
        })
    }

    fn req(kind: &str) -> Request {
        Request { kind: kind.into(), name: None, workspace_id: None, cwd: None, label: None }
    }

    #[test]
    fn only_allowed_kinds() {
        let home = std::env::temp_dir();
        let store = store_with(&[]);
        let refused = plan(Request { workspace_id: Some("w1".into()), ..req("bash") }, &store, &home);
        assert_eq!(refused.unwrap_err().0, "unsupported_kind");
        assert!(plan(Request { workspace_id: Some("w1".into()), ..req("codex") }, &store, &home).is_ok());
    }

    #[test]
    fn names_are_herdrs_and_free() {
        assert!(valid_name("claude-2") && valid_name("a_b9"));
        assert!(!valid_name("Claude") && !valid_name("2fast") && !valid_name("a b") && !valid_name(""));
        assert_eq!(free_name("claude", &store_with(&[])), "claude");
        assert_eq!(free_name("claude", &store_with(&["claude", "claude-2"])), "claude-3");
        assert_eq!(next_name("claude", "claude"), "claude-2");
        assert_eq!(next_name("claude-9", "claude"), "claude-10");
    }

    #[test]
    fn directories_must_exist_inside_home() {
        let home = std::fs::canonicalize(std::env::temp_dir()).unwrap().join("shepsky-start-test");
        let project = home.join("project");
        std::fs::create_dir_all(&project).unwrap();
        assert_eq!(resolve_dir("~/project", &home).unwrap(), project);
        assert_eq!(resolve_dir("~", &home).unwrap(), home);
        assert_eq!(resolve_dir("~/nope", &home).unwrap_err().0, "bad_directory");
        assert_eq!(resolve_dir("/", &home).unwrap_err().0, "bad_directory"); // outside home
        assert_eq!(resolve_dir("~/project/../..", &home).unwrap_err().0, "bad_directory"); // escapes
        assert_eq!(resolve_dir("project", &home).unwrap_err().0, "bad_directory"); // relative
    }

    #[test]
    fn new_workspace_is_named_after_its_directory() {
        let home = std::fs::canonicalize(std::env::temp_dir()).unwrap().join("shepsky-start-label");
        std::fs::create_dir_all(home.join("api-service")).unwrap();
        let plan = plan(Request { cwd: Some("~/api-service".into()), ..req("claude") }, &store_with(&[]), &home).unwrap();
        assert_eq!(
            plan.target,
            Target::NewWorkspace { cwd: home.join("api-service"), label: "api-service".into() }
        );
        let no_dir = super::plan(req("claude"), &store_with(&[]), &home);
        assert_eq!(no_dir.unwrap_err().0, "bad_directory");
    }
}
