#!/bin/sh
# go.sh shot NAME | click X Y | cmdclick X Y | key CODE MODE [FLAG] | hold CODE N | drag X1 Y1 X2 Y2
# X/Y are window-local points of $PID's main "Contour" window. Refuses unless $PID is frontmost.
P=${PID:?}; D="$(cd "$(dirname "$0")" && pwd)"
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $P) to true" >/dev/null
sleep 0.3
F=$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true')
[ "$F" = "$P" ] || { echo "frontmost is $F, not $P: refused"; exit 3; }
set -- $(echo "$@")
G=$("$D/cg" win "$P" | awk '$3==1 && $NF=="Contour"{print $1, $4, $5; exit}')
W=${G%% *}; R=${G#* }; WX=${R% *}; WY=${R#* }
case "$1" in
  shot) screencapture -x -o -l "$W" "$D/shots/$2.png" && sips -Z 1360 "$D/shots/$2.png" >/dev/null && echo "$D/shots/$2.png";;
  click|cmdclick) "$D/cg" "$1" $(echo "$WX+$2" | bc) $(echo "$WY+$3" | bc);;
  drag) "$D/cg" drag $(echo "$WX+$2" | bc) $(echo "$WY+$3" | bc) $(echo "$WX+$4" | bc) $(echo "$WY+$5" | bc);;
  key) "$D/cg" key "$2" "$3" "$4";;
  scroll) "$D/cg" scroll $(echo "$WX+$2" | bc) $(echo "$WY+$3" | bc) "$4";;
  hold) "$D/cg" hold "$2" "$3";;
esac
