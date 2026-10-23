#!/usr/bin/env bash
# A sandboxed herdr session full of FAKE agents, so you can try shepsky without
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

SESSION="shepsky-demo"
SOCK="$HOME/.config/herdr/sessions/$SESSION/herdr.sock"
PORT="${SHEPSKY_DEMO_PORT:-7718}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
RUN_DIR="${TMPDIR:-/tmp}/shepsky-demo"
# The fake agents' transcripts, for the chat view.
TRANSCRIPTS="$RUN_DIR/transcripts"
mkdir -p "$TRANSCRIPTS"
ADB="${ANDROID_HOME:-$HOME/Library/Android/sdk}/platform-tools/adb"

# If an Android emulator with the app is running, pre-fill its pairing
# screen via the shepsky://pair link (the user still taps Connect).
# Emulators only: 10.0.2.2 is the emulator's alias for this Mac.
prefill_emulator() {
  local token="$1" serial
  [ -x "$ADB" ] || return 0
  # ANDROID_SERIAL picks the emulator when there's more than one.
  serial="${ANDROID_SERIAL:-$("$ADB" devices | awk '$1 ~ /^emulator-/ && $2 == "device" {print $1; exit}')}"
  [ -n "$serial" ] || return 0
  if ! "$ADB" -s "$serial" shell pm path dev.shepsky >/dev/null 2>&1; then
    echo "  (emulator $serial is running, but the shepsky app isn't installed — see scripts/try-it.sh)"
    return 0
  fi
  local link="shepsky://pair?url=ws%3A%2F%2F10.0.2.2%3A${PORT}%2Fws&token=${token}"
  "$ADB" -s "$serial" shell "am start -a android.intent.action.VIEW -d '$link' dev.shepsky" >/dev/null
  echo "  Pairing is pre-filled in the emulator ($serial): tap Connect."
}

case "${1:-up}" in
  up)
    echo "demo startup is disabled until herdr supports guarded demo operations" >&2
    exit 1
    if [ ! -S "$SOCK" ]; then
      echo "starting demo herdr session '$SESSION'…"
      nohup herdr --session "$SESSION" server >"$RUN_DIR/herdr.log" 2>&1 &
      echo $! >"$RUN_DIR/herdr.pid"
      for _ in $(seq 1 40); do [ -S "$SOCK" ] && break; sleep 0.25; done
      [ -S "$SOCK" ] || { echo "demo herdr did not come up (see $RUN_DIR/herdr.log)"; exit 1; }
    fi
    echo "creating fake agents…"
    HERDR_SOCKET_PATH="$SOCK" SHEPSKY_DEMO_TRANSCRIPTS="$TRANSCRIPTS" node "$REPO/scripts/demo.mjs"
    echo "building + starting shepsky-bridge on 127.0.0.1:${PORT}…"
    cargo build -q --release -p shepsky-bridge --manifest-path "$REPO/Cargo.toml"
    pkill -f "shepsky-bridge --socket $SOCK serve" 2>/dev/null || true
    # Agents started from the phone are fake ones too.
    DEMO_AGENT="$(printf '%q %q --idle --transcripts %q' "$(command -v node)" "$REPO/scripts/fake-agent.mjs" "$TRANSCRIPTS")"
    nohup "$REPO/target/release/shepsky-bridge" --socket "$SOCK" serve --bind "127.0.0.1:$PORT" --demo \
      --demo-agent "$DEMO_AGENT" --demo-transcripts "$TRANSCRIPTS" >"$RUN_DIR/bridge.log" 2>&1 &
    echo $! >"$RUN_DIR/bridge.pid"
    sleep 1
    TOKEN="$("$REPO/target/release/shepsky-bridge" pair --port "$PORT" | awk '/token:/ {print $2}')"
    echo
    echo "demo is up. Pair the app with:"
    echo "  URL (emulator):  ws://10.0.2.2:$PORT/ws"
    echo "  token:           $TOKEN"
    echo
    prefill_emulator "$TOKEN"
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
