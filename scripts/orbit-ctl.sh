#!/bin/sh
# Token Orbit control (POSIX sh) — used by the /orbit plugin command,
# works from Git Bash on Windows and natively on macOS/Linux.
#
# Usage: orbit-ctl.sh [toggle|show|hide|quit|start]   (default: toggle)
#
# Talks to the running HUD through ~/.token-orbit/control, a file the app
# watches (reaction < 1s). No ports. If the HUD is not running, hide/quit
# are no-ops and toggle/show/start report how to launch it.

ACTION="${1:-toggle}"
DIR="$HOME/.token-orbit"
CTRL="$DIR/control"

running() {
  if command -v tasklist >/dev/null 2>&1; then
    tasklist //FI "IMAGENAME eq token-orbit.exe" 2>/dev/null | grep -qi "token-orbit.exe" && return 0
    tasklist /FI "IMAGENAME eq token-orbit.exe" 2>/dev/null | grep -qi "token-orbit.exe" && return 0
    return 1
  fi
  pgrep -x token-orbit >/dev/null 2>&1
}

launch() {
  # The exe location is user-specific; try PATH, then report.
  if command -v token-orbit >/dev/null 2>&1; then
    (token-orbit >/dev/null 2>&1 &)
    echo "[orbit] launched (from PATH)"
  elif command -v token-orbit.exe >/dev/null 2>&1; then
    (token-orbit.exe >/dev/null 2>&1 &)
    echo "[orbit] launched (from PATH)"
  else
    echo "[orbit] HUD is not running and token-orbit is not on PATH."
    echo "        Start it from your build: target/release/token-orbit.exe"
  fi
}

if running; then
  case "$ACTION" in
    start) echo "[orbit] already running" ;;
    toggle|show|hide|quit)
      mkdir -p "$DIR"
      printf '%s' "$ACTION" > "$CTRL"
      echo "[orbit] sent: $ACTION"
      ;;
    *) echo "[orbit] unknown action: $ACTION (toggle|show|hide|quit|start)"; exit 1 ;;
  esac
else
  case "$ACTION" in
    hide|quit) echo "[orbit] not running - nothing to do" ;;
    toggle|show|start) launch ;;
    *) echo "[orbit] unknown action: $ACTION (toggle|show|hide|quit|start)"; exit 1 ;;
  esac
fi
