# QA bugs: desktop app 0.2.0-beta.20

What only the installed app does (menus, shortcuts, window, native dialogs,
links, drag and drop, export to real files, MCP install), checked on
`/Applications/Contour.app` 0.2.0-beta.20 (macOS 26, Apple silicon) before
beta.21. Numbering follows [bugs.md](bugs.md) (last QA-041).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## How it was run

Never on the owner's data: each run had its own data dir, and the MCP
install ran with a fake `HOME`.

```sh
OD_DATA_DIR=<tmp>/data  /Applications/Contour.app/Contents/MacOS/od-desktop
HOME=<tmp>/fakehome OD_DATA_DIR=<tmp>/data2 /Applications/Contour.app/Contents/MacOS/od-desktop
```

Driven with `osascript` (System Events, aimed at the test app's own pid:
`first process whose unix id is <pid>`), CoreGraphics mouse events (a
small Swift helper: click, scroll, drag) and `screencapture -l<window id>`.
Logs: `<data>/logs/daemon.log` and `log show` (unified log). Screenshots in
[desktop-beta20/](desktop-beta20/).

No automated test: these paths need the packaged app and a real window,
which `make e2e-qa` (browser) does not reach. Each bug lists its manual
steps; the Rust side of QA-042/043/044 can be pinned with the mock-runtime
tests in `apps/desktop/src/main.rs`.

## Status

Unit tests in `apps/desktop/src/main.rs` (desktop) and `crates/od-ui`
(UI); QA-042/043 (also in full screen)/044/048/051 also checked by hand
on the release `.app`.

| ID | Sev | Summary | Test | Status |
|---|---|---|---|---|
| QA-042 | 🔴 | ⌘W in the Studio quits the app | `cmd_w_reaches_the_page_on_macos`, by hand | Fixed |
| QA-043 | 🟠 | Closing the window quits the app; the Dock cannot bring it back | `closing_the_window_hides_it_on_macos`, by hand | Fixed |
| QA-044 | 🟠 | Links with `target="_blank"` do nothing | `web_links_for_a_new_window_go_through_window_open`, by hand | Fixed |
| QA-045 | 🟡 | Network volume prompt on the folder picker | | Fixed in #83 |
| QA-046 | 🟡 | Use everywhere tells app users to run `od` | `the_app_shows_its_own_binary_not_od` | Fixed |
| QA-047 | 🟡 | "Lost the connection" after quitting during a run | | Settled in #72 |
| QA-048 | 🟡 | Export toast names a file that was not written | by hand (native Save dialog, needs the window) | Fixed |
| QA-049 | 🔵 | Native menus stay in English | `the_menu_speaks_the_ui_language`, `the_menu_goes_to_the_shell_in_the_ui_language` | Fixed |
| QA-050 | 🔵 | Shortcut labels disagree | | Fixed |
| QA-051 | 🔵 | ⌘, before onboarding ends runs the agent test again | `coming_back_keeps_the_agent_test_answer`, by hand | Fixed |
| QA-052 | 🔵 | Notifications: "Failure" cut off from its select | | Fixed |

## Results

| Area | Item | Result |
|---|---|---|
| First run | onboarding: agent auto-picked and tested, Continue | Pass |
| First run | agent not logged in: message, install guide, Skip for now | Pass, except the Install guide link (QA-044) |
| Contour menu | About (version shown), Settings… ⌘, | Pass (QA-051 before onboarding ends) |
| File menu | Close Window ⌘W, Close All ⌥⌘W | **Fail** (QA-042, QA-043) |
| Edit menu | Paste, Select All, Copy into a page input | Pass |
| View menu | Toggle Full Screen on and off | Pass |
| Project menu | Open Folder… ⌘O → folder picker → Studio | Pass |
| Window menu | Minimize ⌘M, restore | Pass |
| Help menu | Documentation, Report Issue (clipboard + logs in Finder), Show Data Folder, Keyboard Shortcuts ⌘/, Command Palette…, Check for Updates… | Pass (QA-050 wording) |
| Theme | palette ▸ Theme: dark, window chrome follows, kept after restart | Pass |
| Language | Settings ▸ Language ▸ Tiếng Việt, applies at once | Pass (QA-049 menus) |
| Links | Open in new tab → system browser | Pass |
| Links | `<a target="_blank">` (Install guide …) | **Fail** (QA-044) |
| Drag and drop | Finder file `ghi-chú.md` onto the Files panel | Pass |
| Export | PDF, PNG (1920×1080), PPTX editable (2 slides, Vietnamese text), MP4 → `~/Downloads` | Pass (QA-048 toast) |
| Export | Open project folder → Finder | Pass |
| Settings | Project locations ▸ Add folder (Vietnamese folder name) | Pass (QA-045 prompt) |
| Window | close button, reopen | **Fail** (QA-043) |
| Quit | ⌘Q during a run: agent process stopped, no orphan; turn marked failed with Retry | Pass (QA-047 message) |
| Integrations | Connect an agent ▸ Claude Code ▸ Install; `od-desktop mcp` answers `initialize` | Pass |
| Integrations | Use everywhere commands | **Fail** (QA-046) |
| Updates | Check for Updates: "You're up to date" | Pass (Update now not reachable: no newer release) |

## QA-042 🔴 ⌘W in the Studio quits the app (and stops the running agent)

- **Steps**: open a project in the Studio, with a file tab open (and,
  worse, a run going); press ⌘W.
- **Expected**: the shortcuts dialog (Help ▸ Keyboard Shortcuts, Studio
  section) lists ⌘W as "Close tab": the tab closes, the app stays.
- **Actual**: the app quits. The native File ▸ Close Window (⌘W, from
  Tauri's default menu) takes the key before the page sees it, the last
  window closes and the app exits; `before_exit` stops running agents as
  Stop does. Unified log: `windowShouldClose: prevented close`, then
  `WebPageProxy::close`, process gone 2 s later. ⌥⌘W (Close All) does the
  same.
- **Original**: on macOS the Electron menu has no Close item with ⌘W
  (`Window` gets `role: "front"`, `close` only off macOS:
  `apps/desktop/src/main/index.ts:566`), so ⌘W reaches the page.
- **Code**: `build_menu` (`apps/desktop/src/main.rs:300`) starts from
  `Menu::default`. Drop the File ▸ Close Window/Close All accelerators (or
  route ⌘W to `ui_action(app, "close-tab")` while the Studio is open, and
  to the window otherwise).

## QA-043 🟠 Closing the window quits the app; the Dock cannot bring it back

- **Steps**: click the red close button (or File ▸ Close Window outside the
  Studio) while Contour runs, maybe with a run going.
- **Expected** (original, and the macOS norm): the window hides, the app
  and its runs keep going; clicking the Dock icon shows it again.
- **Actual**: the app exits and running agents are stopped.
- **Original**: `window.on("close")` hides the window on macOS
  (`apps/desktop/src/main/runtime.ts:2723`), `app.on("activate")` shows it.
- **Code**: `main.rs` has no `WindowEvent::CloseRequested` handler
  (prevent close, `window.hide()` on macOS) and no `RunEvent::Reopen`
  (show the window). Combine with QA-042: without it ⌘W would only hide.

## QA-044 🟠 Links with `target="_blank"` do nothing

- **Steps**: first run with an agent that fails its test → "Install guide ↗"
  under Codex CLI; the same for "Get Chrome", the "View" link of an update,
  an image link in a comment.
- **Expected**: the page opens in the system browser.
- **Actual**: nothing happens. `daemon.log`:
  `WARN od_desktop: blocked a navigation away from the app url=https://developers.openai.com/codex/cli`.
  Export ▸ Open in new tab works: it uses `window.open(url, "_blank")`,
  which reaches `on_new_window`; an `<a target="_blank" rel="noopener">`
  reaches `on_navigation` instead and is refused.
- **Code**: `on_navigation` in `main.rs` (`allowed_navigation`). For an
  http(s) URL outside the daemon's origin, open it with `open_in_browser`
  and return false; or have the UI open external links through
  `window.open`. Anchors: `onboarding.rs:226`, `onboarding.rs:494`,
  `updates.rs:282`, `comments_panel.rs:196`.
- **Screenshot**: [41-onb-skip.png](desktop-beta20/41-onb-skip.png).

## QA-045 🟡 "Contour would like to access files on a network volume" on the folder picker

- **Steps**: Settings ▸ Project locations ▸ Add folder (or Project ▸ Open
  Folder…); pick a folder.
- **Actual**: the macOS prompt came up twice in this session, right after
  the picker opened and right after Add folder saved (14:30, test app
  pid 75920). Don't Allow keeps the flow working; the folder is added.
- **Note**: same symptom as the debugger task running today
  (`network-volume-prompt`); listed here for the trigger: the folder
  picker / adding a project location. Not a separate fix.
- **Screenshot**: [20-added.png](desktop-beta20/20-added.png).

## QA-046 🟡 Integrations ▸ Use everywhere tells app users to run `od`, which on macOS is octal dump

- **Steps**: Integrations ▸ Use everywhere; copy `od project list` or
  `od mcp install claude` into Terminal.
- **Expected**: a command that works for someone who installed the app.
- **Actual**: no `od` from Contour is on `PATH`; `/usr/bin/od` (octal
  dump) answers `od: project: No such file or directory`. The "Connect an
  agent" tab shows the right binary
  (`/Applications/Contour.app/Contents/MacOS/od-desktop mcp`).
- **Fix**: in the desktop app, show the bundle path (as Connect an agent
  does) or offer to install an `od` shim.
- **Screenshot**: [43-use-everywhere.png](desktop-beta20/43-use-everywhere.png).

## QA-047 🟡 After quitting during a run the turn says "Lost the connection"

- **Steps**: send a message, press ⌘Q while the agent works, open Contour
  again, open the project.
- **Expected**: the turn says it stopped because Contour was quit, with
  Retry.
- **Actual**: "Failed — Lost the connection to Contour, so this turn was
  cut off. Retry once it is back." The user quit on purpose; nothing is
  "back" to wait for. The agent process was stopped properly (no orphan)
  and Retry is there.
- **Screenshot**: [39-after-quit.png](desktop-beta20/39-after-quit.png).

## QA-048 🟡 Export toast names a file that was not written and never says where

- **Steps**: Export ▸ PDF twice on `index.html`.
- **Expected**: the file lands where the user can find it: a save dialog,
  or a toast with the real name, the folder and "Show in Finder".
- **Actual**: both toasts say "Exported index.pdf"; the second file is
  `~/Downloads/index (1).pdf` (WebKit renames). Nothing says Downloads.
- **Code**: `export_menu.rs` `download()` saves through an `<a download>`
  blob, so the UI cannot know the final name. In the app, a native save
  dialog (`tauri_plugin_dialog` save) would fix both.
- **Screenshot**: [26-pdf2.png](desktop-beta20/26-pdf2.png).

## QA-049 🔵 Native menus stay in English after switching the language

- **Steps**: Settings ▸ Language ▸ Tiếng Việt.
- **Actual**: the page is Vietnamese at once; File/Edit/View/Project/Help
  and their items stay English (Project ▸ Open Folder…, Help ▸ Report
  Issue…).
- **Screenshot**: [48-vi.png](desktop-beta20/48-vi.png).

## QA-050 🔵 Shortcut labels disagree

- Keyboard shortcuts dialog: "Keyboard shortcuts: ?"; Help menu: ⌘/.
  Both work; list both in the dialog.
- The dialog promises ⌘W "Close tab" (see QA-042).
- **Screenshot**: [04-shortcuts.png](desktop-beta20/04-shortcuts.png).

## QA-051 🔵 ⌘, before onboarding ends runs the agent test again

- **Steps**: first run, before Continue, press ⌘, (or Help ▸
  Documentation, then ⌘,).
- **Actual**: the onboarding page comes back and runs the connection test
  again (`connection test agent=claude` twice in `daemon.log`, 14:24:39
  and 14:26:53): a second paid prompt to the agent.
- **Expected**: keep the earlier result while the agent choice is the same.

## QA-052 🔵 Settings ▸ Notifications: the "Failure" label is cut off from its select

- The row reads `Success [ding] Failure`, and the failure sound select
  (`buzz`) wraps to the left under "Completion sound", at the default
  window size (1360×810).
- **Screenshot**: [46-language.png](desktop-beta20/46-language.png).

## Not bugs (checked)

- ⌘Q during a run quits without asking: the original does the same
  (`before-quit` only shuts down).
- Export menu hides PowerPoint and MP4 for a plain page: by design
  (`Format::fits`); a deck shows PowerPoint, a `[data-composition-id]`
  page shows MP4.
- The first drop onto the Files panel did nothing: another app's window
  was on top; with the app in front the drop works.
