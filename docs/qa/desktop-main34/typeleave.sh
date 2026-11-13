#!/bin/sh
# typeleave.sh X Y TEXT KEYCODE MOD : click the field at window X,Y, go to its end (Cmd-Down),
# type TEXT, then at once press KEYCODE with MOD (ctrl|cmd|none). Prints the ms between
# the last typed letter and the leave key.
D="$(cd "$(dirname "$0")" && pwd)"; P=${PID:?}
"$D/go.sh" click "$1" "$2" >/dev/null || exit 3
"$D/cg" key 125 press cmd >/dev/null
"$D/cg" type "$3" >/dev/null
a=$(python3 -c 'import time;print(int(time.time()*1000))')
if [ "$5" = click ]; then "$D/cg" click $((40+${4%,*})) $((30+${4#*,})) >/dev/null; elif [ "$5" = none ]; then "$D/cg" key "$4" press >/dev/null; else "$D/cg" key "$4" press "$5" >/dev/null; fi
b=$(python3 -c 'import time;print(int(time.time()*1000))')
echo "leave key sent $((b-a)) ms after the last letter at $(date +%T)"
