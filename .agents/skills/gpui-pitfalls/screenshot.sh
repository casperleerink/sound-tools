#!/bin/zsh
# Usage: screenshot.sh <binary-name> <out.png> [seconds-to-wait]
# Runs a gpui binary from the shared target dir, brings it to front, captures its window, quits it.
# Needs Screen Recording permission for the terminal/agent host (System Settings > Privacy & Security).
set -e
BIN=$1; OUT=$2; WAIT=${3:-3}
DIR=$(dirname "$0")
"${CARGO_TARGET_DIR:-target}/debug/$BIN" >/tmp/$BIN.log 2>&1 &
PID=$!
sleep "$WAIT"
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true" >/dev/null 2>&1 || true
sleep 0.5
WID=$(swift "$DIR/winid.swift" "$BIN" | head -1 | cut -f1)
echo "pid=$PID window=$WID"
screencapture -x -l "$WID" "$OUT" || screencapture -x "$OUT"
kill $PID
echo "wrote $OUT"
