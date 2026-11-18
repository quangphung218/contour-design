#!/usr/bin/env bash
# Runs a CI setup command again when it fails: downloads and package
# installs (apt, brew, choco, npm, Playwright browsers), which fail now and
# then on a mirror or the network, not on our code. Up to CI_RETRY_TRIES
# tries (default 3), waiting CI_RETRY_SECONDS (default 15) times the try
# number between them. Tests never go through it: a red test stays red.
# Usage: scripts/ci-retry.sh CMD [ARG…]
set -uo pipefail
[ $# -gt 0 ] || { echo "usage: scripts/ci-retry.sh CMD [ARG…]" >&2; exit 2; }
tries="${CI_RETRY_TRIES:-3}"
pause="${CI_RETRY_SECONDS:-15}"
try=1
while :; do
  "$@"
  rc=$?
  [ "$rc" -eq 0 ] && exit 0
  if [ "$try" -ge "$tries" ]; then
    echo "ci-retry: '$*' failed $tries times (last exit $rc)" >&2
    exit "$rc"
  fi
  echo "::warning::'$*' failed (exit $rc), try $try of $tries; trying again in $((try * pause)) s"
  sleep $((try * pause))
  try=$((try + 1))
done
