//! The phone-side connection runtime: owns the WebSocket to the bridge,
//! authenticates, reconnects with backoff, and shuttles JSON both ways.
//!
//! Everything the UI sees arrives as one JSON event stream:
//! - every bridge `ServerMsg` is forwarded verbatim (already `type`-tagged);
//! - synthetic `{"type":"conn","phase":...}` events report connection state.
//!
//! Commands from Kotlin are `ClientMsg` JSON, forwarded verbatim when
//! connected (auth is handled here, not by Kotlin).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const PING_INTERVAL: Duration = Duration::from_secs(30);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub url: String,
    pub token: String,
}

/// Where events for Kotlin go. Unbounded std channel: sends never block the
/// async runtime, and the JNI callback thread drains it.
pub type EventSink = std::sync::mpsc::Sender<String>;

fn emit_conn(events: &EventSink, phase: &str, detail: Option<String>, attempt: u32) {
    let event = serde_json::json!({
        "type": "conn",
        "phase": phase,
        "detail": detail,
        "attempt": attempt,
    });
    let _ = events.send(event.to_string());
}

fn emit_error(events: &EventSink, code: &str, message: &str, op_id: Option<String>) {
    let event = serde_json::json!({
        "type": "error",
        "code": code,
        "message": message,
        "op_id": op_id,
    });
    let _ = events.send(event.to_string());
}

/// The `op_id` of a command, if it has one — echoed on local rejections so
/// the UI can tie the failure to what the user typed.
fn op_id_of(command: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(command)
        .ok()?
        .get("op_id")?
        .as_str()
        .map(str::to_string)
}

/// Runs until `commands` closes (= `stop()` from Kotlin).
pub async fn run(config: Config, mut commands: mpsc::Receiver<String>, events: EventSink) {
    let mut attempt: u32 = 0;
    loop {
        emit_conn(&events, "connecting", None, attempt);
        match connect_async(&config.url).await {
            Ok((socket, _response)) => {
                let (mut sink, mut stream) = socket.split();
                let auth = serde_json::json!({ "type": "auth", "token": config.token });
                if sink.send(Message::text(auth.to_string())).await.is_err() {
                    emit_conn(&events, "disconnected", Some("auth send failed".into()), attempt);
                } else {
                    emit_conn(&events, "connected", None, attempt);
                    attempt = 0;
                    let mut ping = tokio::time::interval(PING_INTERVAL);
                    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                    let reason = loop {
                        tokio::select! {
                            command = commands.recv() => {
                                let Some(command) = command else { return }; // stopped
                                if sink.send(Message::text(command)).await.is_err() {
                                    break "send failed".to_string();
                                }
                            }
                            incoming = stream.next() => {
                                match incoming {
                                    Some(Ok(Message::Text(text))) => {
                                        let _ = events.send(text.to_string());
                                    }
                                    Some(Ok(Message::Close(frame))) => {
                                        break frame
                                            .map(|f| f.reason.to_string())
                                            .unwrap_or_else(|| "closed by bridge".into());
                                    }
                                    Some(Ok(_)) => {} // ping/pong/binary
                                    Some(Err(err)) => break err.to_string(),
                                    None => break "stream ended".into(),
                                }
                            }
                            _ = ping.tick() => {
                                if sink.send(Message::Ping(vec![].into())).await.is_err() {
                                    break "ping failed".to_string();
                                }
                            }
                        }
                    };
                    emit_conn(&events, "disconnected", Some(reason), attempt);
                }
            }
            Err(err) => {
                emit_conn(&events, "disconnected", Some(err.to_string()), attempt);
            }
        }

        // Backoff, still answering (rejecting) commands and honoring stop.
        let delay = Duration::from_secs(1 << attempt.min(5)).min(BACKOFF_MAX);
        attempt = attempt.saturating_add(1);
        let wait = tokio::time::sleep(delay);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                command = commands.recv() => {
                    match command {
                        Some(command) => emit_error(
                            &events,
                            "not_connected",
                            "bridge link is down; reconnecting",
                            op_id_of(&command),
                        ),
                        None => return, // stopped
                    }
                }
            }
        }
    }
}
