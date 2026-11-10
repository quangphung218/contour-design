#!/bin/sh
# crashdlg.sh LABEL X Y : in crash27.html, find the QA copy's own WebContent
# pid (SPIN 8s at window 865,230 pushes exactly one new WebContent to ~100%
# CPU), open the control at window X,Y, then SIGKILL that pid only while the
# dialog/panel is up, and log the copy's windows for 12 s.
# Never touches pids in $NOT (WebContent processes that predate the copy).
D="$(cd "$(dirname "$0")" && pwd)"; P=${PID:?}; NOT=${NOT:-53633}
ts() { python3 -c 'import time;print("%.1f"%time.time())'; }
"$D/go.sh" click 865 230 >/dev/null; sleep 2.5
hot=$(ps -axo pid=,%cpu=,comm= | awk -v not="$NOT" '/com.apple.WebKit.WebContent/ && $2>80 && $1!=not {print $1}')
n=$(echo "$hot" | grep -c .)
[ "$n" = 1 ] || { echo "refused: $n hot WebContent pids [$hot]"; exit 3; }
echo "$(ts) own WebContent pid $hot"
i=0; while [ "$(ps -o %cpu= -p "$hot" | cut -d. -f1)" -gt 50 ] && [ $i -lt 20 ]; do sleep 0.5; i=$((i+1)); done
"$D/go.sh" wins | awk '$3==1' > "$D/w0.txt"
"$D/go.sh" click "$2" "$3" >/dev/null; sleep 2
echo "$(ts) windows with $1 up:"; "$D/cg" win "$P" | awk '$3==1' | grep -v '^flags'
screencapture -x -R40,30,1360,810 "$D/shots/$1-open.png"
kill -9 "$hot" && echo "$(ts) killed $hot"
last=""; i=0
while [ $i -lt 24 ]; do
  w=$("$D/cg" win "$P" | awk '$3==1 && $2!=0 || ($3==1 && $NF!="Contour" && $2==0)' | grep -v '^flags' | awk '{print $1"/"$2}' | tr '\n' ' ')
  if [ "$w" != "$last" ]; then echo "$(ts) extra windows: [$w]"; last=$w; fi
  sleep 0.5; i=$((i+1))
done
screencapture -x -R40,30,1360,810 "$D/shots/$1-after.png"
ps -p "$P" >/dev/null && echo "app alive"
