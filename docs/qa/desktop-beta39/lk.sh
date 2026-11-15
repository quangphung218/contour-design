#!/bin/sh
# lk.sh LABEL LOCALE|- : the QA copy's data folder held by `od daemon start`
# (port 7499), then the copy opened: the dialog's window, its words (via
# Accessibility), a screenshot, then its button pressed (AX, no HID event).
# LOCALE: written to app-config.json as "locale"; "-" removes the file.
S=$(cd "$(dirname "$0")" && pwd)
D="$S/home/Library/Application Support/contour"
OD=${OD_BIN:?od binary}
idle=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')
[ "$idle" -lt 60 ] && { echo "ABORT idle=$idle"; exit 3; }
for p in $("$S/qa.sh" pids); do "$S/cg" quit "$p"; done
n=0; until [ -z "$("$S/qa.sh" pids)" ] || [ $n -ge 40 ]; do sleep 0.5; n=$((n+1)); done
[ -n "$("$S/qa.sh" pids)" ] && { echo "copy did not quit"; exit 4; }
if [ "$2" = "-" ]; then rm -f "$D/app-config.json"; else printf '{"locale":"%s"}\n' "$2" > "$D/app-config.json"; fi
OD_DATA_DIR="$D" HOME="$S/home" "$OD" daemon start --port 7499 > "$S/od-$1.txt" 2>&1 &
odpid=$!
n=0; until curl -s -o /dev/null http://127.0.0.1:7499/ || [ $n -ge 60 ]; do sleep 0.5; n=$((n+1)); done
[ -n "$APP_HOLDER" ] && cp "$D/daemon.lock" "$D/daemon.lock.app" && echo "lock url now the app's: $(cat "$D/daemon.lock.app")"
echo "== $1 locale=$2 od=$odpid idle=$idle answers=$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:7499/)"
t0=$(date +%s)
"$S/qa.sh" open
pid=""; n=0; until [ -n "$pid" ] || [ $n -ge 40 ]; do sleep 0.25; pid=$("$S/qa.sh" pids | head -1); n=$((n+1)); done
P="first process whose unix id is $pid"
U='process "UserNotificationCenter"'
n=0; until "$S/allwin" | grep -q UserNotificationCenter || [ $n -ge 100 ]; do sleep 0.25; n=$((n+1)); done
echo "dialog after $(( $(date +%s) - t0 )) s, pid=$pid, front=$(lsappinfo info -only name "$(lsappinfo front)")"
sleep 0.5
osascript -e "tell application \"System Events\" to tell ($U) to get value of every static text of window 1"
osascript -e "tell application \"System Events\" to tell ($U) to get name of every button of window 1"
wid=$("$S/allwin" | awk '$3=="UserNotificationCenter" {print $1; exit}')
screencapture -o -l "$wid" "$S/$1.png" && echo "shot $S/$1.png (window $wid)"
osascript -e "tell application \"System Events\" to tell ($U) to click button 1 of window 1"
n=0; until [ -z "$("$S/qa.sh" pids)" ] || [ $n -ge 20 ]; do sleep 0.25; n=$((n+1)); done
echo "after the button: pids=$("$S/qa.sh" pids | tr '\n' ' ') (empty = quit) od-still-answers=$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:7499/)"
grep -a "data folder is held" "$D/logs/daemon.log" | tail -1
echo "odpid=$odpid"
