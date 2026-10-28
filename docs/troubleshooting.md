# Troubleshooting

Problems people hit, with the cause and the fix. If yours is not here,
collect diagnostics (Help ▸ Report Issue, or the daemon log in
`<data folder>/logs/daemon.log`, with `RUST_LOG=debug` for more) and open an
issue.

## Running

### "an od daemon is already running at … on …" / the app quits at start

Each data folder has one daemon (`daemon.lock`). The message names the URL
of the daemon that owns the folder: an `od` daemon, or the desktop app.
Use that one, or stop it:

```bash
od daemon status                 # answers if a daemon listens on :7456
pkill -x od                      # stops od daemons; quit the desktop app from its menu
```

To run a second, independent daemon, give it its own data folder and port:
`OD_DATA_DIR=/tmp/contour-2 od daemon start --port 7460`.

### Port 7456 is in use

Another program holds the port. Start on another one with
`od daemon start --port 7460` (or `OD_PORT=7460`) and point clients at it
with `--daemon-url http://127.0.0.1:7460`. `make dev` expects 7456 because
`trunk serve` proxies there.

### The UI is blank or shows "UI bundle not found"

The Linux `.deb` and AppImage of 0.2.0-beta.2 and earlier open on this
page: update to a newer release. From source, the daemon serves
`crates/od-ui/dist` (or `OD_UI_DIST`). Build it:

```bash
make ui-dist
```

After pulling UI changes, rebuild it: `rm -rf crates/od-ui/dist && make ui-dist`.

### My projects are gone after the rename

They are not. The first start of Contour moves
`~/Library/Application Support/open-design-rs` (Linux:
`~/.local/share/open-design-rs`; Windows: `%HOME%\.local\share\open-design-rs`)
to `.../contour`, and on macOS and Linux leaves a symlink at the old name
(Windows gets no link: an old build started again there sees an empty
folder, so run only Contour after the move). If the old folder was
itself a symlink (data on another drive), the link is moved and `contour`
points at the same place. Check:

```bash
ls -la ~/Library/Application\ Support/ | grep -E "contour|open-design-rs"
```

If both folders exist as real folders (the new one was created before the
move could happen), Contour uses `contour` and leaves `open-design-rs`
untouched. Quit Contour and every `od` daemon, then move the projects
yourself or start once with `OD_DATA_DIR` pointing at the old folder. On
Windows, builds started without `HOME` kept their data in `.od` inside the
folder they were started from; Contour does not move that one.

## Ubuntu and other Linux

### The window is blank or white (Wayland, NVIDIA)

WebKitGTK's DMA-BUF renderer fails on some GPU drivers, mostly NVIDIA
under Wayland. Start Contour with it turned off:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 od-desktop
```

If that works, make it stick for the app menu: copy the menu entry and
edit its `Exec` line.

```bash
cp /usr/share/applications/Contour.desktop ~/.local/share/applications/
sed -i 's|^Exec=|Exec=env WEBKIT_DISABLE_DMABUF_RENDERER=1 |' ~/.local/share/applications/Contour.desktop
```

Still blank: try `WEBKIT_DISABLE_COMPOSITING_MODE=1` as well, or log in with
"Ubuntu on Xorg" from the login screen's gear menu.

### The AppImage does not start: "dlopen(): error loading libfuse.so.2"

AppImages need FUSE 2: `sudo apt install libfuse2` on Ubuntu 22.04,
`sudo apt install libfuse2t64` on 24.04. Or run it without FUSE:
`APPIMAGE_EXTRACT_AND_RUN=1 ./Contour_<version>_amd64.AppImage`. The
`.deb` has neither problem.

### The AppImage quits at start: "Couldn't open libGLESv2.so.2"

The AppImage brings WebKitGTK but uses the system's OpenGL libraries, and
a minimal install may lack OpenGL ES: `sudo apt install libgles2`. The
`.deb` pulls in everything it needs.

### `apt install` says "unable to locate package" or "is not a .deb"

Give `apt` a path, with `./` in front: `sudo apt install ./Contour_<version>_amd64.deb`.
Without it, `apt` searches its repositories for a package of that name.

## Windows

### "Windows protected your PC" when opening the installer

The `.msi` is not signed yet, so Microsoft Defender SmartScreen does not
know it. Click **More info**, check that the file is
`Contour_<version>_x64_en-US.msi` from the Releases page (compare it with
`SHA256SUMS`: `Get-FileHash .\Contour_<version>_x64_en-US.msi`), then
**Run anyway**. If the file's Properties show "This file came from another
computer", tick **Unblock** first.

### The installer fails or rolls back

Run it again with a log, from an administrator PowerShell:
`msiexec /i Contour_<version>_x64_en-US.msi /l*v contour-install.log`. Search
the log for `Return value 3`; the lines above it name the failing step.
Installing needs administrator rights (Contour installs for all users).

### The window is blank or white

Contour draws its window with Microsoft Edge WebView2. Windows 10 and 11
have it; the installer downloads it otherwise, which needs a connection.
Check Settings ▸ Apps ▸ Installed apps for **Microsoft Edge WebView2
Runtime**; if it is missing, install the Evergreen runtime from Microsoft
and open Contour again. The app log is in
`%APPDATA%\contour\logs\daemon.log`.

## Agents

### No agent is detected

Detection looks for the binary on the daemon's `PATH`. In a terminal,
`which claude` (or `codex`, …) must print a path.

- Desktop app started from Finder or the Ubuntu app menu: Contour reads
  `PATH` from your login shell (`$SHELL -il`). Put the agent's folder on
  `PATH` in `~/.zprofile` or `~/.zshrc` (macOS), `~/.profile` or
  `~/.bashrc` (Ubuntu), quit the app and open it again.
- Windows: Contour looks for `claude.exe`, `claude.cmd` and the like (not
  the extensionless `claude` npm also writes) on `Path`, including the
  user and system `Path` in the registry, so an agent installed after you
  signed in is found. It also looks in `%USERPROFILE%\.local\bin` and
  `%APPDATA%\npm`. In PowerShell, `Get-Command claude` must print a path.
- Installed it while Contour was open: press **Refresh** on the agent
  picker. Nothing is cached.
- Check what the daemon sees: `curl -s http://127.0.0.1:7456/api/agents`.

### A run hangs

The agent is probably waiting for a permission prompt that nobody answers:
in **Ask first** mode, agents other than Claude get no self-approve flag.
Stop the run, switch Settings ▸ Permissions back to **Automatic**, or answer
in the agent's own terminal. The run log is in
`<data folder>/runs/<run-id>/events.jsonl`.

### The agent says it is not signed in

Contour uses the agent's own login. Run the agent once in a terminal
(`claude`, `codex login`, …) and sign in there.

### MCP does not connect after an upgrade

Up to 0.2.0-beta.4, Integrations ▸ Install registered
`<app> mcp --daemon-url http://127.0.0.1:<port>` in Claude Code and Codex,
with the port the app had that day. The app gets a new port at every
launch, so that line pointed at nothing after a restart.

In releases after 0.2.0-beta.5 the old line works with no change: when nothing
answers at `--daemon-url` (or `OD_DAEMON_URL`) within 1 s, `mcp` uses the
running app's URL from `<data folder>/daemon.lock` and says so on stderr
(`mcp: no daemon at …; using the running app's … from daemon.lock`).
Keep Contour open while the agent uses it.

Still failing? Press **Install** again in Integrations. It replaces the old
line with one that has no `--daemon-url`. Check what the agent has with
`claude mcp get contour` or in `~/.codex/config.toml`.

## Export

### "this export needs Chrome, Edge or Chromium"

PDF, image, PPTX and MP4 export render with the browser already installed.
Install Chrome, or point at a browser with `OD_CHROME_PATH`:

```bash
export OD_CHROME_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
```

### Export fails: Chrome "did not show the page within 60 s"

Chrome started but could not start its own sandbox, which happens when
Contour or `od` itself runs inside another sandbox (macOS `sandbox-exec`,
some containers). Chrome then prints `Failed to initialize sandbox` and
never renders. Only in that setup, let Chrome run without its sandbox:

```bash
OD_CHROME_NO_SANDBOX=1 od daemon start
```

**Security:** this turns off Chrome's sandbox, which isolates the HTML an
agent wrote while it is rendered. Set it only when the outer sandbox already
contains the process, never as a general fix. Contour never turns it on by
itself; without it, export fails with this error instead of hanging. If
Chrome cannot start at all, export fails with "cannot start": at once when the browser exits, within 30 s when it hangs before opening DevTools.

### MP4 export fails

It needs `ffmpeg` on `PATH`, in Homebrew's folder, or at `OD_FFMPEG_PATH`:
`brew install ffmpeg`.

## Building

### `error: linker` / missing `webkit2gtk` on Linux

The desktop crate needs WebKitGTK. Run `scripts/linux-deps.sh`, or build
only the daemon: `cargo build -p od-cli`.

### `trunk: command not found` or `wasm32-unknown-unknown` missing

Run `make setup`: it adds the target and prints the trunk install command.

### Cargo uses a stale build from another worktree (E0432, E0609 on workspace crates)

Use `make`, which keeps one target dir per checkout, or export
`CARGO_TARGET_DIR="$HOME/.cache/odrs-target-$(basename "$PWD")"` before
calling `cargo` directly ([development.md](development.md#build-settings)).

### The machine runs out of memory during a build

Lower the parallelism: `CARGO_BUILD_JOBS=2 make check`.

### The release app is much larger than 130 MB

Its UI was built in debug mode (~76 MB of WASM instead of ~12 MB). Use
`make app-release`, which always runs `trunk build --release`, rather than
calling the Tauri CLI on an existing `crates/od-ui/dist`.

### e2e fails with "Executable doesn't exist" (Playwright)

Install the Playwright browser: `cd crates/od-ui/e2e && npx playwright install chromium`.

## macOS

### "Contour is damaged and can't be opened" / "unidentified developer"

The download is not signed. On macOS 15 and later, open the app once, then
System Settings ▸ Privacy & Security ▸ **Open Anyway** and open it again
(Control-click ▸ Open no longer works there). On macOS 14 and earlier,
Control-click the app ▸ Open the first time. On any version:

```bash
xattr -dr com.apple.quarantine /Applications/Contour.app
```

### `make install` refuses to replace the app

`/Applications/Contour.app` exists but has another bundle identifier than
`dev.contour.desktop`, so it is a different app. Move it away yourself if
you really want to replace it.
