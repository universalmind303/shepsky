//! Long-lived `events.subscribe` connection.
//!
//! Rules learned from herdr 0.9.5 (see docs/herdr-notes.md):
//! - the first line back is the ack (`subscription_started`); after that the
//!   connection is read-only — one stray write or half-close kills the stream
//!   silently;
//! - the server buffers ~512 events and gives subscribers ~5s / 8KiB of slack,
//!   so a dedicated task drains lines into a channel immediately;
//! - `pane.agent_status_changed` needs one entry per pane, and entries go
//!   stale when panes close/move — the caller resubscribes on topology events;
//! - if any referenced pane is gone the whole request is rejected — the caller
//!   treats that as "my snapshot is stale" and retries.

use std::path::Path;

use anyhow::{Context, Result, bail};
use collie_core::herdr::{EventLine, Response};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Lifecycle events that need no per-pane filter. Workspace/tab churn matters
/// for labels in the UI; pane churn triggers resubscription.
const LIFECYCLE_EVENTS: &[&str] = &[
    "pane.created",
    "pane.closed",
    "pane.moved",
    "pane.exited",
    "pane.agent_detected",
    "workspace.created",
    "workspace.closed",
    "workspace.renamed",
    "workspace.focused",
    "tab.created",
    "tab.closed",
    "tab.renamed",
    "tab.focused",
];

pub struct Subscription {
    pub events: mpsc::Receiver<EventLine>,
    reader: JoinHandle<()>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        // Aborting the reader drops the stream, which is the only safe way to
        // end a subscription (never write to it).
        self.reader.abort();
    }
}

/// Subscribe to all lifecycle events plus per-pane status entries for
/// `pane_ids`. Returns once the ack has arrived.
pub async fn subscribe(socket_path: &Path, pane_ids: &[String]) -> Result<Subscription> {
    let mut subscriptions: Vec<serde_json::Value> = LIFECYCLE_EVENTS
        .iter()
        .map(|event| json!({ "type": event }))
        .collect();
    for pane_id in pane_ids {
        subscriptions.push(json!({
            "type": "pane.agent_status_changed",
            "pane_id": pane_id,
        }));
    }
    let request = json!({
        "id": "collie:subscribe",
        "method": "events.subscribe",
        "params": { "subscriptions": subscriptions },
    });

    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connect {}", socket_path.display()))?;
    let (read_half, mut write_half) = stream.into_split();

    let mut line = serde_json::to_string(&request)?;
    line.push('\n');
    write_half.write_all(line.as_bytes()).await?;
    write_half.flush().await?;
    // Do NOT shut down the write half: herdr treats a half-close as a dead
    // subscriber. Keep it open and silent for the connection's lifetime.

    let mut reader = BufReader::new(read_half);
    let mut ack_line = String::new();
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), reader.read_line(&mut ack_line))
        .await
        .context("timed out waiting for subscription ack")??;
    if n == 0 {
        bail!("herdr closed the subscription before acking");
    }
    let ack: Response = serde_json::from_str(&ack_line)
        .with_context(|| format!("parse subscription ack: {ack_line}"))?;
    if let Some(err) = ack.error {
        return Err(anyhow::Error::new(err).context("events.subscribe rejected"));
    }
    let started = ack
        .result
        .as_ref()
        .and_then(|r| r.get("type"))
        .and_then(|t| t.as_str())
        == Some("subscription_started");
    if !started {
        bail!("unexpected subscription ack: {ack_line}");
    }

    let (tx, rx) = mpsc::channel::<EventLine>(256);
    let reader_task = tokio::spawn(async move {
        // Keep the write half alive (and silent) as long as we read.
        let _write_half = write_half;
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => break, // server closed (or died)
                Ok(_) => {}
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            // A fatal error (e.g. events_lost) arrives as an error line; map
            // it to channel-close so the sync loop reconnects + resnapshots.
            if let Ok(resp) = serde_json::from_str::<Response>(trimmed)
                && let Some(err) = resp.error
            {
                tracing::warn!(code = %err.code, msg = %err.message, "subscription ended by server");
                break;
            }
            match serde_json::from_str::<EventLine>(trimmed) {
                Ok(event) => {
                    if tx.send(event).await.is_err() {
                        break; // consumer dropped us
                    }
                }
                Err(err) => {
                    tracing::debug!(%err, line = %trimmed, "unparsed event line");
                }
            }
        }
        // tx drops here; the receiver sees the stream end.
    });

    Ok(Subscription {
        events: rx,
        reader: reader_task,
    })
}
