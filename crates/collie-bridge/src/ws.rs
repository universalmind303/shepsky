//! The phone-facing WebSocket server.
//!
//! Binds loopback only; remote access is `tailscale serve` (HTTPS, tailnet-
//! only) in front of it. Every connection must authenticate with the bearer
//! token as its first message, then gets `Hello` + a full `State`, followed by
//! `State` pushes on every change, `Notify` pushes, and `TerminalFrame`s for
//! watched panes.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use collie_core::protocol::{ClientMsg, PROTO_VERSION, ServerMsg, StateView};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

use crate::auth::token_matches;
use crate::herdr_client::HerdrClient;
use crate::sync::UiState;
use crate::terminal::TerminalHub;

const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WATCHED_PANES: usize = 8;
const MAX_PROMPT_BYTES: usize = 8 * 1024;
const MAX_KEYS: usize = 32;
const OUTBOX_CAP: usize = 64;

pub struct ServeState {
    pub ui_rx: watch::Receiver<UiState>,
    pub notes_tx: broadcast::Sender<collie_core::protocol::NoteView>,
    pub hub: Arc<TerminalHub>,
    pub client: HerdrClient,
    pub token: String,
}

pub async fn serve(state: ServeState, bind: SocketAddr) -> Result<()> {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(Arc::new(state));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    tracing::info!("listening on ws://{bind}/ws");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(
    State(state): State<Arc<ServeState>>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| async move {
        if let Err(err) = handle_conn(socket, state).await {
            tracing::debug!(%err, "connection ended");
        }
    })
}

fn to_text(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).expect("ServerMsg serializes").into())
}

async fn handle_conn(socket: WebSocket, state: Arc<ServeState>) -> Result<()> {
    let (mut sink, mut stream) = socket.split();

    // --- auth phase -------------------------------------------------------
    let first = tokio::time::timeout(AUTH_TIMEOUT, stream.next())
        .await
        .context("no auth message in time")?
        .context("closed before auth")??;
    let authed = matches!(
        &first,
        Message::Text(text) if matches!(
            serde_json::from_str::<ClientMsg>(text),
            Ok(ClientMsg::Auth { token }) if token_matches(&state.token, &token)
        )
    );
    if !authed {
        let _ = sink
            .send(to_text(&ServerMsg::Error {
                code: "unauthorized".into(),
                message: "first message must be a valid auth".into(),
                op_id: None,
            }))
            .await;
        let _ = sink.close().await;
        anyhow::bail!("auth failed");
    }
    tracing::info!("phone connected");

    // --- single-writer outbox ---------------------------------------------
    let (out_tx, mut out_rx) = mpsc::channel::<ServerMsg>(OUTBOX_CAP);
    let writer: JoinHandle<()> = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(to_text(&msg)).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut ui_rx = state.ui_rx.clone();
    let mut notes_rx = state.notes_tx.subscribe();

    let hello = {
        let ui = ui_rx.borrow();
        ServerMsg::Hello {
            proto: PROTO_VERSION,
            bridge_version: env!("CARGO_PKG_VERSION").into(),
            herdr_version: ui.store.herdr_version.clone(),
        }
    };
    out_tx.send(hello).await.ok();
    let mut last_state = {
        let ui = ui_rx.borrow_and_update();
        StateView::project(&ui.store, ui.connected)
    };
    out_tx
        .send(ServerMsg::State { state: last_state.clone() })
        .await
        .ok();

    // Per-connection forwarders from the terminal hub into the outbox.
    let mut watched: HashMap<String, JoinHandle<()>> = HashMap::new();

    let result: Result<()> = async {
        loop {
            tokio::select! {
                inbound = stream.next() => {
                    let Some(msg) = inbound else { break };
                    match msg? {
                        Message::Text(text) => {
                            handle_client_msg(&text, &state, &ui_rx, &out_tx, &mut watched).await;
                        }
                        Message::Close(_) => break,
                        _ => {} // binary unused; ping/pong handled by tungstenite
                    }
                }
                changed = ui_rx.changed() => {
                    if changed.is_err() { break }
                    let next = {
                        let ui = ui_rx.borrow_and_update();
                        StateView::project(&ui.store, ui.connected)
                    };
                    // UiState also changes when only recent_notes move; don't
                    // re-push identical states.
                    if next != last_state {
                        last_state = next.clone();
                        if out_tx.send(ServerMsg::State { state: next }).await.is_err() {
                            break;
                        }
                    }
                }
                note = notes_rx.recv() => {
                    match note {
                        Ok(note) => {
                            if out_tx.send(ServerMsg::Notify { note }).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {} // skip missed
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    for (pane_id, task) in watched.drain() {
        task.abort();
        state.hub.unwatch(&pane_id);
    }
    writer.abort();
    tracing::info!("phone disconnected");
    result
}

async fn handle_client_msg(
    text: &str,
    state: &Arc<ServeState>,
    ui_rx: &watch::Receiver<UiState>,
    out_tx: &mpsc::Sender<ServerMsg>,
    watched: &mut HashMap<String, JoinHandle<()>>,
) {
    let send_err = |code: &str, message: String, op_id: Option<String>| {
        let out = out_tx.clone();
        let (code, message) = (code.to_string(), message);
        async move {
            out.send(ServerMsg::Error { code, message, op_id }).await.ok();
        }
    };

    let msg: ClientMsg = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(err) => {
            send_err("bad_message", err.to_string(), None).await;
            return;
        }
    };

    match msg {
        ClientMsg::Auth { .. } => {} // already authed; ignore
        ClientMsg::Ping => {
            out_tx.send(ServerMsg::Pong).await.ok();
        }

        ClientMsg::WatchPane { pane_id } => {
            if watched.contains_key(&pane_id) {
                return;
            }
            if watched.len() >= MAX_WATCHED_PANES {
                send_err("too_many_watches", format!("limit is {MAX_WATCHED_PANES}"), None).await;
                return;
            }
            if !ui_rx.borrow().store.pane_ids.contains(&pane_id) {
                send_err("not_found", format!("unknown pane {pane_id}"), None).await;
                return;
            }
            let mut frames = state.hub.watch(&pane_id);
            let out = out_tx.clone();
            let forwarder = tokio::spawn(async move {
                loop {
                    match frames.recv().await {
                        Ok(frame) => {
                            if out.send(frame).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {} // next frame is a full screen
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            watched.insert(pane_id, forwarder);
        }

        ClientMsg::UnwatchPane { pane_id } => {
            if let Some(task) = watched.remove(&pane_id) {
                task.abort();
                state.hub.unwatch(&pane_id);
            }
        }

        ClientMsg::Prompt { pane_id, text, op_id } => {
            if text.len() > MAX_PROMPT_BYTES {
                send_err("too_large", format!("prompt over {MAX_PROMPT_BYTES} bytes"), op_id).await;
                return;
            }
            if !agent_exists(ui_rx, &pane_id) {
                send_err("not_found", format!("no agent in pane {pane_id}"), op_id).await;
                return;
            }
            run_herdr_op(
                state, out_tx, op_id,
                "agent.prompt",
                json!({ "target": pane_id, "text": text }),
            );
        }

        ClientMsg::SendKeys { pane_id, keys, op_id } => {
            if keys.is_empty() || keys.len() > MAX_KEYS || keys.iter().any(|k| k.len() > 32) {
                send_err("bad_keys", "1..=32 keys, each <= 32 chars".into(), op_id).await;
                return;
            }
            if !agent_exists(ui_rx, &pane_id) {
                send_err("not_found", format!("no agent in pane {pane_id}"), op_id).await;
                return;
            }
            run_herdr_op(
                state, out_tx, op_id,
                "agent.send_keys",
                json!({ "target": pane_id, "keys": keys }),
            );
        }

        ClientMsg::FocusAgent { pane_id, op_id } => {
            if !agent_exists(ui_rx, &pane_id) {
                send_err("not_found", format!("no agent in pane {pane_id}"), op_id).await;
                return;
            }
            run_herdr_op(
                state, out_tx, op_id,
                "agent.focus",
                json!({ "target": pane_id }),
            );
        }
    }
}

fn agent_exists(ui_rx: &watch::Receiver<UiState>, pane_id: &str) -> bool {
    ui_rx.borrow().store.agents.contains_key(pane_id)
}

/// Run one herdr write op off the connection loop; report Ack/Error by op_id.
fn run_herdr_op(
    state: &Arc<ServeState>,
    out_tx: &mpsc::Sender<ServerMsg>,
    op_id: Option<String>,
    method: &'static str,
    params: serde_json::Value,
) {
    let client = state.client.clone();
    let out = out_tx.clone();
    tokio::spawn(async move {
        let reply = match client.call(method, params).await {
            Ok(_) => ServerMsg::Ack { op_id },
            Err(err) => ServerMsg::Error {
                code: "herdr_error".into(),
                message: err.to_string(),
                op_id,
            },
        };
        out.send(reply).await.ok();
    });
}
