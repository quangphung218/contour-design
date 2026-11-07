#!/bin/sh
# cc.sh dx dy [scroll N] : click (or scroll) at coords of the 1400-px-wide view (sv.png) of $PID's window,
# only when $PID is frontmost and its window is the topmost Contour window.
P=${PID:?}; D="$(dirname "$0")"  # cg: swiftc -O cg.swift -o cg
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $P) to true" >/dev/null
sleep 0.3
F=$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true')
[ "$F" = "$P" ] || { echo "frontmost is $F, not $P: refused"; exit 3; }
W=$("$D/cg" win "$P" | awk '$3==1 && $NF=="Contour"{print $1; exit}')
TOP=$("$D/cg" front | head -1)
[ "$TOP" = "$W" ] || { echo "top window $TOP is not mine ($W): refused"; exit 3; }
G=$("$D/cg" win "$P" | awk '$3==1 && $NF=="Contour"{print $4, $5, $6; exit}')
set -- "$1" "$2" "$3" "$4" $G
f=$(echo "scale=4; $7/1400" | bc)
x=$(echo "$5 + $1*$f" | bc); y=$(echo "$6 + $2*$f" | bc)
if [ "$3" = scroll ]; then "$D/cg" scroll "$x" "$y" "$4"; elif [ "$3" = cmd ]; then "$D/cg" cmdclick "$x" "$y"; else "$D/cg" click "$x" "$y"; fi
