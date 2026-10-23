#!/usr/bin/env bash
# A sandboxed herdr session full of FAKE agents, so you can try collie without
# touching your real workspace or spending agent tokens.
#
#   scripts/demo-session.sh up     # demo herdr + fake agents + bridge on :7718
#   scripts/demo-session.sh down   # stop everything
#
# The fake agents (scripts/fake-agent.mjs) work, ask permission, and react to
# prompts/quick replies from the phone. The demo bridge runs with --demo
# (pane-level input), because herdr's agent-level input only accepts a real
# agent process. Your real bridge can keep port 7717.
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
    HERDR_SOCKET_PATH="$SOCK" node "$REPO/scripts/demo.mjs"
    echo "building + starting collie-bridge on 127.0.0.1:${PORT}…"
    cargo build -p collie-bridge --manifest-path "$REPO/Cargo.toml" >/dev/null
    pkill -f "collie-bridge --socket $SOCK serve" 2>/dev/null || true
    nohup "$REPO/target/debug/collie-bridge" --socket "$SOCK" serve --bind "127.0.0.1:$PORT" --demo \
      >"$RUN_DIR/bridge.log" 2>&1 &
    echo $! >"$RUN_DIR/bridge.pid"
    sleep 1
    TOKEN="$("$REPO/target/debug/collie-bridge" pair --port "$PORT" | awk '/token:/ {print $2}')"
    echo
    echo "demo is up. Pair the app with:"
    echo "  URL (emulator):  ws://10.0.2.2:$PORT/ws"
    echo "  token:           $TOKEN"
    ;;
  down)
    [ -f "$RUN_DIR/bridge.pid" ] && kill "$(cat "$RUN_DIR/bridge.pid")" 2>/dev/null || true
    herdr --session "$SESSION" server stop 2>/dev/null \
      || { [ -f "$RUN_DIR/herdr.pid" ] && kill "$(cat "$RUN_DIR/herdr.pid")" 2>/dev/null; } || true
    echo "demo stopped."
    ;;
  *)
    echo "usage: $0 [up|down]" >&2
    exit 2
    ;;
esac
