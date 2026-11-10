#!/bin/sh
# busywait.sh: BUSY 20s, Cmd-R, Wait on the first "not responding" dialog,
# then log every dialog window (layer 8) seen for 35 s, with timestamps.
# PID must be the QA copy's pid. Wait button at screen (779,384).
D="$(cd "$(dirname "$0")" && pwd)"; P=${PID:?}
ts() { python3 -c 'import time;print("%.1f"%time.time())'; }
"$D/go.sh" click 515 289 >/dev/null; echo "$(ts) BUSY clicked"
sleep 1; "$D/go.sh" key 15 press cmd >/dev/null; echo "$(ts) cmd-R"
waited=""; last=""
i=0
while [ $i -lt 70 ]; do
  w=$("$D/cg" win "$P" | awk '$2==8 && $3==1{print $1}' | tr '\n' ' ')
  if [ "$w" != "$last" ]; then echo "$(ts) dialogs: [$w]"; last=$w; fi
  if [ -n "$w" ] && [ -z "$waited" ]; then
    sleep 0.3; "$D/cg" click 779 384; waited=1; echo "$(ts) Wait clicked on $w"
  fi
  sleep 0.5; i=$((i+1))
done
