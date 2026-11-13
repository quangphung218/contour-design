#!/bin/sh
# busy34.sh BTN_X BTN_Y [WAITX WAITY] [SECS]: press a busy button in the page, Cmd-R,
# then log every dialog window (layer 8) of $PID with bounds for SECS s.
# With WAITX/WAITY (screen points), click Wait on the first "isn't responding" seen.
D="$(cd "$(dirname "$0")" && pwd)"; P=${PID:?}
ts() { python3 -c 'import time;print("%.1f"%time.time())'; }
T0=$(ts)
rel() { python3 -c "print('+%.1f'%($(ts)-$T0))"; }
"$D/go.sh" click "$1" "$2" >/dev/null; echo "$(rel) busy clicked"
sleep 1; "$D/go.sh" key 15 press cmd >/dev/null; echo "$(rel) cmd-R"
waited=""; last=""; n=$(( ${5:-40} * 2 )); i=0; shot=0
while [ $i -lt $n ]; do
  w=$("$D/cg" win "$P" | awk '$2==8 && $3==1{print $1"@"$4","$5"/"$6"x"$7}' | tr '\n' ' ')
  if [ "$w" != "$last" ]; then echo "$(rel) $(date +%T) dialogs: [$w] mouse=$("$D/cg" loc | head -1) front=$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true')"; last=$w
    if [ -n "$w" ]; then shot=$((shot+1)); screencapture -x "$D/shots/busy-$shot.png"; fi
  fi
  if [ -n "$w" ] && [ -z "$waited" ] && [ -n "$3" ]; then
    sleep 0.3; "$D/cg" click "$3" "$4" >/dev/null; waited=1; echo "$(rel) Wait clicked"
  fi
  sleep 0.5; i=$((i+1))
done
