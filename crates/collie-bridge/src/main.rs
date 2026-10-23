//! collie-bridge: the Mac-side daemon that connects herdr to collie phones.
//!
//! M1 scope: `status [--watch]` — prove the herdr client, subscription and
//! sync loop against a live server. The WebSocket server arrives in M2.

mod herdr_client;
mod render;
mod subscription;
mod sync;

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::watch;

use crate::herdr_client::{HerdrClient, default_socket_path};
use crate::sync::UiState;

#[derive(Parser)]
#[command(name = "collie-bridge", version, about = "herdr -> phone bridge")]
struct Cli {
    /// Path to herdr's API socket (default: $HERDR_SOCKET_PATH, then
    /// ~/.config/herdr/herdr.sock).
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show agents and their status.
    Status {
        /// Keep running and live-update as agents change.
        #[arg(long)]
        watch: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let watch_mode = matches!(cli.command, Command::Status { watch: true });

    // In watch mode the alternate screen owns the terminal; keep logs off
    // unless the user explicitly asks via RUST_LOG.
    let default_filter = if watch_mode { "off" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter)),
        )
        .with_writer(std::io::stderr)
        .init();

    let socket_path = match cli.socket {
        Some(path) => path,
        None => default_socket_path()?,
    };
    let client = HerdrClient::new(socket_path);

    match cli.command {
        Command::Status { watch: false } => status_once(client).await,
        Command::Status { watch: true } => status_watch(client).await,
    }
}

async fn status_once(client: HerdrClient) -> Result<()> {
    let pong = client.ping().await?;
    let store = collie_core::store::Store::from_snapshot(client.snapshot().await?);
    let state = UiState {
        store: std::sync::Arc::new(store),
        recent_notes: std::sync::Arc::new(Vec::new()),
        connected: true,
    };
    print!("{}", render::render(&state, None));
    tracing::debug!(version = %pong.version, protocol = pong.protocol, "herdr ping ok");
    Ok(())
}

async fn status_watch(client: HerdrClient) -> Result<()> {
    let (tx, mut rx) = watch::channel(UiState::default());
    let sync_task = tokio::spawn(sync::run(client, tx));

    let mut stdout = std::io::stdout();
    // Alternate screen + hidden cursor; restored on every exit path below.
    let _ = write!(stdout, "\x1b[?1049h\x1b[?25l");
    let _ = stdout.flush();

    let mut last_update = Instant::now();
    let mut redraw_tick = tokio::time::interval(std::time::Duration::from_secs(1));

    let result = loop {
        {
            let state = rx.borrow();
            let frame = render::render(&state, Some(last_update.elapsed()));
            let _ = write!(stdout, "\x1b[H\x1b[2J{frame}");
            let _ = stdout.flush();
        }
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    break Err(anyhow::anyhow!("sync loop ended unexpectedly"));
                }
                last_update = Instant::now();
            }
            _ = redraw_tick.tick() => {}
            _ = tokio::signal::ctrl_c() => break Ok(()),
        }
    };

    sync_task.abort();
    let _ = write!(stdout, "\x1b[?25h\x1b[?1049l");
    let _ = stdout.flush();
    result
}
