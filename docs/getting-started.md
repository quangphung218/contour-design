# Getting started

This guide takes you from nothing to a first design. It is for people using
Contour; contributors want [development.md](development.md).

## 1. Install

### Download a release

Releases are published on the repository's
[Releases page](https://github.com/quangphung218/contour-design/releases):

| Platform | File | Notes |
|---|---|---|
| macOS 13.3 or later (Apple silicon and Intel) | `Contour_<version>_universal.dmg` | Open the `.dmg` and drag Contour to Applications. Unsigned builds need one extra step the first time: see [Open an unsigned build on macOS](#open-an-unsigned-build-on-macos). |
| Ubuntu 22.04 LTS or 24.04 LTS (x64) | `Contour_<version>_amd64.deb` or `Contour_<version>_amd64.AppImage` | `sudo apt install ./Contour_<version>_amd64.deb`, or run the AppImage. See [Install on Ubuntu](#install-on-ubuntu). Other x64 distributions with WebKitGTK 4.1 are best effort. |
| Windows 10 or 11 (x64) | `Contour_<version>_x64_en-US.msi` | Double-click it; the installer is unsigned, so SmartScreen asks once. See [Install on Windows](#install-on-windows). |

#### Open an unsigned build on macOS

A build without a signing certificate is stopped by macOS the first time
("Apple could not verify Contour…"). Allow it once; later launches open
normally.

**macOS 15 (Sequoia) and later.** Control-click ▸ Open no longer gets past
this check.

1. Open Contour from Applications. When macOS refuses, click **Done**.
2. Open **System Settings ▸ Privacy & Security** and scroll to
   **Security**. Next to "Contour was blocked to protect your Mac", click
   **Open Anyway** and enter your password.
3. Open Contour again and click **Open Anyway** in the dialog.

**macOS 13.3 to 14 (Sonoma).** Control-click (or right-click) Contour in
Applications ▸ **Open**, then **Open** in the dialog.

**Any version, from Terminal:**

```bash
xattr -dr com.apple.quarantine /Applications/Contour.app
```

Check a download against `SHA256SUMS` from the same release:

```bash
shasum -a 256 -c SHA256SUMS --ignore-missing    # macOS
sha256sum -c SHA256SUMS --ignore-missing        # Linux
```

On Windows, compare `Get-FileHash .\Contour_<version>_x64_en-US.msi`
(PowerShell) with the line in `SHA256SUMS`.

#### Install on Ubuntu

Ubuntu 22.04 LTS and 24.04 LTS on x64 are supported; every release is
installed and opened on both before it is published. Pick one of the two
files.

**The `.deb` (recommended).** `apt` pulls in WebKitGTK and GTK:

```bash
cd ~/Downloads
sudo apt install ./Contour_<version>_amd64.deb
```

Contour is then in the app menu (Graphics and Development) and starts
from a terminal as `od-desktop`. To update, install the newer `.deb` the
same way.

**The AppImage.** No install, but it needs FUSE 2, which Ubuntu does not
ship by default:

```bash
sudo apt install libfuse2        # Ubuntu 22.04
sudo apt install libfuse2t64     # Ubuntu 24.04
chmod +x Contour_<version>_amd64.AppImage
./Contour_<version>_amd64.AppImage
```

Without FUSE (some containers and VMs), let it unpack itself instead:
`APPIMAGE_EXTRACT_AND_RUN=1 ./Contour_<version>_amd64.AppImage`.

**Uninstall.** `sudo apt remove contour` removes the app and its menu
entry; for the AppImage, delete the file. Your projects and settings stay
in `~/.local/share/contour`; delete that folder too to remove them.

**Logs.** `~/.local/share/contour/logs/daemon.log` (Help ▸ Report Issue
opens the folder). Started from a terminal (`od-desktop`), the log also
prints there.

**Agents.** Opened from the app menu, Contour asks your login shell for
its `PATH` (`bash -il`, which reads `~/.profile` and `~/.bashrc`), so
agents installed with nvm, npm or the native installers (`~/.local/bin`)
are found. PDF, image and PPTX export use Google Chrome or Chromium.

#### Install on Windows

Windows 10 and 11 on x64 are supported; every release's installer is
installed, opened and uninstalled on clean Windows Server 2025 and 2022
(the server editions of Windows 11 and 10) before it is published.

1. Download `Contour_<version>_x64_en-US.msi` and double-click it.
2. The installer is not signed yet, so Microsoft Defender SmartScreen may
   say "Windows protected your PC". Click **More info**, check that the
   app is `Contour_<version>_x64_en-US.msi`, then **Run anyway**.
3. Allow the installer to make changes (it installs for all users, in
   `C:\Program Files\Contour`). Contour is then in the Start menu.

The app uses Microsoft Edge WebView2, which Windows 10 and 11 already have;
if it is missing the installer downloads it. To update, install the newer
`.msi` the same way.

To install without questions (a script, Intune), from an administrator
PowerShell:

```powershell
msiexec /i Contour_<version>_x64_en-US.msi /qn /l*v contour-install.log
```

**Uninstall.** Settings ▸ Apps ▸ Installed apps ▸ Contour ▸ **Uninstall**,
or `msiexec /x Contour_<version>_x64_en-US.msi /qn`. This removes the app
and its Start menu entry. Your projects and settings stay in
`%APPDATA%\contour`; delete that folder too to remove them.

**Logs.** `%APPDATA%\contour\logs\daemon.log` (Help ▸ Report Issue opens
the folder). The installer writes a log only when asked to, with
`/l*v <file>` as above.

**Agents.** Contour looks for `claude.exe`, `claude.cmd` and the like on
your `Path`, including folders an installer added since you signed in to
Windows, and in the usual install folders: `%USERPROFILE%\.local\bin`
(the Claude Code installer) and `%APPDATA%\npm` (`npm install -g`).
Installed an agent while Contour is open? Press **Check again** on the
first screen. PDF, image and PPTX export use Microsoft Edge, which Windows
has, or Chrome.

### Build from source

You need Rust 1.85+, Node.js 20+ and `trunk`. `make setup` checks them and
prints the command for anything missing.

```bash
git clone https://github.com/quangphung218/contour-design.git
cd contour-design
make setup
make app-release      # optimised Contour.app, about 10 minutes the first time
make install          # copies it to /Applications (macOS)
```

On Ubuntu, `scripts/linux-deps.sh` installs the build packages first, and
the desktop packages come from Tauri:

```bash
scripts/linux-deps.sh
make ui-release
(cd apps/desktop && npx --yes @tauri-apps/cli@2.12.4 build --bundles deb,appimage)
```

Or skip the desktop app and run the daemon with the UI in your browser:

```bash
make run              # http://127.0.0.1:7456
```

## 2. Give Contour an agent

Contour does not call a model itself. It drives a coding agent that is
already installed on your machine and writes HTML into the project folder.
Install one of these and sign in to it once in a terminal:

| Agent | Command on `PATH` |
|---|---|
| Claude Code | `claude` |
| Codex CLI | `codex` |
| Cursor Agent | `cursor-agent` |
| GitHub Copilot CLI | `copilot` |
| OpenCode | `opencode-cli`, then `opencode` |
| Qwen Code | `qwen` |

Ten more agents speak ACP (Kimi, Hermes, Kiro, Kilo, Mistral Vibe, Trae,
DeepSeek Reasonix, Devin for Terminal, DeepSeek Harness); see
[features.md](features.md#agents).

No agent? Choose **API key (BYOK)** on the first screen and paste an
Anthropic, OpenAI-compatible or Azure OpenAI key. The model then answers
with a single HTML file, without tools.

## 3. Make a first design

1. Open Contour. The first screen lists the agents it found; pick one (or
   press **Refresh** after installing one).
2. On Home, type a brief, for example *"Landing page for a tea shop, warm
   colours, menu and opening hours"*. Choose the kind (prototype or slide
   deck), optionally a design system and a template, then **Create**.
3. The Studio opens. The agent's messages stream on the left; the preview
   on the right reloads whenever it writes a file. Click **Comment** on any
   element to ask for a change, or edit text and styles directly.
4. **Export** (top right of the preview) saves HTML, PDF, PNG, PPTX, ZIP or
   MP4. PDF, image, PPTX and MP4 need Chrome, Edge or Chromium installed.

## Where your work lives

Projects, runs and settings are in one data folder:

| OS | Folder |
|---|---|
| macOS | `~/Library/Application Support/contour` |
| Linux | `$XDG_DATA_HOME/contour` (default `~/.local/share/contour`) |
| Windows | `%APPDATA%\contour` |

Set `OD_DATA_DIR` to use another folder. Help ▸ Show Data Folder opens it.
If you used Contour when it was still called *Nét*, the first start moves
the old `open-design-rs` folder to `contour`; nothing is copied or deleted.
On macOS and Linux it also leaves a link at the old path, so an older build
still finds the data; on Windows there is no link. Old builds kept it next to the new
folder on macOS and Linux, and in `%HOME%\.local\share\open-design-rs` on
Windows when `HOME` was set. Without `HOME` they used `.od` in the folder
they were started from; that one is not moved, so point `OD_DATA_DIR` at it.

## Next

- Every feature, step by step: [features.md](features.md)
- Something not working: [troubleshooting.md](troubleshooting.md)
- How Contour compares with OpenDesign: [parity.md](parity.md)
