# Changelog

All notable changes to Contour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).
`make version V=x.y.z` turns the Unreleased section into a release
([docs/release.md](docs/release.md)).

## [Unreleased]

## [0.2.0-beta.5] - 2026-10-28

### Added

- Pull requests run the workspace tests on Windows (`test (windows-2025)`
  in `ci.yml`), not only a compile check.
- The Windows smoke test installs the MCP server into Claude Code and
  Codex the way Integrations does, through npm-style `.cmd` shims, and
  checks the command each one gets. It then installs the latest published
  release, makes a project, installs the new `.msi` over it and checks
  there is one uninstall entry with the new version, the app starts and
  the data folder is untouched (`scripts/windows-upgrade-smoke.ps1`).

### Fixed

- Design system cards show the right colors when a `DESIGN.md` has
  Windows (CRLF) line ends, as every bundled one has in the Windows
  installer. Color tables were skipped, so 11 bundled systems showed other
  swatches than on macOS and Linux.
- On Windows, installing the MCP server into an agent gives it the app's
  plain path (`C:\Program Files\…`), not the `\\?\C:\…` form.

## [0.2.0-beta.4] - 2026-10-28

### Added

- Windows 10 and 11 (x64) are supported platforms. Every release's `.msi`
  is installed silently, opened from its Start menu shortcut, runs an agent
  turn and a PDF export, and is uninstalled on clean Windows Server 2025
  and 2022 before the release is drafted (`windows-smoke.yml`). An
  installer that builds but fails this stops the draft; the Windows build
  itself stays best effort.
- Getting started explains installing Contour on Windows (SmartScreen:
  More info ▸ Run anyway), silent installs, uninstalling and where the logs
  are; troubleshooting covers SmartScreen, a failed install and a blank
  window.

### Fixed

- Windows finds agent CLIs installed with npm (`claude.cmd`) or a native
  installer (`claude.exe`). Detection looked for the bare name, which on
  Windows is npm's `sh` script or nothing. It also looks in
  `%USERPROFILE%\.local\bin` and `%APPDATA%\npm`, and reads `Path` from
  the registry, so an agent installed after signing in to Windows is found
  without signing out; onboarding has Check again on Windows too.
- Integrations ▸ install the Contour MCP server into Claude Code or Codex
  works on Windows: it ran npm's `sh` script (`claude`, "not a valid Win32
  application") or found nothing, instead of `claude.cmd` or `claude.exe`.
- PDF, PNG and PPTX export work on Windows: Edge was handed a page address
  it could not open (`file:////%3F/C:/…`) and the export failed after two
  minutes with "loading the page: Request timed out".

## [0.2.0-beta.3] - 2026-10-27

### Added

- Ubuntu 22.04 LTS and 24.04 LTS (x64) are supported platforms. Every
  release's `.deb` and AppImage are installed and opened on clean Ubuntu
  22.04 and 24.04 before the release is drafted (`linux-smoke.yml`):
  the app starts, an agent runs a turn and PDF export works.
- Getting started explains installing, updating and removing Contour on
  Ubuntu, FUSE for the AppImage, and where the logs are; troubleshooting
  covers a blank window under Wayland.

### Fixed

- The Linux `.deb` and AppImage open the app instead of "UI bundle not
  found".
- Contour is listed under Graphics and Development in the Linux app menu,
  with a proper description, and the `.deb` recommends `xdg-utils`.
- The menu bar has one Help menu, and Project comes before Window.
- The Windows installer (`.msi`) builds for pre-release versions: WiX gets a
  numeric version (`0.2.0-beta.2` → `0.2.0.102`) from `scripts/msi-version.sh`.

## [0.2.0-beta.2] - 2026-10-26

### Changed

- The macOS app needs macOS 13.3 or later: the preview's reload guard relies
  on the browser telling the app's own loads from the page's, which older
  versions of the system web view do not do.

### Fixed

- A plugin run whose agent finished at once (a quick failure, a fast model) no
  longer loses its GenUI questions: they are asked before the agent starts.
- The preview no longer stops with "the page kept reloading itself" when files
  are saved in quick succession (an agent writing many files, fast manual
  edits); a page that really reloads itself is still stopped.
- A page that loads itself again under a new preview address each time is
  stopped like any other reload loop: only loads the app starts get an edit
  key, so the page cannot pass for the app saving fast.
- A plugin run canceled while its GenUI questions were being asked no longer
  leaves them waiting in the chat for a run that never started.
- The Windows build compiles without warnings.

## [0.2.0-beta.1] - 2026-10-26

### Added

- The desktop app writes its log to `<data folder>/logs/daemon.log` (rolled over
  at 5 MB, 5 files kept), so a launch from Finder keeps it; `od daemon start
  --log-file` or `OD_LOG_FILE=1` does the same for the CLI daemon. A panic
  writes its message and stack there before the app quits.
- Help ▸ Report Issue and Export diagnostics include the last 200 lines of that
  log, with secret-looking values redacted wherever they sit (`api_key=…`, a
  `?token=…` in a URL, `Bearer …`). Nothing is sent anywhere. Report Issue now
  opens that log folder instead of the run folder.

### Changed

- Templates show a name and a description written for people ("CRM table (live
  data)" instead of "Crm Table Live"); the agent-facing "Use when the brief asks
  for…" text no longer appears in the UI, and each example has its own
  description. Languages without a translation show the English text.
- The Swiss-print web page and four other templates are filed under the right
  type (a web page was listed as Video, decks as Image).
- Templates are sorted by the name on the card; no two share a name, and the
  sample decks are about made-up companies, not about Contour.
- Plugins say what they do for you; kind, atoms, pipeline stages and
  capabilities moved under Developer details, and the filter chips read
  "Workflows" and "Building blocks".
- Design systems are described by their look (the first paragraph of their
  DESIGN.md) on their page and in Home's picker, instead of the maintainer note
  "Bundled … package for X, derived from curated DESIGN.md…"; in the picker, a
  brand-named system also carries the trademark note.
- Home's template picker is sorted by the name shown.
- The example briefs of the Code migration and Media generation plugins read as
  a user would write them.
- Brand-named design systems show "Inspired by <Brand>'s public design language.
  Not affiliated with or endorsed by <Brand>. Trademarks belong to their owners."
  on their page, in every UI language.
- The README opens with what Contour is (no telemetry, runs on your machine, uses
  the agent CLI you have), a recorded demo, and how it differs from OpenDesign.
- Home keeps one composer with one row of controls: the "Recommended for you"
  strip is gone, and New design system, Create plugin and From folder moved into
  the "+" menu. New design system opens the same dialog as the Design systems
  page, with every source.
- The Studio names the project at the top of the chat, with the conversation
  under it; the files panel is headed "Files".
- The Export menu folds the image formats under Image and the two PowerPoint
  kinds under PowerPoint, lists only the formats the open file exports to, and
  says "MP4 video".
- Settings saves Permissions, Appearance and Language as soon as they change;
  the agent card ("Save agent") and the API provider form ("Save and use") each
  have their own Save, never both at once.
- Messages are named by what was asked, with the state as a badge, and the page
  is called Messages everywhere. Each run keeps the message it answered
  (`prompt` in `GET /api/runs` and `GET /api/runs/:id`).
- The chat says what the agent did ("Updated plan", "Wrote", "1 new") instead of
  tool names such as `TodoWrite`.
- New routines start on "New project each run", and the form goes back to it
  after an edit is saved or cancelled.
- "+" on Home names its two folder items by what they do: "Open a folder as a
  project" and "Let the agent read a folder".
- The run status says "1 file written" / "N files written".
- Media and Templates say "Audio"; Media ▸ Generate has one way in, real selects
  and a Set up providers button.
- Segmented controls, tab lists and radio chips move with the arrow keys and are
  one Tab stop each.
- Onboarding: when an installed agent fails its test, the reason and "<Agent> did
  not pass its test. Update it, or install one of these instead." sit right under
  it, on the first screen; "Still no agent found" names both `claude --version`
  and `codex --version`.

### Fixed

- `/api/runs` keeps only the first line of each run's prompt (up to 200
  characters) instead of the whole message, which the run list polled every
  few seconds; runs saved before this are cut the same way when loaded.
- The "deliverable checked" log line compares whole paths: a run that writes
  `old/index.html` next to the entry `index.html` counts as another page.
- Renaming a project to a blank name keeps the old name and says a name is
  required; failed renames, duplicates and deletes now show their error (QA-001).
- The API provider form no longer saves a provider that needs a key without one;
  it says "API key required" and Continue stays off (QA-005).
- Esc closes the drop-down menus: Export, Screenshot, Present, the project menu and
  the files panel menus (QA-004).
- Leaving a Markdown file right after typing no longer panics (QA-009).
- Cancel in the delete-conversation dialog no longer throws a JS error (QA-010).
- Tablet and Phone previews lay the page out at 820 and 390 px and scale it down when
  the pane is narrower, so device media queries apply (QA-003).
- Edit panel: an Apply in one tab, or typing while it saves, no longer wipes what
  was typed and not applied in another field (QA-006).
- With an edit panel change not applied, closing or reloading the page asks first,
  and so does a link that leaves the page (QA-007).
- A message sent right after opening a project no longer vanishes from the chat
  when the stored transcript arrives (QA-002).
- Drafts are kept: the Studio composer per project and conversation, and Home's brief
  and design system, across reloads and visits until sent (QA-008).
- A run that writes another page (a deck next to `index.html`) opens it in a tab
  instead of warning that the entry file was not updated.
- The "Media library" project that holds uploads no longer shows in Projects,
  Recent or the Automations project list.
- Onboarding's brand extraction runs the agent the user picked and tested, not
  the first one found; "Not logged in" and "not signed in" count as a missing
  sign-in.
- A link to a project that does not exist says "This project was deleted or the
  link is wrong." and offers only All projects: no Retry, tabs, Export, files or
  pickers, and the composer is off. Unknown addresses get a "Page not found"
  page with Go to Home.
- An empty template search offers Clear filters; Design systems search matches
  the description people read, not the package note; the Skills tab drops the
  "Use when…" text meant for the agent.
- The "first design is ready" hint closes when the next message is sent.
- At 390 px, Preview closes the side chat and New project leads the Projects
  header; comment chips read "h1: …"; the command palette no longer matches
  scattered letters ("set" found "Quarterly marketing site").

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

[Unreleased]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.5...HEAD
[0.2.0-beta.5]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.4...v0.2.0-beta.5
[0.2.0-beta.4]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.3...v0.2.0-beta.4
[0.2.0-beta.3]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.2...v0.2.0-beta.3
[0.2.0-beta.2]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.1...v0.2.0-beta.2
[0.2.0-beta.1]: https://github.com/quangphung218/contour-design/compare/v0.1.0...v0.2.0-beta.1
[0.1.0]: https://github.com/quangphung218/contour-design/releases/tag/v0.1.0
