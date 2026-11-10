#!/bin/sh
# crash2dlg.sh LABEL X Y : in crash28.html (crash27.html plus S1, a 1.2 s
# busy loop at window 945,230; preview running), crash the QA copy's own web
# content process twice within 10 s, the second time while the control at
# window X,Y (a dialog) is up. $NOT: space-separated WebContent pids that
# predate the copy.
# Crash 1: SPIN pushes exactly one WebContent not in $NOT to ~100% CPU; kill it.
# Crash 2: the one WebContent that S1 (1.2 s busy) pushes hot after the
# reload (WebKit reloads into a prewarmed process, so not the newest pid),
# killed while the dialog is up. Never touches pids in $NOT.
D="$(cd "$(dirname "$0")" && pwd)"; P=${PID:?}; NOT=${NOT:?}
ts() { python3 -c 'import time;print("%.1f"%time.time())'; }
"$D/go.sh" click 865 230 >/dev/null; sleep 1.5
hot=$(ps -axo pid=,%cpu=,comm= | awk -v not=" $NOT " '/com.apple.WebKit.WebContent/ && $2>80 && index(not, " "$1" ")==0 {print $1}')
[ "$(echo "$hot" | grep -c .)" = 1 ] || { echo "refused: hot [$hot]"; exit 3; }
kill -9 "$hot" && echo "$(ts) crash 1: killed $hot"
sleep 2.5
"$D/go.sh" click 945 230 >/dev/null; sleep 0.5
new=$(ps -axo pid=,%cpu=,comm= | awk -v not=" $NOT " '/com.apple.WebKit.WebContent/ && $2>50 && index(not, " "$1" ")==0 {print $1}')
[ "$(echo "$new" | grep -c .)" = 1 ] || { echo "refused: S1 hot [$new]"; exit 3; }
sleep 1
echo "$(ts) own WebContent after reload: $new"
"$D/go.sh" click "$2" "$3" >/dev/null; sleep 1.2
echo "$(ts) windows with $1 up:"; "$D/cg" win "$P" | awk '$3==1 && $2!=0' | grep -v '^flags'
screencapture -x -R40,30,1360,810 "$D/shots/$1-open.png"
kill -9 "$new" && echo "$(ts) crash 2: killed $new"
last="x"; i=0
while [ $i -lt 20 ]; do
  w=$("$D/cg" win "$P" | awk '$3==1 && $2!=0' | grep -v '^flags' | awk '{print $1"/"$2}' | tr '\n' ' ')
  if [ "$w" != "$last" ]; then echo "$(ts) extra windows: [$w]"; last=$w; fi
  sleep 0.5; i=$((i+1))
done
screencapture -x -R40,30,1360,810 "$D/shots/$1-after.png"
ps -p "$P" >/dev/null && echo "app alive" || echo "APP GONE"
