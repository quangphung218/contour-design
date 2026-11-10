#!/bin/sh
# crashloop.sh: in crash27.html (C27 open), crash the QA copy's own web
# content process twice within 10 s, each pid picked as the one WebContent
# that SPIN pushes to ~100% CPU (never a pid in $NOT, a space-separated list). Logs times.
D="$(cd "$(dirname "$0")" && pwd)"; P=${PID:?}; NOT=${NOT:?}
ts() { python3 -c 'import time;print("%.1f"%time.time())'; }
hotpid() {
  "$D/go.sh" click 865 230 >/dev/null; sleep 1.5
  ps -axo pid=,%cpu=,comm= | awk -v not=" $NOT " '/com.apple.WebKit.WebContent/ && $2>80 && index(not, " "$1" ")==0 {print $1}'
}
for n in 1 2; do
  h=$(hotpid); c=$(echo "$h" | grep -c .)
  [ "$c" = 1 ] || { echo "refused: $c hot pids [$h]"; exit 3; }
  kill -9 "$h" && echo "$(ts) crash $n: killed $h"
  [ $n = 1 ] && sleep 2.5
done
sleep 5
"$D/go.sh" shot crashloop-after
ps -p "$P" >/dev/null && echo "app alive" || echo "APP GONE"
