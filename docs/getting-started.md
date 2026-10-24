# Getting started

This guide takes you from nothing to a first design. It is for people using
Contour; contributors want [development.md](development.md).

## 1. Install

### Download a release

Releases are published on the repository's
[Releases page](https://github.com/quangphung218/contour-design/releases):

| Platform | File | Notes |
|---|---|---|
| macOS (Apple silicon and Intel) | `Contour_<version>_universal.dmg` | Open the `.dmg` and drag Contour to Applications. Unsigned builds: right-click ▸ Open the first time, or run `xattr -dr com.apple.quarantine /Applications/Contour.app`. |
| Linux x64 | `Contour_<version>_amd64.AppImage` or `.deb` | `chmod +x` the AppImage and run it, or `sudo apt install ./Contour_<version>_amd64.deb`. |
| Windows x64 | `Contour_<version>_x64_en-US.msi` | Best effort: Windows is compile-checked but not tested yet. |

Check a download against `SHA256SUMS` from the same release:

```bash
shasum -a 256 -c SHA256SUMS --ignore-missing
```

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
If you used Contour when it was still called *Nét* (data in
`open-design-rs`), the first start moves that folder to `contour` and leaves
a link at the old path; nothing is copied or deleted.

## Next

- Every feature, step by step: [features.md](features.md)
- Something not working: [troubleshooting.md](troubleshooting.md)
- How Contour compares with OpenDesign: [parity.md](parity.md)
