#!/usr/bin/env bash
# Smoke test of a Linux package on a clean Ubuntu (22.04 or 24.04), as root
# in a throwaway container or VM: installs the .deb with apt (or runs the
# AppImage), opens Contour under Xvfb as an ordinary user started the way
# the GNOME app menu starts it (bare PATH, no terminal), waits for the
# in-process daemon's /api/health to report the expected version, checks
# that an agent CLI set up by ~/.bashrc (as nvm does) is found and runs,
# and saves a screenshot of the window and the app log in the out dir.
# Usage: scripts/linux-smoke.sh <Contour_*.deb | Contour_*.AppImage> [out-dir]
# EXPECTED_VERSION overrides the version read from Cargo.toml; SMOKE_EXPORT=1
# also installs Google Chrome and checks a PDF export.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
pkg="$(realpath "${1:?usage: scripts/linux-smoke.sh <package> [out-dir]}")"
out="$(mkdir -p "${2:-smoke-out}" && realpath "${2:-smoke-out}")"
# Emulated or loaded machines start slowly; the daemon gets this long.
timeout="${SMOKE_TIMEOUT:-120}"
expected="${EXPECTED_VERSION:-$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[" ]/,"");sub(/version=/,"");print;exit}' "$here/../Cargo.toml")}"
user=smoke
home="/home/$user"
data="$home/.local/share/contour"
failures=0
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; [ -n "${2:-}" ] && printf '%s\n' "$2" | sed 's/^/     /'; failures=$((failures + 1)); }
stop() { fail "$1" "${2:-}"; finish; }
finish() {
  pkill -KILL -u "$user" 2>/dev/null
  cp -r "$data/logs" "$out/" 2>/dev/null
  [ "$failures" -eq 0 ] && echo "linux smoke ($(basename "$pkg")): all checks passed" \
    || { echo "linux smoke ($(basename "$pkg")): $failures failed"; exit 1; }
  exit 0
}

[ -f "$pkg" ] || stop "no package at $1"
[ -n "$expected" ] || stop "no expected version (Cargo.toml or EXPECTED_VERSION)"
. /etc/os-release
echo "Ubuntu $VERSION_ID, package $(basename "$pkg"), expecting $expected"

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
# Test tools only; the app's own libraries must come from the package.
apt-get install -y -qq --no-install-recommends xvfb xauth x11-utils imagemagick dbus \
  curl jq ca-certificates passwd > "$out/apt-tools.log" 2>&1 || stop "installing the test tools" "$(tail -5 "$out/apt-tools.log")"

case "$pkg" in
  *.deb)
    if apt-get install -y "$pkg" > "$out/apt-install.log" 2>&1; then
      ok "apt install ./$(basename "$pkg")"
    else
      stop "apt install ./$(basename "$pkg")" "$(tail -15 "$out/apt-install.log")"
    fi
    version="$(dpkg-query -W -f '${Version}' contour 2>/dev/null)"
    [ "$version" = "$expected" ] && ok "package version $version" || fail "package version is '$version', want $expected"
    entry=/usr/share/applications/Contour.desktop
    if [ -f "$entry" ]; then
      ok "menu entry $entry"
      grep -qx 'Name=Contour' "$entry" && ok "menu entry is named Contour" || fail "menu entry name" "$(grep '^Name=' "$entry")"
      grep -qE '^Categories=.*(Graphics|Development);' "$entry" && ok "menu entry has a category" \
        || fail "menu entry has no Graphics or Development category" "$(grep '^Categories' "$entry")"
      exec_bin="$(sed -n 's/^Exec=\([^ ]*\).*/\1/p' "$entry")"
      app="$(command -v "$exec_bin")" && ok "Exec=$exec_bin is on PATH ($app)" || fail "Exec=$exec_bin is not on PATH"
      icon="$(sed -n 's/^Icon=//p' "$entry")"
      ls /usr/share/icons/hicolor/*/apps/"$icon".png > /dev/null 2>&1 && ok "icon $icon is installed" || fail "no icon $icon in hicolor"
    else
      fail "no menu entry at $entry" "$(dpkg -L contour | grep '\.desktop$')"
    fi
    [ -n "${app:-}" ] || stop "no app to start"
    ;;
  *.AppImage)
    # The AppImage needs FUSE 2: libfuse2 on 22.04, libfuse2t64 on 24.04.
    # Without /dev/fuse (most containers) it unpacks itself instead.
    fuse=libfuse2
    [ "${VERSION_ID%%.*}" -ge 24 ] && fuse=libfuse2t64
    # A desktop has GTK and the GL stack (GL, EGL, GLES 2: WebKitGTK loads
    # libGLESv2 from the host); the AppImage brings WebKitGTK itself.
    apt-get install -y -qq --no-install-recommends "$fuse" fuse3 libgtk-3-0 libgl1 libegl1 libgles2 \
      fonts-dejavu-core > "$out/apt-install.log" 2>&1 || stop "installing $fuse and the desktop libraries" "$(tail -5 "$out/apt-install.log")"
    ok "installed $fuse"
    app="/opt/$(basename "$pkg")"
    install -m 0755 "$pkg" "$app"
    if [ -c /dev/fuse ] && "$app" --appimage-version > /dev/null 2>&1; then
      ok "the AppImage mounts with FUSE"
    else
      export APPIMAGE_EXTRACT_AND_RUN=1
      echo "note no usable /dev/fuse here: APPIMAGE_EXTRACT_AND_RUN=1"
    fi
    ;;
  *) stop "not a .deb or .AppImage: $pkg" ;;
esac

# PDF export drives Google Chrome (its own .deb: Ubuntu's chromium is a snap).
if [ "${SMOKE_EXPORT:-0}" = 1 ]; then
  curl -fsSL -o /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
    && apt-get install -y /tmp/chrome.deb > "$out/apt-chrome.log" 2>&1 \
    || fail "installing Google Chrome" "$(tail -5 "$out/apt-chrome.log" 2>/dev/null)"
fi

# An ordinary user whose agent CLI is only on the PATH ~/.bashrc sets up
# (like nvm), not on the bare PATH the app menu gives the app.
id "$user" > /dev/null 2>&1 || useradd -m -s /bin/bash "$user"
agent_dir="$home/.nvm/versions/node/v22.11.0/bin"
install -d -o "$user" -g "$user" "$agent_dir" "$home/.local/share"
install -m 0755 -o "$user" -g "$user" "$here/../crates/od-ui/e2e/mock-agent/claude" "$agent_dir/claude"
echo "export PATH=\"$agent_dir:\$PATH\"" >> "$home/.bashrc"

Xvfb :99 -screen 0 1440x900x24 -nolisten tcp > "$out/xvfb.log" 2>&1 &
for _ in $(seq 1 50); do [ -e /tmp/.X11-unix/X99 ] && break; sleep 0.2; done
[ -e /tmp/.X11-unix/X99 ] || stop "Xvfb did not start" "$(cat "$out/xvfb.log")"
runtime="/tmp/runtime-$user"
install -d -m 0700 -o "$user" -g "$user" "$runtime"
runuser -u "$user" -- env -i HOME="$home" USER="$user" LOGNAME="$user" SHELL=/bin/bash \
  PATH=/usr/local/bin:/usr/bin:/bin DISPLAY=:99 XDG_RUNTIME_DIR="$runtime" LANG=C.UTF-8 \
  ${APPIMAGE_EXTRACT_AND_RUN:+APPIMAGE_EXTRACT_AND_RUN=1} \
  dbus-run-session -- "$app" > "$out/app.log" 2>&1 &
app_pid=$!

# The daemon writes its URL into daemon.lock once it listens.
url=""
for _ in $(seq 1 "$timeout"); do
  url="$(cat "$data/daemon.lock" 2>/dev/null)"
  [ -n "$url" ] && curl -fsS "$url/api/health" > "$out/health.json" 2>/dev/null && break
  kill -0 "$app_pid" 2>/dev/null || stop "the app exited during startup" "$(tail -20 "$out/app.log")"
  url=""
  sleep 1
done
[ -n "$url" ] || stop "no /api/health within ${timeout}s" "$(tail -20 "$out/app.log")"
got="$(jq -r .version "$out/health.json")"
[ "$got" = "$expected" ] && ok "/api/health at $url reports $got" || fail "/api/health reports '$got', want $expected"

# The window shows what / serves: the UI, not "UI bundle not found".
curl -fsS "$url/" > "$out/index.html" 2>&1; grep -q '<title>Contour</title>' "$out/index.html" \
  && ok "the window's page is the UI bundle" || fail "GET / is not the UI bundle" "$(head -c 300 "$out/index.html")"

curl -fsS "$url/api/agents" > "$out/agents.json"
claude_path="$(jq -r '.agents[] | select(.id == "claude" and .available) | .path // empty' "$out/agents.json")"
[ "$claude_path" = "$agent_dir/claude" ] && ok "finds the agent on the login shell's PATH ($claude_path)" \
  || fail "claude not found on the login shell's PATH" "$(jq -c '.agents[] | select(.id == "claude")' "$out/agents.json")"

# One turn with the agent, as the first design after onboarding.
project="$(curl -fsS -X POST -H 'content-type: application/json' -d '{"name":"smoke"}' "$url/api/projects")"
project_id="$(jq -r .project.id <<< "$project")"
conversation="$(jq -r .conversationId <<< "$project")"
run_id="$(curl -fsS -X POST -H 'content-type: application/json' \
  -d "$(jq -nc --arg p "$project_id" --arg c "$conversation" '{agentId:"claude",projectId:$p,conversationId:$c,message:"A landing page"}')" \
  "$url/api/runs" | jq -r .runId)"
status=running
for _ in $(seq 1 60); do
  status="$(curl -fsS "$url/api/runs/$run_id" | jq -r .run.status)"
  [ "$status" != running ] && break
  sleep 1
done
page="$data/projects/$project_id/index.html"
[ "$status" = succeeded ] && [ -s "$page" ] && ok "an agent run wrote $(basename "$page")" \
  || fail "agent run $run_id ended '$status'" "$(ls -l "$data/projects/$project_id" 2>&1)"

if [ "${SMOKE_EXPORT:-0}" = 1 ]; then
  curl -sS -X POST -H 'content-type: application/json' -d '{"fileName":"index.html"}' \
    -o "$out/export.pdf" "$url/api/projects/$project_id/export/pdf" 2> "$out/export.err"
  [ "$(head -c 4 "$out/export.pdf" 2>/dev/null)" = "%PDF" ] && ok "PDF export through Chrome" \
    || fail "PDF export" "$(cat "$out/export.err"; head -c 400 "$out/export.pdf" 2>/dev/null)"
fi

# The window: titled Contour, and not blank once the UI has loaded.
for _ in $(seq 1 30); do
  DISPLAY=:99 xwininfo -root -tree 2>/dev/null | grep -q '"Contour"' && break
  sleep 1
done
DISPLAY=:99 xwininfo -root -tree 2>/dev/null | grep -q '"Contour"' && ok "a window titled Contour is open" || fail "no window titled Contour"
sleep "${SMOKE_SETTLE:-8}"
shot="$out/window-ubuntu-$VERSION_ID-${pkg##*.}.png"
if DISPLAY=:99 import -window root "$shot" 2>> "$out/app.log"; then
  spread="$(convert "$shot" -colorspace gray -format '%[fx:standard_deviation]' info:)"
  awk -v s="$spread" 'BEGIN { exit !(s > 0.02) }' && ok "screenshot $(basename "$shot") is not blank" \
    || fail "the window looks blank (grey spread $spread); see $(basename "$shot")"
else
  fail "no screenshot"
fi

# SIGTERM (logout) quits cleanly. The wrappers (runuser, dbus-run-session)
# are left alone: the signal goes to the app itself.
pkill -TERM -u "$user" -x od-desktop
for _ in $(seq 1 20); do pgrep -u "$user" -x od-desktop > /dev/null || break; sleep 0.5; done
if pgrep -u "$user" -x od-desktop > /dev/null; then fail "the app ignored SIGTERM"; else ok "SIGTERM quits the app"; fi
if grep -rqs 'panicked' "$data/logs" "$out/app.log"; then
  fail "the log has a panic" "$(grep -rhs -A3 'panicked' "$data/logs" "$out/app.log" | head -8)"
fi

if [ "${pkg##*.}" = deb ]; then
  apt-get remove -y contour > "$out/apt-remove.log" 2>&1 && [ ! -e "$app" ] && [ ! -e /usr/share/applications/Contour.desktop ] \
    && ok "apt remove contour removes the app and its menu entry" || fail "apt remove contour" "$(tail -5 "$out/apt-remove.log")"
fi
finish
