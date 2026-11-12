#!/bin/sh
# open.sh NAME : opens the project named NAME from Projects (first card after search)
PID=${PID:?}; D="$(cd "$(dirname "$0")" && pwd)"
"$D/go.sh" key 53 press >/dev/null
"$D/go.sh" click ${NAVX:-28} ${NAVY:-212} >/dev/null; sleep 1.2
"$D/go.sh" click 652 150 >/dev/null; sleep 0.3
"$D/go.sh" key 0 press cmd >/dev/null
"$D/go.sh" type "$1" >/dev/null; sleep 1.5
"$D/go.sh" click 440 300 >/dev/null; sleep 3
