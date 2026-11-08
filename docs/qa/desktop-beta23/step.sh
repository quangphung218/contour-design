#!/bin/sh
# step.sh LABEL WAIT go.sh-args... : runs one action, prints how many opens it added
D="$(cd "$(dirname "$0")" && pwd)"; L=$1; W=$2; shift 2
b=$(wc -l < "$D/data/opens.log")
PID=${PID:?} "$D/go.sh" "$@" >/dev/null || exit 3
sleep "$W"
a=$(wc -l < "$D/data/opens.log")
echo "$L: +$((a-b)) $(tail -n $((a-b)) "$D/data/opens.log" | awk '{print $2}' | tr '\n' ' ')"
