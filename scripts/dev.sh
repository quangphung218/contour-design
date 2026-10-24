#!/usr/bin/env bash
# UI development loop: the `od` daemon on :7456 (OD_PORT) and `trunk serve`
# on :8080 with hot reload. Trunk proxies /api/ to the daemon
# (crates/od-ui/Trunk.toml), so open http://127.0.0.1:8080.
# Ctrl-C stops both. Usage: scripts/dev.sh   (or `make dev`)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
port="${OD_PORT:-7456}"
if [ "$port" != 7456 ]; then
  echo "note: trunk proxies to :7456 (crates/od-ui/Trunk.toml); OD_PORT=$port needs a matching proxy" >&2
fi

cargo build -p od-cli --bin od --manifest-path "$root/Cargo.toml"
od="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" \
  | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')/debug/od"

RUST_LOG="${RUST_LOG:-info}" "$od" daemon start --port "$port" &
daemon=$!
trap 'kill "$daemon" 2>/dev/null; wait "$daemon" 2>/dev/null || true' EXIT INT TERM

for _ in $(seq 1 100); do
  curl -fsS "http://127.0.0.1:$port/api/health" >/dev/null 2>&1 && break
  kill -0 "$daemon" 2>/dev/null || { echo "daemon exited; see the log above" >&2; exit 1; }
  sleep 0.1
done
echo "daemon: http://127.0.0.1:$port   ui (hot reload): http://127.0.0.1:8080"
cd "$root/crates/od-ui" && trunk serve
