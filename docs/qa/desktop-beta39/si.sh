#!/bin/sh
# si.sh LABEL STATE [APP] : puts the QA copy's window in STATE (shown, closed,
# minimised, hidden) through Accessibility (no HID events, so HIDIdleTime
# still tells whether a person is at the Mac), brings Finder to the front,
# then a second launch (open -n, own HOME) of APP, watched for 6 s.
S=$(cd "$(dirname "$0")" && pwd)
D="$S/home/Library/Application Support/contour"
idle=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')
[ "$idle" -lt 60 ] && { echo "ABORT idle=$idle"; exit 3; }
first=$("$S/qa.sh" pids | head -1)
P="first process whose unix id is $first"
case "$2" in
  closed) osascript -e "tell application \"System Events\" to tell ($P) to click (first button of window 1 whose subrole is \"AXCloseButton\")" ;;
  minimised) osascript -e "tell application \"System Events\" to tell ($P) to set value of attribute \"AXMinimized\" of window 1 to true" ;;
  hidden) osascript -e "tell application \"System Events\" to set visible of ($P) to false" ;;
esac
sleep 1
osascript -e 'tell application "Finder" to activate'; sleep 0.7
echo "== $1 state=$2 first=$first idle=$idle"
echo "before: front=$(lsappinfo info -only pid "$(lsappinfo front)") wins=$(osascript -e "tell application \"System Events\" to tell ($P) to get {name, value of attribute \"AXMinimized\"} of every window" 2>&1) visible=$(osascript -e "tell application \"System Events\" to get visible of ($P)")"
"$S/cg" watch "$first" 6 > "$S/w.txt" &
sleep 0.3
"$S/qa.sh" opennew "${3:-ContourQA.app}"
wait
cat "$S/w.txt"
echo "after: front=$(lsappinfo info -only pid "$(lsappinfo front)") wins=$(osascript -e "tell application \"System Events\" to tell ($P) to get {name, value of attribute \"AXMinimized\"} of every window" 2>&1) visible=$(osascript -e "tell application \"System Events\" to get visible of ($P)")"
echo "pids now: $("$S/qa.sh" pids | tr '\n' ' ') daemons=$(lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | grep -c od-deskto)"
grep -a "opened again\|data folder is held\|daemon did not start" "$D/logs/daemon.log" | tail -1
