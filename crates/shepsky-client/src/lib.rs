//! The phone side of shepsky, shared by the Android (JNI) and iOS (C) shims.
//!
//! One [`start`]ed session = one WebSocket to the bridge (auth, reconnect
//! with backoff, pings) on its own small tokio runtime. Commands go in as
//! `ClientMsg` JSON ([`send`]); events come out, in order, as JSON strings on
//! the returned channel: every bridge `ServerMsg` verbatim plus synthetic
//! `{"type":"conn",...}` events (see `client`). Sessions are numbered handles
//! so each platform's FFI stays a thin three-function wrapper: the platform
//! only owns the thread that hands events to its UI.

mod client;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use tokio::sync::mpsc;

pub use client::Config;

struct Session {
    commands: mpsc::Sender<String>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            // Never block the caller (a UI thread) on shutdown.
            runtime.shutdown_background();
        }
    }
}

static SESSIONS: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn sessions() -> &'static Mutex<HashMap<u64, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start a session from `{"url":..,"token":..}` JSON. Returns its handle
/// (never 0) and its event stream, which ends once the session is stopped.
pub fn start(config: &[u8]) -> Result<(u64, std::sync::mpsc::Receiver<String>), String> {
    init();
    let config: Config =
        serde_json::from_slice(config).map_err(|err| format!("bad config json: {err}"))?;
    let (events_tx, events_rx) = std::sync::mpsc::channel::<String>();
    let (commands_tx, commands_rx) = mpsc::channel::<String>(64);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("shepsky-net")
        .build()
        .map_err(|err| format!("tokio runtime: {err}"))?;
    runtime.spawn(client::run(config, commands_rx, events_tx));

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    sessions().lock().unwrap().insert(
        handle,
        Session {
            commands: commands_tx,
            runtime: Some(runtime),
        },
    );
    log::info!("session {handle} started");
    Ok((handle, events_rx))
}

/// Queue a `ClientMsg` JSON command. False if the handle is unknown or the
/// queue is full (the UI never blocks on the network).
pub fn send(handle: u64, msg: String) -> bool {
    let sender = sessions().lock().unwrap().get(&handle).map(|s| s.commands.clone());
    match sender {
        Some(sender) => sender.try_send(msg).is_ok(),
        None => false,
    }
}

/// Stop a session. Its event stream drains and ends shortly after.
pub fn stop(handle: u64) {
    let session = sessions().lock().unwrap().remove(&handle);
    if session.is_some() {
        log::info!("session {handle} stopped");
    }
    // Dropping the session shuts its runtime down in the background.
}

fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Pin rustls to the ring provider (the only one we compile).
        let _ = rustls::crypto::ring::default_provider().install_default();
        // Panics in Rust threads otherwise vanish; route them to the log.
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("rust panic: {info}");
            default(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rejects_bad_config() {
        assert!(start(b"not json").is_err());
    }

    #[test]
    fn lifecycle_reports_connection_and_ends_on_stop() {
        // Nothing listens on port 1: connecting fails fast, no network needed.
        let (handle, events) = start(br#"{"url":"ws://127.0.0.1:1/ws","token":"t"}"#).unwrap();
        let first = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.contains(r#""phase":"connecting""#), "{first}");
        let second = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(second.contains(r#""phase":"disconnected""#), "{second}");

        // While down, commands are answered locally with their op_id.
        assert!(send(handle, r#"{"type":"prompt","pane_id":"w1:p1","text":"hi","op_id":"op1"}"#.into()));
        let rejected = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(rejected.contains("not_connected") && rejected.contains("op1"), "{rejected}");

        stop(handle);
        assert!(!send(handle, "{}".into()));
        // The stream ends: the runtime is gone, so every sender is dropped.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match events.recv_timeout(Duration::from_millis(200)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    assert!(std::time::Instant::now() < deadline, "stream never ended")
                }
            }
        }
    }
}
