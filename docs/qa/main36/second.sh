#!/bin/sh
# second.sh LABEL [APP] [FRONT] : opens a second process of the QA copy
# (open -n, own HOME) with FRONT (an app name, default Finder) frontmost,
# and watches for 6 s: frontmost pid, topmost window owner, the first
# copy's window. Then the QA pids and the log lines about it.
S=$(cd "$(dirname "$0")" && pwd)
D="$S/home/Library/Application Support/contour"
first=$("$S/qa.sh" pids | head -1)
osascript -e "tell application \"${3:-Finder}\" to activate"; sleep 0.5
echo "== $1 first=$first"
"$S/cg" watch "$first" 6 | grep -v '^flags' &
w=$!
sleep 0.3
"$S/qa.sh" opennew "${2:-ContourQA.app}"
wait $w
echo "pids now: $("$S/qa.sh" pids | tr '\n' ' ')"
grep -a "opened again\|data folder is held\|daemon did not start" "$D/logs/daemon.log" | tail -2
