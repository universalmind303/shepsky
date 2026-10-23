#!/usr/bin/env bash
# A sandboxed herdr session full of FAKE agents, so you can try collie without
# touching your real workspace or spending agent tokens.
#
#   scripts/demo-session.sh up        # start demo herdr + fake agents + bridge
#   scripts/demo-session.sh scenario  # keep statuses changing (foreground)
#   scripts/demo-session.sh down      # stop everything
#
# The bridge serves the DEMO session on port 7718 (your real one can keep 7717).
set -euo pipefail

SESSION="collie-demo"
SOCK="$HOME/.config/herdr/sessions/$SESSION/herdr.sock"
PORT="${COLLIE_DEMO_PORT:-7718}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
RUN_DIR="${TMPDIR:-/tmp}/collie-demo"
mkdir -p "$RUN_DIR"

case "${1:-up}" in
  up)
    if [ ! -S "$SOCK" ]; then
      echo "starting demo herdr session '$SESSION'…"
      nohup herdr --session "$SESSION" server >"$RUN_DIR/herdr.log" 2>&1 &
      echo $! >"$RUN_DIR/herdr.pid"
      for _ in $(seq 1 40); do [ -S "$SOCK" ] && break; sleep 0.25; done
      [ -S "$SOCK" ] || { echo "demo herdr did not come up (see $RUN_DIR/herdr.log)"; exit 1; }
    fi
    echo "creating fake agents…"
    HERDR_SOCKET_PATH="$SOCK" node "$REPO/scripts/demo.mjs" setup
    echo "building + starting collie-bridge on 127.0.0.1:$PORT…"
    cargo build -p collie-bridge --manifest-path "$REPO/Cargo.toml" >/dev/null
    nohup "$REPO/target/debug/collie-bridge" --socket "$SOCK" serve --bind "127.0.0.1:$PORT" \
      >"$RUN_DIR/bridge.log" 2>&1 &
    echo $! >"$RUN_DIR/bridge.pid"
    sleep 1
    echo
    echo "demo is up. Pair the app with:"
    echo "  URL (emulator):  ws://10.0.2.2:$PORT/ws"
    echo "  token:           $(cat "$HOME/.config/collie/token")"
    echo
    echo "now run: scripts/demo-session.sh scenario   (keeps statuses moving)"
    ;;
  scenario)
    HERDR_SOCKET_PATH="$SOCK" node "$REPO/scripts/demo.mjs" scenario
    ;;
  down)
    [ -f "$RUN_DIR/bridge.pid" ] && kill "$(cat "$RUN_DIR/bridge.pid")" 2>/dev/null || true
    herdr --session "$SESSION" server stop 2>/dev/null \
      || { [ -f "$RUN_DIR/herdr.pid" ] && kill "$(cat "$RUN_DIR/herdr.pid")" 2>/dev/null; } || true
    echo "demo stopped."
    ;;
  *)
    echo "usage: $0 [up|scenario|down]" >&2
    exit 2
    ;;
esac
