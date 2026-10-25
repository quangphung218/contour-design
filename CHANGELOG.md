# Changelog

All notable changes to Contour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).
`make version V=x.y.z` turns the Unreleased section into a release
([docs/release.md](docs/release.md)).

## [Unreleased]

### Fixed

- Renaming a project to a blank name keeps the old name and says a name is
  required; failed renames, duplicates and deletes now show their error (QA-001).
- The API provider form no longer saves a provider that needs a key without one;
  it says "API key required" and Continue stays off (QA-005).

## [0.1.0] - 2026-10-24

First release: a Rust rewrite of [OpenDesign](https://github.com/nexu-io/open-design)
that runs entirely on your machine, at feature parity with the original app
(207 of 207 scored rows, [docs/parity.md](docs/parity.md)).

### Added

- **Daemon and CLI** (`od`): HTTP + SSE + WebSocket API on `127.0.0.1:7456`,
  one daemon per data folder, thin CLI clients for projects, files, runs,
  lint, export, media, plugins and MCP.
- **Desktop app** (Tauri 2): the daemon runs inside the app on a random
  loopback port; native folder picker, menus, login-shell `PATH` for agents.
- **Agents**: 6 coding-agent CLIs (Claude Code, Codex, Cursor Agent, Copilot,
  OpenCode, Qwen Code) and 10 ACP agents, detected on `PATH`; auto or ask
  permission mode; BYOK with 26 provider presets (Anthropic, OpenAI-compatible,
  Azure OpenAI and more).
- **Studio**: chat with the agent, live preview with comments, inspect and
  direct edits written back to the source, file versions, tweaks, decks,
  code view with highlighting, terminal tabs (xterm.js, bundled).
- **Content**: 169 skills, 115 design templates, 152 design systems, craft
  guides and plugins copied from OpenDesign with their licences (`NOTICE`).
- **Export**: self-contained HTML, ZIP, PDF (raster and vector), PNG/JPEG,
  PPTX (image and editable), MP4, React component as HTML; uses the Chrome,
  Edge or Chromium already installed.
- **Automations**: routines on a local cron schedule, never overlapping.
- **MCP**: Contour as an MCP server for other agents (`od mcp install`) and
  as an MCP client for stdio/HTTP servers (OAuth 2.1 PKCE).
- **Media**: image, video and audio generation with your own keys.
- **UI** in 20 languages, light and dark themes, keyboard shortcuts and a
  command palette.
- **Data folder migration**: the first run moves an existing
  `open-design-rs` data folder to `contour`, leaving a link at the old path
  on macOS and Linux.
- **Project tooling**: `make help` targets for setup, dev, test, e2e, lint,
  bundles, install and versioning; GitHub Actions for CI and releases;
  cargo-deny policy; docs for users and contributors.

### Fixed

- The hidden ZIP input on the Projects page has an accessible name (axe `label`).

[Unreleased]: https://github.com/quangphung218/contour-design/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/quangphung218/contour-design/releases/tag/v0.1.0
