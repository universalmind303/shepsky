#!/usr/bin/env bash
# Try collie on this Mac: no phone, no real agents, no tokens spent.
#
#   scripts/try-it.sh        # emulator + app + demo, pairing filled in
#   scripts/try-it.sh down   # stop the demo and the emulator
#
# What it does, each step skipped when already done:
#   1. checks the toolchain and installs the missing Android SDK pieces
#   2. creates an Android emulator ("collie", Pixel 7, Android 15)
#   3. boots it in a window
#   4. builds the app and installs it
#   5. starts the demo (scripts/demo-session.sh: fake agents in a sandboxed
#      herdr session + a demo bridge), which fills in the app's pairing
#      screen. You tap Connect.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
AVD="${COLLIE_AVD:-collie}"
API=35
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_HOME/platform-tools/adb"
EMULATOR="$ANDROID_HOME/emulator/emulator"
SDKMANAGER="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
AVDMANAGER="$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager"
AVD_DIR="${ANDROID_AVD_HOME:-$HOME/.android/avd}/$AVD.avd"
RUN_DIR="${TMPDIR:-/tmp}/collie-demo"
mkdir -p "$RUN_DIR"

if [ "$(uname -m)" = "arm64" ]; then
  ABI=arm64-v8a RUST_TARGET=aarch64-linux-android
else
  ABI=x86_64 RUST_TARGET=x86_64-linux-android
fi
IMAGE="system-images;android-$API;google_apis;$ABI"

step() { printf '\n==> %s\n' "$*"; }
die() { printf '\ntry-it: %s\n' "$*" >&2; exit 1; }

running_emulator() {
  [ -x "$ADB" ] || return 0
  "$ADB" devices | awk '$1 ~ /^emulator-/ && $2 == "device" {print $1; exit}'
}

if [ "${1:-up}" = "down" ]; then
  "$REPO/scripts/demo-session.sh" down
  serial="$(running_emulator)"
  [ -n "$serial" ] && "$ADB" -s "$serial" emu kill >/dev/null && echo "emulator stopped."
  exit 0
fi

# --- 1. toolchain -------------------------------------------------------------
step "checking the toolchain"
command -v herdr >/dev/null || die "herdr isn't installed: see https://herdr.dev (0.9.5 or newer)."
command -v cargo >/dev/null || die "Rust isn't installed: see https://rustup.rs"
command -v node >/dev/null || die "Node.js isn't installed (the fake agents are Node scripts): brew install node"

if [ -z "${JAVA_HOME:-}" ]; then
  JAVA_HOME="$(/usr/libexec/java_home -v 21 2>/dev/null || /usr/libexec/java_home -v 17 2>/dev/null || true)"
fi
[ -n "$JAVA_HOME" ] || die "no JDK 17–21 found: brew install --cask temurin@21"
export JAVA_HOME

if ! rustup target list --installed 2>/dev/null | grep -qx "$RUST_TARGET"; then
  echo "adding Rust target $RUST_TARGET…"
  rustup target add "$RUST_TARGET"
fi
if ! cargo ndk --version >/dev/null 2>&1; then
  echo "installing cargo-ndk…"
  cargo install cargo-ndk
fi

if [ ! -x "$SDKMANAGER" ]; then
  # Homebrew's android-commandlinetools can bootstrap the SDK's own copy.
  command -v sdkmanager >/dev/null || die "Android command-line tools not found.
  Install them with: brew install --cask android-commandlinetools
  (or install Android Studio), then re-run this script."
  echo "installing Android command-line tools into $ANDROID_HOME…"
  sdkmanager --sdk_root="$ANDROID_HOME" --install "cmdline-tools;latest"
fi

missing=()
[ -x "$ADB" ] || missing+=("platform-tools")
[ -x "$EMULATOR" ] || missing+=("emulator")
[ -d "$ANDROID_HOME/platforms/android-$API" ] || missing+=("platforms;android-$API")
[ -d "$ANDROID_HOME/build-tools/35.0.0" ] || missing+=("build-tools;35.0.0")
[ -n "$(ls -A "$ANDROID_HOME/ndk" 2>/dev/null)" ] || missing+=("ndk;27.3.13750724")
[ -d "$ANDROID_HOME/system-images/android-$API/google_apis/$ABI" ] || missing+=("$IMAGE")
if [ ${#missing[@]} -gt 0 ]; then
  echo "installing Android SDK packages: ${missing[*]}"
  echo "(you may be asked to accept the Android SDK licenses)"
  "$SDKMANAGER" --install "${missing[@]}"
fi

# Gradle's Android plugin finds the SDK through local.properties.
[ -f "$REPO/android/local.properties" ] || echo "sdk.dir=$ANDROID_HOME" >"$REPO/android/local.properties"

# --- 2 + 3. emulator ------------------------------------------------------------
serial="$(running_emulator)"
if [ -n "$serial" ]; then
  step "using the emulator that's already running ($serial)"
else
  if ! "$EMULATOR" -list-avds 2>/dev/null | grep -qx "$AVD"; then
    step "creating emulator '$AVD' (Pixel 7, Android 15)"
    echo no | "$AVDMANAGER" create avd -n "$AVD" -k "$IMAGE" -d pixel_7 >/dev/null
  fi
  # Type with the Mac keyboard; draw with the Mac's GPU ("auto" can pick a
  # software renderer that makes typing crawl). If the window comes up
  # blank, re-run with COLLIE_EMULATOR_ARGS="-gpu swiftshader_indirect".
  config="$AVD_DIR/config.ini"
  for kv in hw.keyboard=yes hw.gpu.enabled=yes hw.gpu.mode=host; do
    key="${kv%%=*}"
    if grep -q "^$key=" "$config"; then
      sed -i '' "s/^$key=.*/$kv/" "$config"
    else
      echo "$kv" >>"$config"
    fi
  done

  step "booting the emulator (a window will open)"
  # shellcheck disable=SC2086 # COLLIE_EMULATOR_ARGS is a list of flags
  nohup "$EMULATOR" -avd "$AVD" ${COLLIE_EMULATOR_ARGS:-} >"$RUN_DIR/emulator.log" 2>&1 &
  emulator_pid=$!
  for _ in $(seq 1 120); do
    kill -0 "$emulator_pid" 2>/dev/null || die "the emulator exited; see $RUN_DIR/emulator.log"
    serial="$(running_emulator)"
    if [ -n "$serial" ] && [ "$("$ADB" -s "$serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; then
      break
    fi
    sleep 2
  done
  [ -n "$serial" ] || die "the emulator didn't finish booting in 4 minutes; see $RUN_DIR/emulator.log"
  echo "booted ($serial)"
fi

# --- 4. app -------------------------------------------------------------------
step "building and installing the app (the first build takes a few minutes)"
(cd "$REPO/android" && ANDROID_SERIAL="$serial" ./gradlew -q installDebug "-Pcollie.abis=$ABI")

# --- 5. demo ------------------------------------------------------------------
step "starting the demo"
"$REPO/scripts/demo-session.sh" up

cat <<EOF

All set. In the emulator, tap Connect (and allow notifications).
Then try:
  - tap an agent that "needs input" and answer it with 1 / 2 / 3
  - send an idle agent a prompt and watch it work in the terminal
  - leave the app: notifications arrive when an agent needs you

Stop everything with: scripts/try-it.sh down
EOF
