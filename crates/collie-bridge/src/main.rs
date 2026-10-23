//! collie-bridge: the Mac-side daemon that connects herdr to collie phones.
//!
//! - `status [--watch]` — agent table in the terminal
//! - `serve`            — WebSocket server for phones (loopback only)
//! - `pair`             — print what the phone needs to connect

mod auth;
mod herdr_client;
mod render;
mod subscription;
mod sync;
mod terminal;
mod ws;

use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::{broadcast, watch};

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
    /// Serve the phone WebSocket API.
    Serve {
        /// Address to bind. Keep it loopback; expose remotely with
        /// `tailscale serve` (see README).
        #[arg(long, default_value = "127.0.0.1:7717")]
        bind: SocketAddr,
    },
    /// Print the connection details for pairing a phone.
    Pair {
        /// Port the bridge serves on.
        #[arg(long, default_value_t = 7717)]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // In watch mode the alternate screen owns the terminal; keep logs off
    // unless the user explicitly asks via RUST_LOG.
    let default_filter = match &cli.command {
        Command::Status { watch: true } => "off",
        Command::Serve { .. } => "info",
        _ => "warn",
    };
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
        Command::Serve { bind } => serve(client, bind).await,
        Command::Pair { port } => pair(port),
    }
}

async fn serve(client: HerdrClient, bind: SocketAddr) -> Result<()> {
    let token = auth::load_or_create_token()?;

    // Fail fast with a friendly message if herdr isn't reachable.
    match client.ping().await {
        Ok(pong) => tracing::info!(version = %pong.version, "herdr is up"),
        Err(err) => tracing::warn!(%err, "herdr not reachable yet; will keep retrying"),
    }

    let (ui_tx, ui_rx) = watch::channel(UiState::default());
    let (notes_tx, _) = broadcast::channel(64);
    tokio::spawn(sync::run(client.clone(), ui_tx, notes_tx.clone()));
    let hub = terminal::TerminalHub::new(client.clone());

    eprintln!();
    eprintln!("  collie-bridge is up:   ws://{bind}/ws");
    eprintln!("  pairing details:       collie-bridge pair");
    eprintln!();

    ws::serve(
        ws::ServeState {
            ui_rx,
            notes_tx,
            hub,
            client,
            token,
        },
        bind,
    )
    .await
}

fn pair(port: u16) -> Result<()> {
    let token = auth::load_or_create_token()?;
    println!();
    println!("Pair your phone (Settings screen in the collie app):");
    println!();
    println!("  token:  {token}");
    println!();
    println!("  URL depends on where the phone is:");
    println!("    Android emulator on this Mac:  ws://10.0.2.2:{port}/ws");
    println!("    Tailscale (recommended):       wss://<this-mac>.<tailnet>.ts.net/ws");
    println!("      one-time setup: tailscale serve --bg --https=443 http://127.0.0.1:{port}");
    println!("    Same Wi-Fi (no TLS):           ws://<this-mac's-LAN-IP>:{port}/ws");
    println!("      requires: collie-bridge serve --bind 0.0.0.0:{port}  (token is the only gate — prefer Tailscale)");
    println!();
    Ok(())
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
    let (notes_tx, _) = broadcast::channel(64);
    let sync_task = tokio::spawn(sync::run(client, tx, notes_tx));

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
