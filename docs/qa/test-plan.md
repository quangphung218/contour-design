# QA test plan: business flows → scenarios

This plan maps what a person does with Contour to end-to-end scenarios.
Expected results come from [features.md](../features.md),
[getting-started.md](../getting-started.md), [spec.md](../spec.md) and the
behaviour of the original app, not from the current code. The existing
suite in `crates/od-ui/e2e/*.e2e.mjs` mostly walks the happy paths; this
plan adds the mistakes and accidents of real users. Bugs found are in
[bugs.md](bugs.md).

## Priorities and status

- **P0**: the main flow of an area. If it breaks, the user cannot work.
- **P1**: a common mistake or error branch of a main flow.
- **P2**: an edge case (very long input, narrow window, two tabs…).

Status column: **auto** = automated in `crates/od-ui/e2e/qa/<file>`,
**manual** = not automated (reason given), **exist** = already covered by
the older suite (file named), so not duplicated here.

All scenarios run against a real `od` daemon with the mock `claude` CLI
(`crates/od-ui/e2e/mock-agent/claude`), a temporary data folder and a free
port. Requests leaving loopback are blocked.

Run: `make e2e-qa` (or `crates/od-ui/e2e/qa/run.sh`).

## 1. Onboarding and agent connection (`onboarding.qa.mjs`)

Goal: a first-time user reaches Home with a working agent or API key.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| ONB-01 | First launch goes to onboarding and the found agent is preselected | Empty data folder, mock `claude` on PATH | Open `/` | Redirect to `/onboarding`; Claude is selected; agents not installed are disabled | P0 | auto |
| ONB-02 | Continue with the found agent lands on Home and is remembered | ONB-01 | Continue, reload `/` | Home stays (no second onboarding) | P0 | auto |
| ONB-03 | No agent installed: the user is told what to do | PATH without any agent CLI | Open `/onboarding` | Every local agent is disabled; the screen says how to install one or use an API key; Continue does not lead to a dead Home | P1 | auto |
| ONB-04 | BYOK with an empty key cannot be saved | ONB-03 | Choose API key, leave key empty, save | Save is refused with a message naming the missing key | P1 | auto |
| ONB-05 | BYOK with a wrong key: the connection test explains the failure | Fake provider answering 401 | Enter key, Test | A readable error ("invalid key"/401), not a raw stack or silence | P1 | auto |
| ONB-06 | BYOK key is never shown back by the API | Key saved | `GET /api/app-config` | The key is absent or masked | P0 | auto |
| ONB-07 | Refresh after installing an agent detects it | Start without agent, then add mock to PATH | Press Refresh | (daemon PATH fixed at start) | P2 | manual: the daemon's PATH cannot change after spawn |
| ONB-08 | Keyboard only: onboarding can be finished with Tab/Enter | ONB-01 | Tab to Continue, Enter | Home opens | P2 | auto |

## 2. Create a project from Home (`home.qa.mjs`)

Goal: the user types a brief, picks type, design system and template, and
gets a project that starts working.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| HOME-01 | Brief + Enter creates a Prototype project and starts the first turn | Onboarded | Type brief, Enter | Studio opens; the brief is the first user message; preview shows the page | P0 | exist (studio) + auto |
| HOME-02 | Empty or whitespace-only brief does not create a project | Onboarded | Press Enter on empty / "   " | No project is created, no navigation | P1 | auto |
| HOME-03 | Vietnamese and emoji brief keeps every character | Onboarded | Brief "Trang đặt bàn quán phở 🍜 – giờ mở cửa" | The user message and project name show the exact text | P1 | auto |
| HOME-04 | Very long brief (5,000 chars) works | Onboarded | Paste a long brief, Enter | Project created, message stored whole; layout does not overflow horizontally | P2 | auto |
| HOME-05 | Pressing Enter twice quickly creates one project only | Onboarded | Enter, Enter | Exactly one new project | P1 | auto |
| HOME-06 | Slide deck type creates a deck project | Onboarded | Pick Slide deck chip, brief, Enter | Project kind is deck; the run request carries the deck skill | P0 | auto |
| HOME-07 | Chosen design system is sent with the first run | Onboarded | Pick a design system, brief, Enter | Run request carries `designSystemId`; project shows that system | P0 | auto |
| HOME-08 | Chosen template is sent as the skill of the first run | Onboarded | Pick a template, brief, Enter | Run request carries the template as `skillId` | P0 | auto |
| HOME-09 | Video (Motion) type creates a video project | Onboarded | Pick Video chip, Enter | Project kind is video | P1 | auto |
| HOME-10 | Shift+Enter adds a new line instead of sending | Onboarded | Type, Shift+Enter, type | Brief has two lines, still on Home | P1 | auto |
| HOME-11 | Home at phone width keeps the brief box usable | Viewport 390×844 | Open `/` | No horizontal page scroll; brief box and Create visible | P2 | auto |

## 3. Chat with the agent (`chat.qa.mjs`)

Goal: the user keeps editing a design through chat and stays in control
of runs.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| CHAT-01 | A follow-up turn updates the same design | Project with v1 | Send "make it darker" | Same conversation; preview shows v2 | P0 | exist (studio) + auto |
| CHAT-02 | Stop cancels a long run and the composer is usable again | Slow run | Send "slow", Stop | Message marked canceled; composer enabled; a new turn succeeds | P0 | auto |
| CHAT-03 | Reload during a run keeps the run and its streamed text | Slow run | Reload mid-run | The assistant message comes back and the run ends normally | P1 | auto |
| CHAT-04 | Empty composer cannot send | Project | Press Enter on empty | No user message, no run | P1 | auto |
| CHAT-05 | Agent fails: the user sees an error and can retry | Mock agent exits non-zero | Send "crash" | The message shows failed with a readable reason; a retry or new turn works | P1 | auto |
| CHAT-06 | History survives a daemon restart | Two turns done | Restart daemon, open project | Both turns are in the transcript | P0 | auto |
| CHAT-07 | Two tabs on one project both see a new turn | Project open in 2 tabs | Send from tab A | Tab B shows the new messages after reload at the latest | P2 | auto |
| CHAT-08 | Vietnamese message round-trips through the agent | Project | Send "làm cho tối hơn" | The user bubble and the agent echo show the same text | P1 | auto |
| CHAT-09 | Queued message while running goes out after the run | Slow run | Send a second message while running | It is queued, then sent when the first run ends | P1 | exist (studio) |
| CHAT-10 | Daemon dies mid-session: the user is told | Project open | Kill daemon | A visible "cannot reach" notice, not a frozen UI | P1 | exist (preview, recovery tip) |

## 4. Studio: files, preview, editing (`studio.qa.mjs`)

Goal: the user inspects the result, fixes things by hand and goes back in
time when needed.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| STU-01 | A file written outside the app shows up and previews | Project open | `od files write` / API write | File appears in the panel; preview reloads | P0 | auto |
| STU-02 | Upload a file whose name is already used | `logo.png` exists | Upload another `logo.png` | No silent overwrite: renamed copy or a confirm | P1 | auto |
| STU-03 | Upload a file with a Vietnamese name | Project | Upload `ảnh bìa.png` | Listed with the exact name and opens | P1 | auto |
| STU-04 | Delete the file that is open in the preview | `index.html` open | Delete it | Preview shows an empty state, not a broken frame | P1 | auto |
| STU-05 | New folder with an invalid name is refused | Project | New folder "../x" or "" | Refused with a message; nothing created outside the project | P1 | auto |
| STU-06 | Manual text edit is written to the source and versioned | Page open | Manual edit ▸ Content, apply | Source file changed; a "manual" version appears | P0 | exist (preview) |
| STU-07 | Restore an older version | 2 AI versions | History ▸ restore v1 | Preview shows v1; a "restore" version is added; v2 still listed | P0 | auto |
| STU-08 | Comment on an element is sent with the next turn | Page open | Comment mode, pin note, send turn | Agent receives the comment | P0 | exist (studio) |
| STU-09 | Empty comment cannot be added | Comment mode | Pin with empty note | Add disabled or refused | P1 | auto |
| STU-10 | Draw on the preview and undo everything | Page open | Draw, undo | Canvas empty; nothing attached | P2 | exist (studio, files) |
| STU-11 | Device sizes switch the preview width | Page open | Mobile, Tablet, Desktop | The frame width changes accordingly | P1 | auto |
| STU-12 | Deep link to a project that does not exist | — | Open `/projects/nope` | A clear "not found" with a way back, no crash | P1 | auto |
| STU-13 | File path with spaces and Unicode previews | `trang chủ.html` | Open it | Preview renders it | P1 | auto |

## 5. Export (`export.qa.mjs`)

Goal: the user takes the design out of Contour.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| EXP-01 | Export HTML gives one self-contained file | Page with CSS and image | Export ▸ HTML | Download has no references to project files; images inlined | P0 | auto |
| EXP-02 | Export ZIP contains every project file | Project with folders | Export ▸ ZIP | Zip lists every file, Unicode names intact | P0 | auto |
| EXP-03 | Export PDF without Chrome says Chrome is needed | `OD_CHROME_PATH` invalid, no Chrome | Export ▸ PDF | Error mentions Chrome, Edge or Chromium | P1 | auto |
| EXP-04 | Export PDF with Chrome gives a PDF | Chrome installed | Export ▸ PDF | A `%PDF` file | P1 | auto (skipped without Chrome) |
| EXP-05 | Export PPTX of a deck | Deck, Chrome | Export ▸ PPTX | A valid zip with one slide per section | P1 | auto (skipped without Chrome) |
| EXP-06 | Export React component as HTML | `.jsx` file | Export ▸ React | Standalone HTML that renders without network | P1 | exist (c2) |
| EXP-07 | Export on an empty project | No files | Open Export | Disabled or a message; not a broken download | P1 | auto |
| EXP-08 | Project name with Vietnamese and `/` gives a safe file name | Name "Bản/thảo" | Export ZIP | Download name has no path separator and keeps letters | P2 | auto |

## 6. Manage projects (`projects.qa.mjs`)

Goal: the user finds, renames, copies, removes and brings in projects.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| PRJ-01 | Rename a project | Project | Rename to "Quán phở 🍜" | New name on the card and in the Studio | P0 | auto |
| PRJ-02 | Rename to empty or spaces is refused | Project | Rename to "  " | Old name kept, message shown | P1 | auto |
| PRJ-03 | Duplicate copies files and keeps the original | Project with index.html | Duplicate | Two projects; the copy has the file; editing the copy leaves the original | P0 | auto |
| PRJ-04 | Delete asks first; Cancel keeps the project | Project | Delete ▸ Cancel | Project still listed | P0 | auto |
| PRJ-05 | Delete removes the project and its card | Project | Delete ▸ confirm | Gone from list and API; its URL shows not found | P0 | auto |
| PRJ-06 | Search finds a Vietnamese name, any letter case | "Quán phở" | Search "phở", then "QUÁN PHỞ" | Found both times (the original is case-insensitive, not accent-insensitive) | P2 | auto |
| PRJ-07 | Search with no match says so | Projects | Search "zzzz" | An empty-state message | P1 | auto |
| PRJ-08 | Import a ZIP export | Valid zip | Import ZIP | New project with the entry HTML opened | P0 | auto |
| PRJ-09 | Import a file that is not a ZIP | `notes.txt` renamed `.zip` | Import ZIP | Readable error, no half project | P1 | auto |
| PRJ-10 | Open a folder that does not exist | — | Open folder "/no/such" | Readable error, no project created | P1 | auto |
| PRJ-11 | Open an existing folder: files are used in place | Folder with index.html | Open folder | Project lists the file; edits land in that folder | P0 | auto |
| PRJ-12 | Delete a folder-backed project keeps the user's folder | PRJ-11 | Delete project | The folder and its files are still on disk | P0 | auto |

## 7. Library (`library.qa.mjs`)

Goal: the user browses and uses templates, design systems, plugins, skills
and media.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| LIB-01 | Templates page lists templates and Use starts a project with it | — | Templates ▸ Use | Home/project with that template selected | P0 | auto |
| LIB-02 | Design system search with no result says so | — | Search "zzzz" | Empty state | P1 | auto |
| LIB-03 | Design system detail opens and shows DESIGN.md | — | Open one | Detail with tokens/DESIGN.md | P0 | auto |
| LIB-04 | Create a design system with an empty brief is refused | — | New ▸ Generate without brief | Generate disabled or message | P1 | auto |
| LIB-05 | Plugins list opens a detail page | — | Plugins ▸ row | Detail page with Use | P1 | auto |
| LIB-06 | Plugin from a folder that does not exist | — | Add from folder "/no/such" | Readable error | P1 | auto |
| LIB-07 | Skills: a user skill overrides a bundled one | — | Create skill with a bundled id | Override notice shown | P2 | exist (library) |
| LIB-08 | Media library upload of a non-media file | — | Upload `.txt` | Refused with a message or listed as file | P2 | auto |
| LIB-09 | Media generate without a provider key tells the user | No key | Generate | "No key" message pointing to Settings | P1 | auto |

## 8. Settings (`settings.qa.mjs`)

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| SET-01 | Switch language to Tiếng Việt and it persists after reload | — | Settings ▸ Language ▸ vi, reload | UI in Vietnamese after reload | P0 | auto |
| SET-02 | BYOK base URL that is not a URL is refused | — | Base URL "abc" | Save refused with a message | P1 | auto |
| SET-03 | Project location that does not exist is refused | — | Add "/no/such" | Readable error; not added | P1 | auto |
| SET-04 | Notifications: test notification works without permission crash | — | Notifications ▸ Test | No page error | P2 | auto |
| SET-05 | Settings opened by Cmd/Ctrl+, | Home | Press shortcut | Settings page opens | P1 | auto |
| SET-06 | Permission mode switch is saved | — | Ask first, reload | Still Ask first; API returns `ask` | P1 | auto |

## 9. Automations (`automations.qa.mjs`)

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| AUT-01 | Create a routine and Run now produces a run in its project | Project | New routine, Run now | History shows a run that succeeded | P0 | auto |
| AUT-02 | Invalid cron is refused with a message | — | Schedule "61 * * * *" | Save refused; message names the schedule | P1 | auto |
| AUT-03 | Empty prompt is refused | — | Save without prompt | Save refused | P1 | auto |
| AUT-04 | Delete a routine removes it from the list and API | Routine | Delete | Gone from list and `GET /api/routines` | P1 | auto |
| AUT-05 | Routine on a project that was deleted | Routine on project P | Delete P, Run now | Readable error or routine marked broken; no crash | P2 | auto |

## Not automated, and why

- **Desktop shell (Tauri)**: native menus, Cmd+O folder dialog, Finder
  launch PATH and single-instance with `od`. Playwright drives the browser
  UI only; the desktop app needs a manual pass on a built `Contour.app`.
- **Real agents and real BYOK providers**: the suite uses the mock CLI and
  a fake provider on loopback (no outbound network by design).
- **MP4 export**: needs ffmpeg and a HyperFrames composition; out of scope.
- **ONB-07** (agent installed while running): the daemon's `PATH` is fixed
  when it starts.
- **Desktop notifications**: the browser in headless mode never shows OS
  notifications; only the absence of errors is checked.
