//! The sync loop: keeps a `Store` in lockstep with herdr.
//!
//! Shape (mirrors herdr's own model — events are invalidation, snapshot is
//! truth):
//!   1. snapshot -> store
//!   2. subscribe (lifecycle + per-pane status entries)
//!   3. status events patch the store immediately (low latency); every event
//!      also arms a short debounce, after which one fresh snapshot re-syncs;
//!   4. topology events (pane created/closed/moved) additionally force a
//!      resubscribe, since per-pane entries went stale;
//!   5. a slow safety-net resnapshot catches anything events missed (notably
//!      the silent done->idle flip has its own event path, but belt+braces);
//!   6. any error: backoff, reconnect, start over at 1.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use collie_core::herdr::{AgentStatusChanged, EventKind, classify_event};
use collie_core::notify::{Notification, NotifyState};
use collie_core::protocol::NoteView;
use collie_core::store::{Patch, Store};
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;

use crate::herdr_client::HerdrClient;
use crate::subscription::subscribe;

const DEBOUNCE: Duration = Duration::from_millis(150);
const SAFETY_NET: Duration = Duration::from_secs(5);
const BACKOFF_START: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// What the sync loop publishes to consumers (the watch UI now; the
/// WebSocket fan-out in M2).
#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub store: Arc<Store>,
    /// Most recent notifications, newest last (capped).
    pub recent_notes: Arc<Vec<Notification>>,
    /// Whether we currently have a live herdr connection.
    pub connected: bool,
}

const RECENT_NOTES_CAP: usize = 6;

/// Run forever, publishing state into `tx` and pushing each notification into
/// `notes_tx` (ignored when nobody listens).
pub async fn run(
    client: HerdrClient,
    tx: watch::Sender<UiState>,
    notes_tx: broadcast::Sender<NoteView>,
) {
    let mutes = BTreeSet::new(); // per-agent mutes arrive with the phone app (M5)
    let mut notify_state = NotifyState::default();
    let mut recent_notes: Vec<Notification> = Vec::new();
    let mut backoff = BACKOFF_START;

    'outer: loop {
        // 1. Snapshot.
        let snapshot = match client.snapshot().await {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(%err, "snapshot failed; retrying in {backoff:?}");
                mark_disconnected(&tx);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        let mut store = Store::from_snapshot(snapshot);
        publish(&tx, &notes_tx, &store, &mut notify_state, &mut recent_notes, &mutes);

        // 2. Subscribe for the current pane set.
        let pane_ids: Vec<String> = store.pane_ids.iter().cloned().collect();
        let mut subscription = match subscribe(client.socket_path(), &pane_ids).await {
            Ok(s) => s,
            Err(err) => {
                // Typically: a pane closed between snapshot and subscribe.
                tracing::warn!(%err, "subscribe failed; retrying in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        backoff = BACKOFF_START;
        tracing::info!(panes = pane_ids.len(), "subscribed to herdr events");

        // 3. Event loop.
        let mut needs_resub = false;
        let mut resync_at: Option<Instant> = None;
        let mut safety_net = tokio::time::interval(SAFETY_NET);
        safety_net.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        safety_net.reset(); // don't fire immediately; we just snapshotted

        loop {
            let debounce = async {
                match resync_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };

            tokio::select! {
                event = subscription.events.recv() => {
                    let Some(event) = event else {
                        tracing::info!("herdr event stream ended; resyncing");
                        continue 'outer;
                    };
                    match classify_event(&event.event) {
                        EventKind::Status => {
                            if let Ok(change) =
                                serde_json::from_value::<AgentStatusChanged>(event.data.clone())
                            {
                                match store.patch_agent_status(&change) {
                                    Patch::Changed => publish(
                                        &tx, &notes_tx, &store, &mut notify_state,
                                        &mut recent_notes, &mutes,
                                    ),
                                    Patch::Unchanged => {}
                                    Patch::UnknownPane => needs_resub = true,
                                }
                            }
                            resync_at = Some(Instant::now() + DEBOUNCE);
                        }
                        EventKind::Topology => {
                            needs_resub = true;
                            resync_at = Some(Instant::now() + DEBOUNCE);
                        }
                        EventKind::Other => {
                            resync_at = Some(Instant::now() + DEBOUNCE);
                        }
                    }
                }

                _ = debounce => {
                    resync_at = None;
                    match client.snapshot().await {
                        Ok(snapshot) => {
                            store.apply_snapshot(snapshot);
                            publish(&tx, &notes_tx, &store, &mut notify_state, &mut recent_notes, &mutes);
                        }
                        Err(err) => {
                            tracing::warn!(%err, "debounced resync failed; reconnecting");
                            continue 'outer;
                        }
                    }
                    if needs_resub {
                        continue 'outer; // rebuild subscription for the new pane set
                    }
                }

                _ = safety_net.tick() => {
                    match client.snapshot().await {
                        Ok(snapshot) => {
                            let fresh = Store::from_snapshot(snapshot);
                            if fresh != store {
                                store = fresh;
                                publish(&tx, &notes_tx, &store, &mut notify_state, &mut recent_notes, &mutes);
                                // Pane set may have changed without us seeing
                                // the event (e.g. buffer overflow window).
                                let fresh_panes: Vec<String> =
                                    store.pane_ids.iter().cloned().collect();
                                if fresh_panes != pane_ids {
                                    continue 'outer;
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(%err, "safety-net snapshot failed; reconnecting");
                            continue 'outer;
                        }
                    }
                }
            }
        }
    }
}

fn publish(
    tx: &watch::Sender<UiState>,
    notes_tx: &broadcast::Sender<NoteView>,
    store: &Store,
    notify_state: &mut NotifyState,
    recent_notes: &mut Vec<Notification>,
    mutes: &BTreeSet<String>,
) {
    let notes = notify_state.update(store, mutes);
    for note in &notes {
        tracing::info!(
            kind = ?note.kind,
            agent = %note.agent_label,
            workspace = note.workspace_label.as_deref().unwrap_or("-"),
            "notification"
        );
    }
    for note in &notes {
        let _ = notes_tx.send(NoteView::from(note));
    }
    recent_notes.extend(notes);
    let overflow = recent_notes.len().saturating_sub(RECENT_NOTES_CAP);
    if overflow > 0 {
        recent_notes.drain(..overflow);
    }
    tx.send_replace(UiState {
        store: Arc::new(store.clone()),
        recent_notes: Arc::new(recent_notes.clone()),
        connected: true,
    });
}

fn mark_disconnected(tx: &watch::Sender<UiState>) {
    tx.send_modify(|state| state.connected = false);
}
