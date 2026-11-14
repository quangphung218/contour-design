#!/bin/sh
# quitreopen.sh DELAY : starts a stubborn agent run (ignores SIGTERM, so
# quitting waits the 8 s stop limit), quits the QA copy, opens it again
# DELAY s later (open -n), then logs every 0.5 s for 25 s: QA pids, whether
# a daemon answers, and the log lines about the lock and second launches.
S=$(cd "$(dirname "$0")" && pwd)
D="$S/home/Library/Application Support/contour"
B=$(cat "$D/daemon.lock")
id=$(curl -s -X POST -H 'content-type: application/json' -d '{"name":"Quit test"}' "$B/api/projects" | python3 -c 'import sys,json;d=json.load(sys.stdin);print((d.get("project") or d)["id"])')
curl -s -o /dev/null -X POST -H 'content-type: application/json' -d "{\"agentId\":\"claude\",\"projectId\":\"$id\",\"message\":\"qa-stubborn go\"}" "$B/api/runs"
n=0; until pgrep -f "qa/mock/claude" >/dev/null || [ $n -ge 20 ]; do sleep 0.25; n=$((n+1)); done
echo "agent: $(pgrep -f 'qa/mock/claude' | tr '\n' ' ') first: $("$S/qa.sh" pids | tr '\n' ' ')"
lines=$(wc -l < "$D/logs/daemon.log")
t0=$(python3 -c 'import time;print(time.time())')
"$S/cg" quit "$("$S/qa.sh" pids | head -1)" | head -1 &
sleep "$1"
"$S/qa.sh" opennew
i=0
while [ $i -lt ${2:-50} ]; do
  now=$(python3 -c "import time;print('%.1f'%(time.time()-$t0))")
  url=$(cat "$D/daemon.lock" 2>/dev/null)
  ok=$(curl -s -m 0.4 -o /dev/null -w '%{http_code}' "$url/api/version")
  echo "+$now pids=[$("$S/qa.sh" pids | tr "\n" " ")] agent=[$(pgrep -f qa/mock/claude | tr "\n" " ")] lock=$url api=$ok"
  sleep 0.5; i=$((i+1))
done | uniq -f1
tail -n +"$((lines+1))" "$D/logs/daemon.log" | grep -a "opened again\|held\|listening\|did not start\|stop" | cut -c1-160
