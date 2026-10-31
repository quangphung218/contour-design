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
| ONB-01 | First launch goes to onboarding and the found agent is preselected | Empty data folder, mock `claude` on PATH | Open `/` | Redirect to `/onboarding`; Claude is selected and tested; agents not installed sit folded under Other agents and cannot be picked | P0 | auto |
| ONB-02 | Continue with the found agent lands on Home and is remembered | ONB-01 | Continue, reload `/` | Home stays (no second onboarding) | P0 | auto |
| ONB-03 | No agent installed: the user is told what to do | PATH without any agent CLI, no Chrome | Open `/onboarding`; install the mock agent; Check again | No agent is picked; Claude Code and Codex come first, each with a copyable install command and its setup page; the other agents are folded under "Other agents (N)"; the API key path says it makes one HTML file without tools; no brand extraction offered; a line says PDF, PPTX and image export need Chrome; Continue is off; after Check again the new agent is picked | P0 | auto |
| ONB-04 | Skip onboarding with no agent, then send a brief | ONB-03 | Skip for now, brief, Enter | A message names the missing agent or API key and where to fix it | P1 | auto |
| ONB-05 | An API provider cannot be saved with an empty key | ONB-03 | API key ▸ Anthropic, key empty, Save | Save refused; Continue stays off (original requires the key) | P1 | auto (QA-005) |
| ONB-06 | A wrong key saved without testing: the first brief explains the failure | Fake provider answering 401 | Save wrong key, Continue, brief | The failed turn mentions the key / 401 | P1 | auto |
| ONB-07 | The API key is never sent back and its file is private | Key saved | `GET /api/app-config`, `/api/agents`; file mode | Key absent; `app-config.json` is `0600` | P0 | auto |
| ONB-08 | Keyboard only: onboarding can be finished with Tab/Enter | ONB-01 | Tab to Continue, Enter | Home opens | P2 | auto |
| ONB-09 | On a phone, "API key required" sits with the key field and Save stays in place | Viewport 390×844, no agent | API key ▸ Anthropic ▸ empty key | Hint tied to the key field; Save next to Test or at the right edge; no sideways scroll | P2 | auto (QA-016) |
| ONB-09b | Check again after installing an agent detects it | Start without agent, then install one | Press Check again | The new agent is listed and picked, no restart | P2 | auto (in ONB-03; the login shell rescan in `od-server` tests) |
| ONB-10 | Phone width, no agent: the install cards fit | PATH without any agent CLI | Open `/onboarding` at 390 px | No sideways scroll; Copy stays inside its card; Copy and Install guide are at least 44 px high | P2 | auto |
| ONB-11 | Windows, no agent: setup pages only | PATH without any agent CLI, Windows browser | Open `/onboarding` | No install command, nothing about Terminal (Check again is offered, as on the other systems); each card links its setup page; the API key path is offered | P2 | auto |
| ONB-12 | Installed but not signed in | Mock `claude` fails every prompt until signed in | Open `/onboarding`; sign in; Check again | The failed test shows "Then open a new Terminal window and run claude to sign in."; after Check again the test passes and Continue is on | P1 | auto |
| ONB-13 | Installed agent fails its test, not a sign-in issue | Mock `claude` rejects every prompt | Open `/onboarding` at 1440×900 | Claude stays a card; Codex install card and the API key path are shown; no sign-in hint, no brand step; Continue is off; the test result and "Claude Code did not pass its test…" sit under the Claude card, above the install cards, inside the first 900 px | P1 | auto |
| ONB-14 | Brand extraction uses the tested agent | Broken mock `claude`, working mock `codex` | Pick Codex; wait for its test; enter a website; Extract | The extraction request names `codex` as its agent | P2 | auto |

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
| HOME-12 | While design systems load, the kept pick shows a name or "Loading", not a raw id | A system picked on Home, list request held | Reload | The picker never shows the bare id; the pick is kept once loaded | P2 | auto (QA-017) |
| HOME-13 | An address no page answers | — | Open `/nope/page`, Go to Home | Heading "Page not found", a Go to Home button that lands on Home | P2 | auto |
| HOME-14 | One composer, one row of controls | 1440×900 | Open Home, open "+" | No "Recommended… Start creating" strip, no buttons under the composer; New design system, Create plugin and From folder are in "+"; New design system opens the full create dialog | P2 | auto |
| HOME-15 | From folder fits a phone | 390×844 | "+" ▸ From folder | The path field is at least 200 px wide | P2 | auto |

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
| CHAT-11 | Queued message while running goes out after the run | Slow run | Send a second message while running | It is queued, then sent when the first run ends | P1 | exist (studio) |
| CHAT-12 | Messages are named by what was asked | Two turns | Open Messages | Titles are the prompts' first lines, newest first; the state is a badge | P2 | auto |
| CHAT-13 | The chat says what the agent did in words | A turn with a plan | Send "plan…" | The plan shows as the reply's Plan list; tool rows read "Wrote"; files read "1 new"; no `TodoWrite` | P2 | auto |
| CHAT-12 | Daemon dies mid-session: the user is told | Project open | Kill daemon | A visible "cannot reach" notice, not a frozen UI | P1 | exist (preview, recovery tip) |
| CHAT-09 | A message sent right after opening a project shows in the chat | Fresh project | Send as soon as the composer shows | User message and answer are shown | P1 | auto as CHAT-09 (QA-002) |
| CHAT-10 | Switching to another conversation while the first still loads shows only the one picked | Two conversations with one turn each; the first transcript held | Open project, pick the other conversation, let the first answer arrive | Only the picked conversation's messages | P1 | auto (QA-002 guard) |

## 4. Studio: files, preview, editing (`studio.qa.mjs`)

Goal: the user inspects the result, fixes things by hand and goes back in
time when needed.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| STU-01 | A file written outside the app shows up and previews | Project open | `od files write` / API write | File appears in the panel; preview reloads | P0 | auto |
| STU-02 | Upload a file whose name is already used | `logo.png` exists | Upload another `logo.png` | No silent overwrite: renamed copy (`logo-1.png`, as in the original) or a confirm | P1 | auto |
| STU-03 | Upload a file with a Vietnamese name | Project | Upload `ảnh bìa.png` | Letters kept (spaces become `-`, as in the original), the image opens | P1 | auto |
| STU-04 | Delete the file that is open in the preview | `index.html` open | Delete it | Preview shows an empty state, not a broken frame | P1 | auto |
| STU-05 | New folder with an invalid name is refused | Project | New folder "../x" or "" | Refused with a message; nothing created outside the project | P1 | auto |
| STU-06 | Manual text edit is written to the source and versioned | Page open | Manual edit ▸ Content, apply | Source file changed; a "manual" version appears | P0 | exist (preview) |
| STU-07 | Restore an older version | 2 AI versions | History ▸ restore v1 | Preview shows v1; a "restore" version is added; v2 still listed | P0 | auto |
| STU-08 | Comment on an element is sent with the next turn | Page open | Comment mode, pin note, send turn | Agent receives the comment | P0 | exist (studio) |
| STU-09 | Empty comment cannot be added | Comment mode | Pin with empty note | Add disabled or refused | P1 | auto |
| STU-10 | Draw on the preview and undo everything | Page open | Draw, undo | Canvas empty; nothing attached | P2 | exist (studio, files) |
| STU-11 | Device sizes lay the page out at their width | Window 1360×860 | Phone, Tablet, Desktop | The page sees 390 px, 820 px (original preset), then the pane width | P1 | auto (QA-003) |
| STU-12 | Deep link to a project that does not exist | — | Open `/projects/nope` | A clear "not found" with All projects as the only way out; no Retry, no preview steps, composer off; no crash | P1 | auto |
| STU-13 | A page with spaces and Vietnamese letters in the user's own folder previews | Folder with `trang chủ.html` | Open folder, open the file | Preview renders it | P1 | auto |
| STU-14 | With Tablet shrunk to fit, the zoom label shows the real size, also after + | Window 1360×860, Tablet | Read the label once the frame has stopped resizing, press + | Label = drawn size of the 820 px page; + changes the page or is off | P2 | auto (QA-015) |
| STU-15 | Tablet on a phone-width window can still be zoomed in | Window 390×844, Tablet | More, then + | + is on at the fit; the page gets bigger; the pane scrolls | P2 | auto (QA-024) |
| STU-16 | A + that is off says why | Window 1360×860, Tablet | + until off, hover | Tooltip gives the reason, not just "Zoom in" | P2 | auto (QA-025) |
| STU-17 | The Studio names the project | A project | Open it at 1440 and 1024 | The chat head shows the project name (the page heading) above the conversation, at least 100 px wide | P2 | auto |
| STU-18 | The first-design hint goes away by itself | New browser, empty project | First turn, then a second turn without closing the hint | The hint is gone after the second turn and after a reload | P3 | auto |

## 5. Export (`export.qa.mjs`)

Goal: the user takes the design out of Contour.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| EXP-01 | Export HTML gives one self-contained file | Page with CSS and image | Export ▸ HTML | Download has no references to project files; images inlined | P0 | auto |
| EXP-02 | Export ZIP contains every project file | Project with folders | Export ▸ ZIP | Zip lists every file, Unicode names intact | P0 | auto |
| EXP-03 | Export PDF without a usable Chrome says Chrome is the problem | `OD_CHROME_PATH` pointing nowhere | Export ▸ PDF | Error names Chrome (`OD_CHROME_PATH`) | P1 | auto |
| EXP-04 | Export PDF with Chrome gives a PDF | Chrome installed | Export ▸ PDF | A `%PDF` file | P1 | auto (skipped without Chrome) |
| EXP-05 | Export PPTX of a deck | Deck, Chrome | Export ▸ PPTX | A valid zip with one slide per section | P1 | auto (skipped without Chrome) |
| EXP-06 | Export React component as HTML | `.jsx` file | Export ▸ React | Standalone HTML that renders without network | P1 | exist (c2) |
| EXP-07 | Export on an empty project | No files | Open Export | Disabled or a message; not a broken download | P1 | auto |
| EXP-08 | Project name with Vietnamese and `/` gives a safe file name | Name "Bản/thảo: v1" | Export ZIP | Download name has no `/` or `:` and keeps letters | P2 | auto |
| EXP-09 | Esc closes the Export menu | Export menu open | Esc | Menu closes (original closes it) | P2 | auto (QA-004) |
| EXP-10 | Esc that closes the Export menu leaves comment mode on | Comment mode on, Export open | Esc | Menu closes; comment mode stays on | P1 | auto (QA-013) |
| EXP-11 | Esc that closes the Export menu does not stop a running Design Jury | Jury on and running, Export open | Esc | Menu closes; jury still running | P1 | auto (QA-013) |
| EXP-12 | The Export menu lists what applies, grouped | A page open | Open Export, unfold Image and PowerPoint | No greyed formats; no Markdown or JSX for a page; "MP4 video"; Image ▸ PNG, JPEG, WebP; PowerPoint ▸ Slides as images, Editable text | P2 | auto |

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
| LIB-01 | Use a template from the Templates page | — | Templates ▸ card ▸ Use, brief, Enter | Home shows the template; the project's skill is that template | P0 | auto |
| LIB-02 | Template search with no result says so | — | Search "zzqqxx" | No cards and an empty-state message with Clear filters, which brings the templates back | P1 | auto |
| LIB-03 | Design system search with no result says so | — | Search "zzqqxx" | Empty state with text | P1 | auto |
| LIB-04 | Design system detail opens its DESIGN.md | — | Open one ▸ DESIGN.md | The document is shown | P0 | auto |
| LIB-05 | Create a design system with an empty brief is refused | — | Create, brief empty or spaces | Generate disabled | P1 | auto |
| LIB-06 | A plugin opens a detail page with a way to use it | — | Plugins ▸ row | Detail page with Use / Try | P1 | auto |
| LIB-07 | Plugin from a folder that does not exist | — | Create ▸ From folder "/no/such" | Readable error | P1 | auto |
| LIB-08 | Media library upload of a non-media file | — | Upload `.txt` | Refused with a message, or listed under its name; no page error | P2 | auto |
| LIB-09 | Media generate without a provider key points to Providers | No key | Generate tab, prompt | "Needs a key" note with a Providers button that works | P1 | auto |
| LIB-10 | Library pages fit a phone-sized window | 390×844 | Open each page | No sideways scroll | P2 | auto |
| LIB-11 | Uploads do not add a project | — | Media ▸ upload a PNG | Projects and Recent do not list "Media library" | P2 | auto |
| LIB-12 | Design system search reads what the page shows | — | Search "coral", then "curated" | Airbnb found; the package note matches nothing | P2 | auto |
| LIB-13 | Skills say what they make | — | Plugins ▸ Skills, open faq-page | No "Use when…" text for the agent | P2 | auto |
| LIB-14 | Media and Templates labels, Generate tab | No provider key | Media, Generate tab, Templates | "Audio", no second Generate button, selects with arrows, a Set up providers button | P3 | auto |
| LIB-11 | Skills: a user skill overrides a bundled one | — | Create skill with a bundled id | Override notice shown | P2 | exist (library) |

## 8. Settings (`settings.qa.mjs`)

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| SET-01 | Switch language to Tiếng Việt and it persists after reload | — | Settings ▸ Language ▸ vi, reload | UI in Vietnamese after reload | P0 | auto |
| SET-02 | Testing an API provider whose base URL is not a URL | — | Base URL "not a url", Test | A readable error about the URL | P1 | auto |
| SET-03 | Project location that does not exist is refused | — | Add "/no/such" | Readable error; not added, also after reload | P1 | auto |
| SET-04 | Notifications: test notification works without permission crash | — | Notifications ▸ Test | No page error | P2 | auto |
| SET-05 | Settings opened by Cmd/Ctrl+, | Home | Press shortcut | Settings page opens | P1 | auto |
| SET-06 | Permission mode switch is saved | — | Ask first, reload | Still Ask first; API returns `ask` | P1 | auto |
| SET-07 | Settings still save with a keyless Ollama provider stored | BYOK `http://ollama:11434/v1`, no key, in the config | Theme ▸ Dark ▸ Save; Language ▸ vi ▸ Save | Both saves 200, no error; provider unchanged | P0 | auto (QA-018) |
| SET-08 | A keyless Ollama on a one-word host saves | — | API ▸ Ollama ▸ base URL `http://gpu-box:11434/v1` ▸ Save | 200, saved | P1 | auto (QA-018) |
| SET-10 | One Save per thing to save | — | Settings, local agent then API provider; change the theme | One Save in each mode; the theme saves on its own | P2 | auto |
| SET-09 | Check again in Settings ▸ Agent | No agent at start | Install one, press Check again | The agent is listed, install cards gone | P2 | auto |

## 9. Automations (`automations.qa.mjs`)

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| AUT-01 | Create a routine and Run now produces a run in its project | Project | New routine (starts on "New project each run"), pick the project, Run now | History shows a run that succeeded | P0 | auto |
| AUT-02 | Invalid cron is refused with a message | — | Schedule "61 * * * *" | Save refused; message names the schedule | P1 | auto |
| AUT-03 | Empty prompt is refused | — | Save without prompt | Save refused | P1 | auto |
| AUT-04 | Delete a routine removes it from the list and API | Routine | Delete | Gone from list and `GET /api/routines` | P1 | auto |
| AUT-05 | Routine on a project that was deleted | Routine on project P | Delete P, Run now | The run is refused or recorded as failed with "project not found" | P2 | auto |
| AUT-06 | The form resets after editing a routine | A routine on a project | Edit, Save; Edit, Cancel | The form is back on "New project each run" with no name | P2 | auto |

## 10. Work in progress is not lost (`drafts.qa.mjs`)

Goal: what the user typed and did not save yet survives tab switches,
saves of other fields, reloads and navigation; destructive actions ask
first. Expected results follow the original app (composer and Home drafts
in `localStorage`, a leave guard for unsettled manual edits). The original
has no undo for deleting a project, file or conversation; a confirmation is
the guard, so that is what is checked.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| DL-01 | Text typed in Content survives an Apply in Attributes | Edit panel on a paragraph | Type in Content, Attributes ▸ Apply, back | Typed text still there | P1 | auto (QA-006) |
| DL-02 | Text typed in Content survives a style Apply | Same | Type in Content, Style ▸ Apply, back | Typed text still there | P1 | auto (QA-006) |
| DL-03 | Typing while an Apply is saving is kept | Same, save answer held | Apply Attributes, type in Content before the answer | Typed text still there | P1 | auto (QA-006) |
| DL-04 | Closing with a manual edit not applied asks first | Typed in Content | Close the tab | A "leave page?" prompt | P1 | auto (QA-007) |
| DL-05 | Composer draft survives a reload | Project open | Type, reload | Text back in the composer | P1 | auto (QA-008) |
| DL-06 | Composer draft survives going to Projects and back | Project open | Type, Projects, Back | Text back | P1 | auto (QA-008) |
| DL-07 | Home brief and design system survive a trip to Settings | Home | Type, pick system, Settings, Home | Both back | P1 | auto (QA-008) |
| DL-08 | Markdown typed and left at once is saved | `.md` open | Type, click Projects | File has the text; no error | P1 | auto (QA-009 for the panic) |
| DL-09 | Deleting a file asks; Cancel keeps it | File | Delete, Cancel | File unchanged | P1 | auto |
| DL-10 | Deleting a conversation asks; Cancel keeps it | One turn | Delete, Cancel | Messages kept; no error | P1 | auto (QA-010 for the error) |
| DL-11 | Deleting a project asks; Cancel keeps it | Project | Delete, Cancel | Project kept | P0 | auto as PRJ-04 |
| DL-12 | A version overwritten by the agent can be restored | Two versions | History ▸ restore | Older version back, newer kept | P0 | auto as STU-07 |
| DL-13 | Desktop app: Leave on the "edit not applied" question leaves | Typed in Content; `confirm` async as in Contour.app | Click Projects, choose Leave | Projects opens | P1 | auto (QA-011) |
| DL-14 | A sidebar link with an edit not applied asks | Typed in Content | Click Projects: Cancel, then OK | Cancel stays with the text; OK leaves | P1 | auto |
| DL-15 | Command palette ▸ Projects with an edit not applied asks | Same | Cmd/Ctrl+K ▸ Projects: Cancel, then OK | As DL-14 | P1 | auto (QA-012) |
| DL-16 | Command palette ▸ Settings with an edit not applied asks | Same | Cmd/Ctrl+K ▸ Settings: Cancel, then OK | As DL-14 | P1 | auto (QA-012) |
| DL-17 | Quick menu ▸ a Settings section with an edit not applied asks | Same | Quick menu ▸ Models: Cancel, then OK | As DL-14 | P1 | auto (QA-012) |
| DL-18 | The chat's Settings button with an edit not applied asks | A failed turn; typed in Content | Settings chip: Cancel, then OK | As DL-14 | P1 | auto (QA-012) |
| DL-19 | A message queued during a run survives a reload | A long run going | Queue a message, reload | Message still queued, back in the composer, or sent after the run | P1 | auto (QA-014) |
| DL-20 | A steered message is not back in the composer after a reload | A long run going | Queue a message, Steer, reload | Composer empty | P1 | auto (QA-019) |
| DL-21 | A sent and answered message is not back in the composer after a reload | Project open | Send, wait for the answer, reload | Composer empty | P1 | auto |
| DL-22 | Duplicate project with an edit not applied asks first | Typed in Content | "…" ▸ Duplicate: Cancel, then Leave | The app's question; Cancel stays with the text; Leave opens the copy | P1 | auto (QA-020) |
| DL-23 | Ctrl/Cmd-click on a link with an edit not applied | Typed in Content | Ctrl/Cmd-click Projects | New tab on Projects; this tab stays with the text, no question | P2 | auto (QA-021) |
| DL-24 | The browser's Back with an edit not applied asks | Came from Projects; typed in Content | Back: Cancel, then OK | As DL-14 | P1 | auto (QA-022) |
| DL-25 | The leave question opens with the focus on Cancel | Typed in Content | Click Projects | Focus on Cancel; Enter stays | P2 | auto (QA-023) |
| DL-26 | A failed Duplicate after Leave keeps the guard | Typed in Content; `/duplicate` answers 500 | "…" ▸ Duplicate ▸ Leave, then reload | Error shown, page stays; the reload still asks | P2 | auto (QA-020) |

## Not automated, and why

- **Desktop shell (Tauri)**: native menus, Cmd+O folder dialog, Finder
  launch PATH and single-instance with `od`. Playwright drives the browser
  UI only; the desktop app needs a manual pass on a built `Contour.app`.
- **Real agents and real BYOK providers**: the suite uses the mock CLI and
  a fake provider on loopback (no outbound network by design).
- **MP4 export**: needs ffmpeg and a HyperFrames composition; out of scope.
- **ONB-09b in the desktop app**: Check again asks the login shell for its
  `PATH` again only in the app (a CLI daemon has none); the suite covers it
  with a fake shell in `od-server` tests, the real shell needs a manual pass.
- **No Chrome at all**: Chrome is installed on this machine at its standard
  path, so EXP-03 points `OD_CHROME_PATH` at nothing instead; the message
  for a machine without any Chrome ("this export needs Chrome, Edge or
  Chromium") is not exercised. EXP-04 and EXP-05 skip without Chrome.
- **Desktop notifications**: the browser in headless mode never shows OS
  notifications; only the absence of errors is checked.
