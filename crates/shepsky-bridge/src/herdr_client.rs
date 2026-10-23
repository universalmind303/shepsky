//! RPC client for herdr's unix socket.
//!
//! herdr answers exactly one request per connection (read line, respond,
//! close), so every call opens a fresh connection. herdr has no server-side
//! timeout on ordinary requests; we enforce our own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use shepsky_core::herdr::{Pong, Request, Response, Snapshot, SnapshotResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Every herdr method the bridge is allowed to call, ever. The socket itself
/// has no auth and allows input injection, so this list — not herdr — is the
/// security boundary on what phones can make the bridge do.
///
/// NOTE: herdr's own "changes UI" flag is NOT a read-only marker (send_text is
/// flagged as not changing the UI); this list is curated by hand.
const ALLOWED_METHODS: &[&str] = &[
    // reads
    "ping",
    "session.snapshot",
    "agent.list",
    "agent.get",
    "pane.list",
    "pane.get",
    "pane.read",
    "workspace.list",
    "tab.list",
    "events.subscribe",
    // input (the only other writes, by design)
    "agent.prompt",
    "agent.send_keys",
    "agent.focus",
    "pane.send_keys",
    "pane.send_text",
    // starting agents from the phone (guardrails in start.rs); the closes
    // only ever undo a workspace/tab start.rs just made for a failed start
    "workspace.create",
    "tab.create",
    "agent.start",
    "workspace.close",
    "tab.close",
    // renaming and closing agents (guardrails in ws.rs: only panes that
    // host an agent)
    "agent.rename",
    "pane.rename",
    "pane.close",
    // scrolling an agent pane's history (guardrail in ws.rs: agent panes
    // only; the bridge scrolls back to the bottom what a phone left behind)
    "pane.scroll",
];

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    format!("shepsky:{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

/// Default socket path, honoring `HERDR_SOCKET_PATH` then
/// `~/.config/herdr/herdr.sock`. (A launchd-started bridge won't inherit
/// `HERDR_SOCKET_PATH`; the `--socket` flag exists for that.)
pub fn default_socket_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/herdr/herdr.sock"))
}

#[derive(Debug, Clone)]
pub struct HerdrClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl HerdrClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            timeout: Duration::from_secs(10),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// One RPC: fresh connection, one request line, one response line.
    pub async fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        if !ALLOWED_METHODS.contains(&method) {
            bail!("method {method:?} is not on the bridge allowlist");
        }
        let request = Request {
            id: next_id(),
            method: method.to_string(),
            params,
        };
        let response = tokio::time::timeout(self.timeout, self.roundtrip(&request))
            .await
            .with_context(|| format!("herdr call {method} timed out"))??;
        if let Some(err) = response.error {
            return Err(anyhow::Error::new(err).context(format!("herdr call {method} failed")));
        }
        response
            .result
            .with_context(|| format!("herdr call {method}: response had no result"))
    }

    async fn roundtrip(&self, request: &Request) -> Result<Response> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("connect {}", self.socket_path.display()))?;
        let (read_half, mut write_half) = stream.into_split();

        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        write_half.write_all(line.as_bytes()).await?;
        write_half.flush().await?;

        let mut reader = BufReader::new(read_half);
        let mut response_line = String::new();
        let n = reader.read_line(&mut response_line).await?;
        if n == 0 {
            bail!("herdr closed the connection without responding");
        }
        serde_json::from_str(&response_line)
            .with_context(|| format!("parse herdr response: {response_line}"))
    }

    pub async fn ping(&self) -> Result<Pong> {
        let result = self.call("ping", serde_json::json!({})).await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn snapshot(&self) -> Result<Snapshot> {
        let result = self.call("session.snapshot", serde_json::json!({})).await?;
        let parsed: SnapshotResult = serde_json::from_value(result)?;
        Ok(parsed.snapshot)
    }
}
