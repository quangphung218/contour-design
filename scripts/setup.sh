#!/usr/bin/env bash
# Checks the toolchain for building, testing and bundling Contour. Installs only
# project-local pieces (the rustup wasm target, the e2e npm packages) and
# prints the command for anything system-wide that is missing.
# Usage: scripts/setup.sh   (or `make setup`); exits 1 when a required tool is missing.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
missing=0

ok() { printf '  \033[32mok\033[0m       %-22s %s\n' "$1" "$2"; }
miss() { printf '  \033[31mmissing\033[0m  %-22s run: %s\n' "$1" "$2"; missing=1; }
opt() { printf '  \033[33moptional\033[0m %-22s run: %s\n' "$1" "$2"; }

# version_ge 1.91.1 1.85 -> true when $1 >= $2.
version_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }

echo "Required"
if command -v rustup >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
  rustc_v="$(rustc --version | awk '{print $2}')"
  if version_ge "$rustc_v" 1.85; then ok rust "$rustc_v"; else miss "rust >= 1.85 ($rustc_v)" "rustup update stable"; fi
  if rustup target list --installed | grep -qx wasm32-unknown-unknown; then
    ok wasm32-unknown-unknown "installed"
  else
    echo "  adding rustup target wasm32-unknown-unknown"
    rustup target add wasm32-unknown-unknown >/dev/null && ok wasm32-unknown-unknown "added" \
      || miss wasm32-unknown-unknown "rustup target add wasm32-unknown-unknown"
  fi
  for c in rustfmt clippy; do
    if rustup component list --installed | grep -q "^$c"; then ok "$c" "installed"; else miss "$c" "rustup component add $c"; fi
  done
else
  miss rustup "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi
if command -v trunk >/dev/null 2>&1; then ok trunk "$(trunk --version | awk '{print $2}')"; else miss trunk "cargo install trunk --locked  (or: brew install trunk)"; fi
if command -v node >/dev/null 2>&1; then
  node_v="$(node --version | tr -d v)"
  if version_ge "$node_v" 20; then ok node "$node_v"; else miss "node >= 20 ($node_v)" "brew install node"; fi
else
  miss node "brew install node  (or https://nodejs.org)"
fi
if command -v cargo-tauri >/dev/null 2>&1; then
  ok tauri-cli "cargo tauri $(cargo tauri --version | awk '{print $2}')"
elif command -v npx >/dev/null 2>&1; then
  ok tauri-cli "via npx @tauri-apps/cli (pinned in Makefile)"
else
  miss tauri-cli "brew install node  (the Makefile runs the CLI with npx)"
fi

echo "Tests and CI"
e2e="$root/crates/od-ui/e2e"
if [ -d "$e2e/node_modules/playwright" ]; then
  ok "e2e npm packages" "installed"
elif command -v npm >/dev/null 2>&1; then
  echo "  installing e2e npm packages (npm ci in crates/od-ui/e2e)"
  (cd "$e2e" && npm ci --no-audit --no-fund >/dev/null) && ok "e2e npm packages" "installed" \
    || miss "e2e npm packages" "cd crates/od-ui/e2e && npm ci"
fi
browsers_ok=0
if [ -d "$e2e/node_modules/playwright" ]; then
  dirs="$(cd "$e2e" && npx --no-install playwright install --dry-run chromium 2>/dev/null | awk '/Install location/ {print $3}')"
  browsers_ok=1
  [ -n "$dirs" ] || browsers_ok=0
  for d in $dirs; do [ -d "$d" ] || browsers_ok=0; done
fi
if [ "$browsers_ok" -eq 1 ]; then ok "playwright chromium" "installed"; else miss "playwright chromium" "cd crates/od-ui/e2e && npx playwright install chromium"; fi
if command -v cargo-deny >/dev/null 2>&1; then ok cargo-deny "$(cargo deny --version | awk '{print $2}')"; else opt cargo-deny "cargo install cargo-deny --locked  (or: brew install cargo-deny)"; fi
if command -v actionlint >/dev/null 2>&1; then ok actionlint "$(actionlint -version | head -n1)"; else opt actionlint "brew install actionlint"; fi

echo "Runtime (export)"
chrome=""
for p in "${OD_CHROME_PATH:-}" "/Applications/Google Chrome.app" "/Applications/Microsoft Edge.app" "/Applications/Chromium.app"; do
  [ -n "$p" ] && [ -e "$p" ] && { chrome="$p"; break; }
done
[ -z "$chrome" ] && for c in google-chrome chromium chromium-browser microsoft-edge; do
  command -v "$c" >/dev/null 2>&1 && { chrome="$(command -v "$c")"; break; }
done
if [ -n "$chrome" ]; then ok chrome "$chrome"; else opt "chrome/edge/chromium" "install Chrome (needed for PDF, image, PPTX, MP4 export)"; fi
if command -v ffmpeg >/dev/null 2>&1; then ok ffmpeg "$(command -v ffmpeg)"; else opt ffmpeg "brew install ffmpeg  (MP4 export only)"; fi

if [ "$missing" -ne 0 ]; then
  echo "Some required tools are missing; run the commands above, then make setup again."
  exit 1
fi
echo "Toolchain ready. Next: make run"
