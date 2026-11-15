#!/bin/sh
# own.sh LABEL CMD... : with Finder in front, runs CMD (a second launch of the
# owner's Contour) and watches pid 8954 for 6 s. Aborts if someone is at the Mac.
S=$(cd "$(dirname "$0")" && pwd)
idle=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')
[ "$idle" -lt 60 ] && { echo "ABORT idle=$idle"; exit 3; }
label=$1; shift
osascript -e 'tell application "Finder" to activate'; sleep 0.7
echo "== $label idle=$idle front-before=$(lsappinfo info -only pid "$(lsappinfo front)")"
"$S/cg" watch 8954 6 > "$S/w.txt" &
sleep 0.3; "$@"; wait
cat "$S/w.txt"
echo "front-after=$(lsappinfo info -only pid "$(lsappinfo front)") pids=$(pgrep -f 'MacOS/od-desktop' | tr '\n' ' ') listeners=$(lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | grep -c od-deskto)"
