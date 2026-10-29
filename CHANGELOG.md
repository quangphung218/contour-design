# Changelog

All notable changes to Contour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).
`make version V=x.y.z` turns the Unreleased section into a release
([docs/release.md](docs/release.md)).

## [Unreleased]

### Added

- Settings ▸ About ▸ Your usage shows how far first-run setup got, out of
  5 steps: the app opened, an agent found, an agent (or API key) passed
  its test, the first project, the first preview. Each step is recorded
  once, the first time it happens, on this machine only and never sent
  anywhere; an automation's project is not a first project. It reads
  "Reached step 5 of 5" with the time from the first open to the first
  preview, or the step still to come ("Next step: agent" for someone who
  quit at onboarding without an agent). Copy summary adds a `Setup:` line
  that names where setup stopped (`stopped at: agent`). A data folder
  made before this version says "not recorded" instead of a guess. In
  every UI language.

### Changed

- Your usage: a number without enough data reads "Needs 3 projects to
  measure (you have 1)", not "Not enough projects yet (1 of 3)", which
  read like 1 of 3 projects exported. "Since 0.2.0-beta.7" is said once,
  under the title, instead of under every number (in the summary, once on
  its first line). Copy summary points to the "Share your usage" thread in
  [GitHub Discussions](https://github.com/quangphung218/contour-design/discussions),
  and to an issue with steps to reproduce for a bug. In every UI language.

### Fixed

- A link to a Settings section (the quick menu, the Memory toast,
  `/settings#settings-about`) opens Settings at that section and keeps it
  there while the sections above finish loading. Before, the page always
  opened at its top. The chat's and Home's links to MCP servers now point
  at that section too.
- The Settings nav marks the section in view, and follows as the page
  scrolls (`aria-current` for screen readers).

## [0.2.0-beta.7] - 2026-10-29

### Added

- Settings ▸ About ▸ Your usage: three numbers worked out on this machine
  and never sent anywhere. Time from creating a project to its first
  preview (median), the share of projects exported or with 3+ chat turns,
  and the share of runs that failed, by cause (agent missing or didn't
  start, agent error, timed out…); runs you stopped are listed apart and
  not counted. Copy summary puts a short English summary on the clipboard
  (app version, OS, agent and the three numbers; no project name, path,
  prompt or file content) to paste into a GitHub discussion or issue; the
  text shows below the button, so it can be read first or copied by hand.
  With fewer than 3 projects or runs it says how many there are, "Not
  enough projects yet (1 of 3)", instead of a number. Previews (HTML and
  React components) and exports (Export menu, MP4, screenshot PNG, ZIP)
  are recorded from this version on, so the first two numbers count
  projects created since 0.2.0-beta.7 that someone started empty: not
  imported, remixed, duplicated or made from a saved template, nor the
  media library or an automation's project for one run. The bug report
  and feature request templates point to it. In every UI language.

### Fixed

- A deck lays out at its 1920×1080 design size and is scaled to fit the
  preview, so a narrow window or a phone shows the same slide, smaller.
  Before, a deck that does not scale itself (the html-ppt templates) was
  laid out again at the pane's width and its text ran off the slide. The
  slide thumbnails use the same size. On a phone the slide sits at the
  top of the preview, by the deck bar. A deck rewritten as a page is laid
  out as a page again.
- `od mcp` (the CLI) finds the app like `<app> mcp` does: a
  `--daemon-url` or `OD_DAEMON_URL` with nothing answering within 1 s
  gives way to the running app's URL in `daemon.lock`, said on stderr,
  and with neither given it uses `daemon.lock` (the default port only
  when there is none).
- Settings ▸ About's subtitle mentions updates in every UI language, as
  it does in English and Vietnamese.
- The three Export plugins no longer share the tile "ET": a plugin whose
  initials match another's shows other letters of its name (Export to
  Vue is EV, to React ER, to Next.js EN), in every UI language.
- Templates: the type chips no longer include "Template" between
  Prototype and Video. The two dashboards it held are under Prototype,
  where they open; the chips read Prototype, Slide deck, Image, Video,
  Audio.
- Templates ▸ All industries lists "Operations" once: the GitHub and live
  dashboards said "operation", so the list showed "Operation" next to it.
- When the agent crashes mid-turn, the chat shows its error ("Error:
  ENOSPC: no space left on device, write") instead of the last line of
  its stack trace ("at writeFileSync (node:fs:…)") or Node's version
  line. A failed turn has an Agent output disclosure (with a chevron)
  with everything the agent wrote to stderr, next to Retry, and "Failed"
  no longer breaks over two lines.
- A failed turn keeps its error message when the chat is reopened. Before,
  only "See the message above" was left, over nothing.
- PDF, PNG and PowerPoint exports try Chrome or Edge once more when it
  does not start within 30 s, so the first export on a slow or freshly
  set up PC does not fail with "Timeout while resolving websocket URL".
- Picking a `/` command or an `@` mention puts the caret after it at once:
  text typed straight after the pick no longer lands in the wrong place
  (`/search endstea tr` instead of `/search tea trends`).
- Text edited in the preview right after it reloads is saved; it was
  dropped without a word when typed before the reload had finished
  setting up.
- E (or Edit) on a design system that is still loading opens the DESIGN.md
  editor once it has loaded, instead of showing the DESIGN.md tab with no
  editor.

## [0.2.0-beta.6] - 2026-10-29

### Added

- Help ▸ Check for Updates… and Settings ▸ About ▸ Check for updates. On
  that click only, Contour asks GitHub Releases for the newest release,
  pre-releases included and drafts skipped, and shows its version with
  View release (the release page: notes and downloads), "You're up to
  date", or a short error with Try again (its own message when GitHub
  rate limits the network). Nothing is checked or downloaded on its own.
  Keyboard and screen reader friendly; in every UI language.

### Changed

- The Windows upgrade smoke test runs for releases and manual runs, not
  on pull requests. Fetching the previous `.msi` counted as a download, so
  pull requests made up most of the `.msi` download count.
- The chat's Chat mode is called Ask in English, so a phone-width Studio
  no longer shows two "Chat" controls (the Chat/Preview/Files switch and
  the mode tabs).
- Home's example briefs read "e.g. …", so they do not look like an
  instruction.

### Fixed

- OpenCode 2 works. Onboarding's agent test and every run failed with
  "Unrecognized flag: --dir": OpenCode 2 dropped `--dir` and takes the
  project from its working folder. Contour tells the two apart by
  `opencode run --help`: 2.x runs with `--standalone` (a private server,
  so the run's MCP settings apply and no background service is left
  behind) and `--auto` instead of `--dangerously-skip-permissions`; 1.x
  keeps `--dir`; when the help does not answer in time, `opencode
  --version` decides. Upgrading OpenCode while Contour runs takes effect
  on the next run (and on Check again) without restarting the app.
  OpenCode 2's tool calls (no `callID`, `path` for the file) show in the
  chat and count as written files.
- Studio on a phone: a deck hides the thumbnail column, so the slide gets
  the width, and Present wraps under the slide arrows instead of past the
  edge. The slide buttons are 44 px touch targets.
- Onboarding on a phone: the brand URL field takes the whole row; the
  Extract button goes under it.
- Integrations on a phone shows all three tabs ("Connect an agent" is the
  shorter label), and the agent launch command wraps inside its card.
- Integrations ▸ Install says what happened with the client's name and
  what to do next ("Contour is now available in Cursor. Restart it to
  load the server.") instead of "installed contour into cursor", and shows
  that it is working while the agent's settings are updated.
- "Saved to … as vN" shows only on the tab of the file that was saved.
- The two PowerPoint exports download under different names:
  `<file>-images.pptx` and `<file>-editable.pptx`.
- A PDF export of a page without a `<title>` is titled after its file,
  not "index.html".
- Template industries read "HR" and "AI", not "Hr" and "Ai", in the
  filter and on the template's detail badge.
- MCP works again after upgrading from 0.2.0-beta.4 or earlier without
  pressing Install again. Those versions registered
  `<app> mcp --daemon-url http://127.0.0.1:<port>` with a port that is
  dead after the app restarts. When nothing answers at `--daemon-url` or
  `OD_DAEMON_URL` within 1 s, `mcp` now uses the running app's URL from
  `daemon.lock` and says so on stderr. See "MCP does not connect after an
  upgrade" in docs/troubleshooting.md.

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

- Integrations ▸ Install (MCP into Claude Code, Codex) works from the
  desktop app. It registered `<app> mcp`, which opened a second window
  instead of serving MCP; the app now runs the MCP stdio server for `mcp`
  (as `od mcp`). The app's port changes per launch, so it registers no
  `--daemon-url`: `mcp` takes `--daemon-url`, then `OD_DAEMON_URL`, then
  the running app's URL from `daemon.lock`. An AppImage registers its
  `.AppImage` file rather than the mount that goes when it quits. The
  Linux and Windows smoke tests run the registered command.
- Folder import, project locations and linked folders refuse the home
  folder on Windows too (they read `HOME`, which Windows does not set).
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

[Unreleased]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.7...HEAD
[0.2.0-beta.7]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.6...v0.2.0-beta.7
[0.2.0-beta.6]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.5...v0.2.0-beta.6
[0.2.0-beta.5]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.4...v0.2.0-beta.5
[0.2.0-beta.4]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.3...v0.2.0-beta.4
[0.2.0-beta.3]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.2...v0.2.0-beta.3
[0.2.0-beta.2]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.1...v0.2.0-beta.2
[0.2.0-beta.1]: https://github.com/quangphung218/contour-design/compare/v0.1.0...v0.2.0-beta.1
[0.1.0]: https://github.com/quangphung218/contour-design/releases/tag/v0.1.0
