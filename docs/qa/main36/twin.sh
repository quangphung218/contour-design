#!/bin/sh
# twin.sh : with no QA copy running, two `open -n` at once; logs for 20 s
# the QA pids and the socket, then the log lines about the lock.
S=$(cd "$(dirname "$0")" && pwd)
D="$S/home/Library/Application Support/contour"
for p in $("$S/qa.sh" pids); do "$S/cg" quit "$p" >/dev/null; done
n=0; while [ -n "$("$S/qa.sh" pids)" ] && [ $n -lt 40 ]; do sleep 0.5; n=$((n+1)); done
lines=$(wc -l < "$D/logs/daemon.log")
"$S/qa.sh" opennew & "$S/qa.sh" opennew & wait
i=0; while [ $i -lt 20 ]; do
  echo "+$i pids=[$("$S/qa.sh" pids | tr '\n' ' ')] sock=$(ls /private/tmp/ | grep -c qa_si.sock)"
  sleep 1; i=$((i+1))
done | uniq -f1
tail -n +"$((lines+1))" "$D/logs/daemon.log" | grep -a "opened again\|held\|listening\|did not start" | cut -c12-120
