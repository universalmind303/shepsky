//! Terminal mirroring: poll a pane's visible screen while at least one phone
//! watches it, parse to styled frames, and broadcast changes.
//!
//! Read-only by construction: `pane.read {source:"visible", format:"ansi"}`
//! never triggers herdr's scroll-to-collect-history side effect (that needs
//! `format:"text"` + a `recent` source). See docs/herdr-notes.md.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use shepsky_core::protocol::{PaneScroll, ServerMsg};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::herdr_client::HerdrClient;

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const CHANNEL_CAP: usize = 8;

struct PaneWatch {
    tx: broadcast::Sender<ServerMsg>,
    /// Last frame sent, so a late joiner doesn't wait for the screen to change.
    latest: Arc<Mutex<Option<ServerMsg>>>,
    watchers: usize,
    task: JoinHandle<()>,
}

/// Shared across all phone connections; one poll task per watched pane,
/// refcounted.
pub struct TerminalHub {
    client: HerdrClient,
    panes: Mutex<HashMap<String, PaneWatch>>,
}

impl TerminalHub {
    pub fn new(client: HerdrClient) -> Arc<Self> {
        Arc::new(Self {
            client,
            panes: Mutex::new(HashMap::new()),
        })
    }

    /// Start (or join) the watch on a pane. Returns the current frame, if
    /// the pane is already being polled, plus the stream of changes.
    pub fn watch(
        self: &Arc<Self>,
        pane_id: &str,
    ) -> (Option<ServerMsg>, broadcast::Receiver<ServerMsg>) {
        let mut panes = self.panes.lock().unwrap();
        if let Some(watch) = panes.get_mut(pane_id) {
            watch.watchers += 1;
            // Subscribe before reading `latest`: a frame racing in between
            // then arrives twice (harmless) instead of not at all.
            let rx = watch.tx.subscribe();
            let latest = watch.latest.lock().unwrap().clone();
            return (latest, rx);
        }
        let (tx, rx) = broadcast::channel(CHANNEL_CAP);
        let latest = Arc::new(Mutex::new(None));
        let task = tokio::spawn(poll_pane(
            self.client.clone(),
            pane_id.to_string(),
            tx.clone(),
            latest.clone(),
        ));
        panes.insert(
            pane_id.to_string(),
            PaneWatch {
                tx,
                latest,
                watchers: 1,
                task,
            },
        );
        tracing::debug!(pane_id, "terminal watch started");
        (None, rx)
    }

    /// Drop one watcher; the poll task stops when nobody's left.
    pub fn unwatch(&self, pane_id: &str) {
        let mut panes = self.panes.lock().unwrap();
        if let Some(watch) = panes.get_mut(pane_id) {
            watch.watchers = watch.watchers.saturating_sub(1);
            if watch.watchers == 0 {
                if let Some(watch) = panes.remove(pane_id) {
                    watch.task.abort();
                }
                tracing::debug!(pane_id, "terminal watch stopped");
            }
        }
    }
}

async fn poll_pane(
    client: HerdrClient,
    pane_id: String,
    tx: broadcast::Sender<ServerMsg>,
    latest: Arc<Mutex<Option<ServerMsg>>>,
) {
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_hash: u64 = 0;
    let mut last_error: Option<String> = None;
    let mut seq: u64 = 0;

    loop {
        ticker.tick().await;
        let params = json!({
            "pane_id": pane_id,
            "source": "visible",
            "format": "ansi",
            "strip_ansi": false,
        });
        match client.call("pane.read", params).await {
            Ok(result) => {
                last_error = None;
                let Some(text) = result
                    .get("read")
                    .and_then(|r| r.get("text"))
                    .and_then(|t| t.as_str())
                else {
                    continue;
                };
                // Where herdr's view is scrolled (a phone may have scrolled
                // it back); a frame goes out when either changes.
                let scroll = client
                    .call("pane.get", json!({ "pane_id": pane_id }))
                    .await
                    .ok()
                    .and_then(|r| serde_json::from_value::<PaneScroll>(r["pane"]["scroll"].clone()).ok());
                let mut hasher = DefaultHasher::new();
                text.hash(&mut hasher);
                scroll.hash(&mut hasher);
                let hash = hasher.finish();
                if hash == last_hash {
                    continue;
                }
                last_hash = hash;
                seq += 1;
                let msg = ServerMsg::TerminalFrame {
                    pane_id: pane_id.clone(),
                    seq,
                    frame: shepsky_core::ansi::parse_frame(text),
                    scroll,
                };
                *latest.lock().unwrap() = Some(msg.clone());
                // Lagging receivers just miss intermediate frames; the next
                // changed frame is always a full screen, so no harm done.
                let _ = tx.send(msg);
            }
            Err(err) => {
                // Pane mid-close, herdr restarting… Tell watchers once per
                // distinct error and keep trying; the connection layer drops
                // the watch when the pane leaves the state.
                let msg = err.to_string();
                if last_error.as_deref() != Some(msg.as_str()) {
                    tracing::debug!(pane_id, %msg, "terminal read failed");
                    let _ = tx.send(ServerMsg::WatchError {
                        pane_id: pane_id.clone(),
                        message: msg.clone(),
                    });
                    last_error = Some(msg);
                }
            }
        }
    }
}
