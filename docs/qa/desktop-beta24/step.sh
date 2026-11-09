#!/bin/sh
# step.sh LABEL WAIT go.sh-args... : runs one action, prints how many opens it added to $QA_DATA/opens.log
D="$(cd "$(dirname "$0")" && pwd)"; L=$1; W=$2; shift 2
O="${QA_DATA:?}/opens.log"; touch "$O"
b=$(wc -l < "$O")
PID=${PID:?} "$D/go.sh" "$@" >/dev/null || exit 3
sleep "$W"
a=$(wc -l < "$O")
echo "$L: +$((a-b)) $(tail -n $((a-b)) "$O" | awk '{print $2}' | tr '\n' ' ')"
