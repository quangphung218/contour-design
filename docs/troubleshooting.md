# Troubleshooting

Problems people hit, with the cause and the fix. If yours is not here,
collect diagnostics (Help ▸ Report Issue, or the daemon log with
`RUST_LOG=debug`) and open an issue.

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

The daemon serves `crates/od-ui/dist` (or `OD_UI_DIST`). Build it:

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

## Agents

### No agent is detected

Detection looks for the binary on the daemon's `PATH`. In a terminal,
`which claude` (or `codex`, …) must print a path.

- Desktop app started from Finder: Contour reads `PATH` from your login
  shell. Put the agent's folder on `PATH` in `~/.zprofile` or `~/.zshrc`,
  quit the app and open it again.
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

The download is not signed. Right-click the app ▸ Open the first time, or:

```bash
xattr -dr com.apple.quarantine /Applications/Contour.app
```

### `make install` refuses to replace the app

`/Applications/Contour.app` exists but has another bundle identifier than
`dev.contour.desktop`, so it is a different app. Move it away yourself if
you really want to replace it.
