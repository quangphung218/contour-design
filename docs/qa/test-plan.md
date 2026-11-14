# QA test plan: business flows → scenarios

This plan maps what a person does with Contour to end-to-end scenarios.
Expected results come from [features.md](../features.md),
[getting-started.md](../getting-started.md), [spec.md](../spec.md) and the
behaviour of the original app, not from the current code. The existing
suite in `crates/od-ui/e2e/*.e2e.mjs` mostly walks the happy paths; this
plan adds the mistakes and accidents of real users. Bugs found are in
[bugs.md](bugs.md); the failed-call pass (QA-081 to QA-084) in
[bugs-web-failures.md](bugs-web-failures.md); the web pass after beta.28 (QA-074 to QA-078) in
[bugs-web-beta28.md](bugs-web-beta28.md); the desktop beta.32 pass (QA-085 to QA-088, §18 on
macOS) in [bugs-desktop-beta32.md](bugs-desktop-beta32.md); the web pass after the beta.34
redesign (QA-089 to QA-092, §21) in [bugs-web-beta34.md](bugs-web-beta34.md); the desktop
pass on main before beta.34 (QA-093 to QA-095, §19 and §21 by hand) in
[bugs-desktop-main34.md](bugs-desktop-main34.md); the pass on main after beta.36
(QA-096 to QA-098, §22 and §23) in [bugs-main36.md](bugs-main36.md).

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

The installed app's own daemon, next to the app in use:
`OD_DATA_DIR=<tmp> od-desktop daemon start --port 0`, URL in
`<tmp>/daemon.lock` ([getting started](../getting-started.md#run-the-installed-apps-daemon-without-its-window)).

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
| HOME-06 | Slide deck type creates a deck project | Onboarded | Pick Slide deck chip, no template, brief, Enter | Project kind is deck; the project and the run request carry no skill (as the original) | P0 | auto |
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
| STU-12 | Deep link to a project that does not exist | — | Open `/projects/nope` | A clear "not found" with All projects as the only way out; no Retry, no preview steps, composer off; the preview frame says the same with All projects, not "Nothing to preview yet"; no crash | P1 | auto (QA-080) |
| STU-13 | A page with spaces and Vietnamese letters in the user's own folder previews | Folder with `trang chủ.html` | Open folder, open the file | Preview renders it | P1 | auto |
| STU-14 | With Tablet shrunk to fit, the zoom label shows the real size, also after + | Window 1360×860, Tablet | Read the label once the frame has stopped resizing, press + | Label = drawn size of the 820 px page; + changes the page or is off | P2 | auto (QA-015) |
| STU-15 | Tablet on a phone-width window can still be zoomed in | Window 390×844, Tablet | More, then + | + is on at the fit; the page gets bigger; the pane scrolls | P2 | auto (QA-024) |
| STU-16 | A + that is off says why | Window 1360×860, Tablet | + until off, hover | Tooltip gives the reason, not just "Zoom in" | P2 | auto (QA-025) |
| STU-17 | The Studio names the project | A project | Open it at 1440 and 1024 | The chat head shows the project name (the page heading) above the conversation, at least 100 px wide | P2 | auto |
| STU-18 | The first-design hint goes away by itself | New browser, empty project | First turn, then a second turn without closing the hint | The hint is gone after the second turn and after a reload | P3 | auto |
| STU-19 | Manual edit picks the text clicked inside a marked section | Page whose `<article>` has `data-od-id` (as the real Claude writes) | Manual edit, click a paragraph in it | The paragraph is picked, its text in Content (as in a plain section) | P0 | auto (QA-027) |
| STU-20 | History: a late answer for the file just left is dropped | a.html with 3 versions, b.html with 1 | History on a.html; its list held back; pick b.html; let a.html's answer in | b.html's panel lists only "B only" | P1 | auto |
| STU-21 | History: a version viewed on the file just left is dropped | a.html with 2 versions, b.html with 1 | View an a.html version, held back; pick b.html; let it in | b.html's live page, no "Viewing vN", no Restore of a.html's version | P1 | auto (QA-089) |
| STU-22 | A link and a `window.open` button inside a web component open | Page with an open shadow root holding a `_blank` link and a button | Click each | One tab each, at the right address | P1 | auto |

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
| EXP-13 | Every format offered for a plain page works | A page, not a deck, Chrome | Export ▸ PowerPoint (both), MP4 | Each offered item gives a file; one that does not apply is not offered | P1 | auto (QA-031) |
| EXP-14 | Back in the Studio during an MP4 export, the export still shows | A Motion film, Chrome | Export ▸ MP4, Projects, Back | The running export is shown; a second press does not render again | P1 | auto (QA-075) |
| EXP-15 | An MP4 export, then the Studio left for good | A Motion film, Chrome | Export ▸ MP4, Projects, stay there | One `.mp4` download arrives on Projects; the user is not pulled back | P1 | auto (QA-075) |

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
| PRJ-13 | Sending in a project deleted from another tab | Project open in tab A, deleted elsewhere | Send a message in tab A | "This project was deleted…" with All projects; no "Reconnecting", no HTTP 404, no agent Settings hint | P1 | auto (QA-076) |
| PRJ-14 | A project deleted from another tab while its Studio is open | Project open in tab A, deleted elsewhere | Wait, no send | "This project was deleted…" within seconds; no "Reconnecting"; composer off | P1 | auto (QA-076) |
| PRJ-15 | Attaching a file to a project deleted from another tab | Project open in tab A, deleted elsewhere | Attach a file in the composer | "This project was deleted…"; composer off | P1 | auto (QA-076) |
| PRJ-16 | Uploading in the Files panel of a project deleted from another tab | Project open in tab A, deleted elsewhere | Upload a file in the Files panel | "This project was deleted…"; composer off; no bare "Not found … (HTTP 404)" anywhere | P1 | auto (QA-076, QA-079) |
| PRJ-17 | Restoring a version in a project deleted from another tab | Project with two versions open in tab A, History open, deleted elsewhere | Restore v1 | "This project was deleted…"; Versions and preview gone; no "Restore failed", "Not found:" or "HTTP 404" anywhere on the page | P1 | auto (QA-079) |
| PRJ-18 | Asking in the side chat of a project deleted from another tab | Project open in tab A, Side chat tab, deleted elsewhere | Send "tiêu đề có to quá không? 🤔" | "This project was deleted…"; side chat and its composer gone; no "Not found:" or "HTTP 404" anywhere on the page | P1 | auto (QA-079) |
| PRJ-19 | On a phone, a project deleted from another tab while its Preview is shown | 390×844, Preview tab, deleted elsewhere, no action in tab A | Wait | Chat tab picked, "This project was deleted…" with All projects visible; no preview, no bare 404 | P2 | auto (QA-079) |
| PRJ-20 | On a phone, with the chat folded on a wide window first, a project deleted while Files is shown | Fold the chat at 1360, narrow to 390, Files tab, deleted elsewhere | Wait | Chat tab picked, "This project was deleted…" visible, not an empty screen; no files panel, no bare 404 | P2 | auto (QA-079) |
| PRJ-21 | On a phone, the Files and Preview tabs of a project that is gone | 390×844, project deleted elsewhere, not-found shown on the Chat tab | Tap Files, then Preview | Each tab is hidden or off, or still shows "This project was deleted…"; never a blank pane or "Nothing to preview yet" | P2 | auto (QA-080) |

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
| LIB-15 | A failed image generation is explained in words | OpenAI answering 500, then 401 | Generate | No raw JSON; both name OpenAI and the reason; the 401 has Set up providers, which opens the OpenAI key | P1 | auto (QA-077) |
| LIB-18 | A key refused by fal.ai | fal.ai key and a base URL answering 401 `{"detail": …}` | Generate with flux-dev (fal) | "fal.ai returned an error: <reason>", no raw JSON or `fal submit 401`; Set up providers opens the fal.ai key | P1 | auto (QA-077) |
| LIB-16 | A generated image is not labelled Uploaded | A provider | Generate, open Library | The card does not say "Uploaded" | P3 | auto (QA-078) |
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
| AUT-07 | Add pressed twice saves one routine | — | Fill the form, double-click Add | One routine | P1 | auto (QA-074) |

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
| DL-24 | The browser's Back with an edit not applied asks | Came from Projects; typed in Content | Back: Cancel, then OK; eight fresh tries | As DL-14, every try | P1 | auto (QA-022, QA-033) |
| DL-25 | The leave question opens with the focus on Cancel | Typed in Content | Click Projects | Focus on Cancel; Enter stays | P2 | auto (QA-023) |
| DL-26 | A failed Duplicate after Leave keeps the guard | Typed in Content; `/duplicate` answers 500 | "…" ▸ Duplicate ▸ Leave, then reload | Error shown, page stays; the reload still asks | P2 | auto (QA-020) |
| DL-27 | A preview reload does not take the browser's Back | Came from Projects; typed in Content | The page saved again (preview reloads) ▸ Back: Cancel, then Back: Leave | Each Back asks; Leave goes to Projects | P1 | auto (QA-033) |

## 11. The app itself fails (`resilience.qa.mjs`)

Goal: quitting, a crash, a folder without write access or a full disk
never lose work, never leave half-made things, and say what happened in
the user's words.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| RES-01 | Quit while the agent works stops the agent | A turn running (agent that ignores a closed pipe, as Node does) | Quit (SIGTERM) | Nothing is written into the project after the quit (features.md: quitting stops every running agent) | P0 | auto (QA-026) |
| RES-02 | A turn cut short by a crash says so | A turn running | Crash (SIGKILL), open again | Turn failed, words about Contour closing, Retry; no "daemon", no "check the agent in Settings" | P1 | auto (QA-030) |
| RES-03 | A folder that cannot be written to leaves no half project | Folder mode 555 | Open folder twice | Opens, or an error and no new project | P1 | auto (QA-028) |
| RES-04 | A folder that cannot be written to is explained | Same | Open folder | Plain message about write access, no OS error text | P1 | auto (QA-029) |
| RES-05 | Full disk: nothing lost, plain message | Data folder on a full 40 MB disk image | Create a project; send a message; reload | Message back in the composer, files intact; a plain "disk full" message | P1 | manual (QA-029: needs `hdiutil`) |

## 12. Contour as an MCP server (`mcp.qa.mjs`)

Goal: the user's coding agent works on Contour projects through `od mcp`.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| MCP-01 | Create, write, read; delete needs confirm | Daemon running; client on `od mcp` (stdio) | `create_project` "  Quán phở 🍜  ", `write_file`, `get_file` by name, `write_file ../x`, `delete_project` without confirm | Name trimmed, emoji kept, project in the app; file read back; `../` refused; delete refused | P0 | auto |
| MCP-02 | A name with a space reads back | Same | `write_file "trang chủ.html"`, `get_file` same name | The file | P2 | auto (QA-032) |

## 13. Templates added on 2 Nov (`templates.qa.mjs`)

Goal: a user finds the 48 new templates (18 Motion films, 10 WebGL pages,
14 decks, 6 landing pages), previews them, and starts a project from one
with Use template or Remix; a film ends as an MP4. Expected results from
CHANGELOG 0.2.0-beta.13/14 and features.md (Templates).

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| TPL-01 | The Motion industry lists all 31 films | Bundled content | Templates ▸ Industry Motion | Option reads "Motion"; exactly the 31 films (18 from 2 Nov, 13 from 6 Nov), each typed Video | P0 | auto |
| TPL-02 | Type chips find the new pages and decks | Same | Prototype; then Deck | All 10 WebGL and 6 landing pages under Prototype, no deck; all 14 decks under Deck | P0 | auto |
| TPL-03 | Filters that match nothing | Same | Industry Motion + type Deck; Clear filters | Empty state; Clear filters brings every template back | P1 | auto |
| TPL-04 | Search ignores case | Same | Motion, search "LOGO" | Logo reveal found | P2 | auto |
| TPL-05 | A film plays in the sandboxed preview | Same | Open Kinetic type | The frame moves (no scripts in the preview) | P0 | auto |
| TPL-06 | WebGL pages show their poster | Same | Open each of the 10 | Visible text in each preview | P0 | auto |
| TPL-07 | Decks show their first slide | Same | Open each of the 14 | Text visible, nothing wider than the frame | P0 | auto |
| TPL-08 | Keyboard only | Same | Focus a Motion card, Enter, Esc | Preview opens, then closes | P2 | auto |
| TPL-09 | Phone width | 390×844 | Motion filter, open a film | No sideways scroll; Use template on screen | P2 | auto |
| TPL-10 | Use template on a film | Same | Preview ▸ Use template, Enter | Home under Motion, the film picked, brief = example prompt; project with the template as skill, kind video, model hyperframes-html | P0 | auto |
| TPL-11 | Use template on a page or deck | Same | WebGL, landing, deck | Prototype, Prototype, Deck chip | P0 | auto |
| TPL-12 | Another chip after Use template on a film | TPL-10 | Pick Prototype, send | The film template is not sent; the project is not a video | P1 | auto |
| TPL-13 | A link to a template that does not exist | — | Open `/?template=motion-does-not-exist` | Plain Home, nothing picked, no error | P1 | auto |
| TPL-14 | Remix a film | Same | Preview ▸ Remix | Project opens on the film page; Export offers MP4 | P0 | auto |
| TPL-15 | Remix pressed twice | Same | Double-click Remix | One project | P1 | auto |
| TPL-16 | A remixed film is tagged like a film | Remix a film | Home recent card | "Motion", as a film started with Use template or the Motion chip (UI-013) | P1 | auto (QA-034) |
| TPL-17 | Export ▸ MP4 of a remixed film | TPL-14, Chrome + ffmpeg | Export ▸ MP4 video | h264 MP4 as long as `data-duration` | P0 | auto |
| TPL-18 | `od media generate` as the film's skill says | Chrome + ffmpeg | `media scaffold`, copy example, `media generate` | `film.mp4`, h264 1920×1080, length = `data-duration` | P0 | auto |

## 14. Files from this turn, written with shell commands (`turnfiles.qa.mjs`)

Goal: files the agent makes with `ffmpeg`, `python` or `cp` show under the
turn like files from the Write tool, frames fold into one row, counts are
in files, and two runs of one project do not swap files. Expected results
from CHANGELOG 0.2.0-beta.12/14. The QA mock answers `qa-sh <shape>` with
plain shell writes.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| TF-01 | A shell-written file opens from the turn | Empty project | `qa-sh name` ("báo cáo 🍜.txt") | One row, New, "1 new"; Open shows it; Download gives it | P0 | auto |
| TF-02 | A shell edit is an edit | `notes.txt` exists | `qa-sh edit` | One row, Edited, "1 edited" | P0 | auto |
| TF-03 | 30 frames fold into one row | Empty project | `qa-sh frames-30` (+ film.mp4 there) | `frames/ (30 files)` and `film.mp4` rows; "31 new"; "Done · 31 files written"; the film opens | P0 | auto |
| TF-04 | 20 frames do not fold | Same | `qa-sh frames-20` | 21 single rows; "+N more" = rows not shown | P1 | auto |
| TF-05 | Two kinds, none past 20 | Same | `qa-sh kinds` (11 PNG + 10 JPG) | 21 rows | P2 | auto |
| TF-06 | More than 50 rows | Same | `qa-sh rows-60` | 50 rows, "+10 more files not listed"; header and Done both count 60 | P2 | auto (QA-036) |
| TF-07 | Reload keeps the folder row | TF-03 shape (25) | Reload | `frames/ (25 files)`, "26 new", "26 files written" | P1 | auto |
| TF-08 | Vietnamese UI | locale vi | `qa-sh frames-22` | `frames/ (22 file)`, "23 mới", "Đã ghi 23 file" | P2 | auto |
| TF-09 | Two runs at once: own files only | Two tabs, two conversations | A writes `a.txt` by shell after B ended; B writes `index.html` with Write | A lists `a.txt` only; B lists `index.html` only | P0 | auto |
| TF-10 | Two runs at once, both by shell | Same | A writes at once and keeps running; B writes after A ended | Neither lists the other's file; B lists `b.txt` | P0 | auto |
| TF-11 | A later turn with no files | TF-03 shape | Second turn writes nothing | No "Files from this turn" | P1 | auto |
| TF-12 | No "without writing any file" beside listed files | Empty project | `qa-sh page` (index.html by shell) | Turn lists index.html, preview shows it, no deliverable warning | P1 | auto (QA-035) |
| TF-13 | Stop after a shell render | Empty project | `qa-sh early-render.mp4-20`, Stop | Canceled turn still lists `render.mp4` | P1 | auto |

## 15. Previews ask nothing off loopback (`templates.qa.mjs`)

Goal: no template preview (old or new) loads fonts, scripts or images from
the internet. The harness blocks and records any request whose host is not
`127.0.0.1`, `localhost` or `[::1]`; any such request fails the test. Random
picks are seeded by `QA_SEED` (printed) so a red run can be replayed.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| NET-01 | The whole gallery | Bundled content | Scroll every card into view | No request off loopback | P0 | auto |
| NET-02 | 24 random previews | Same | 12 new + 12 older: open the preview and Open in new tab (scripts on) | No request off loopback | P0 | auto |
| NET-03 | 6 random remixes | Same | 3 new + 3 older: Remix, open the studio preview | No request off loopback | P0 | auto |
| NET-04 | Every example on the installed app | `Contour.app` 0.2.0-beta.14 running | Load all 164 examples with scripts on | No request off loopback, no page error | P0 | manual (scratch script, round 5) |

## 16. A reference video for Motion (`reference.qa.mjs`)

Goal: a user picks Motion on Home, attaches a film and asks "Dựng lại phim
này"; the agent gets the motion-design skill's reference steps, runs
`od media reference` (and, after a render, `od media compare`), and the
project gets `reference/<name>/` with `facts.json`, `scenes/`, `grid/`,
`contact-sheet.png` and `audio.m4a` when the film has sound. Mistakes
(a link, too long, too large, not a video) reach the chat with what to do
next; `--out` never removes or replaces the user's files. Expected results
from the skill (`content/skills/motion-design/SKILL.md`, "When the user
attaches a reference video") and `docs/features.md` (CLI reference).

The QA mock answers `qa-ref [compare] [out=<dir>] [cmp-out=<dir>]
[file=<path>]` (`mock-agent/qa-ref.mjs`): for each attached file, link or
`file=`, it runs `"$OD_NODE_BIN" "$OD_BIN" media reference "<path>"
--project "$OD_PROJECT_ID"` as the skill writes it, shows it as a Bash
call and quotes what the command printed; with `compare` it copies the
film as `film.mp4` (a render stand-in) and runs `media compare … --points 4
--out reference/<name>/compare`. It also says whether the skill's
reference steps reached it. Films are made at start with ffmpeg's lavfi
sources; without ffmpeg every test fails.

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| REF-01 | Landscape MP4 | ffmpeg | Home → Motion → attach `ngang.mp4` (10 s, 640×360, cuts at 4 and 7 s, sound) → brief | Skill steps reached the agent; `reference/ngang/` has facts (10 s, cuts 4 and 7, sound), 3 scene frames, 10 grid frames, contact sheet, `audio.m4a` | P0 | auto |
| REF-02 | Portrait phone clip | Same | Attach `doc-xoay.mp4` (stored 640×360, turned 90°) | Facts 360×640; scene and grid frames are portrait | P0 | auto |
| REF-03 | MKV, sound longer than picture | Same | Attach `tieng-dai.mkv` (6 s picture, 15 s sound) | 6.00 s; every grid frame before 6 s; `audio.m4a` | P0 | auto |
| REF-04 | Score the render | Same | `qa-ref compare` | 4 side-by-side pairs in `reference/ngang/compare/`; same film scores ≥ 0.9; reference folder intact | P0 | auto |
| REF-05 | `--out` at a folder of the user's | `mine/notes.md`, `mine/001.png` | `qa-ref out=mine` | Refused ("already holds files … never removes or replaces"); both files unchanged, nothing added | P0 | auto |
| REF-06 | `--out` at the user's page | `index.html` | `qa-ref out=index.html` | Refused; `index.html` unchanged | P0 | auto |
| REF-07 | Run again, user notes kept | REF-01 done; user adds `beats.md`, `scenes/sketch.png` | Run the reference again | Both kept; folder complete again | P0 | auto |
| REF-21 | Run again, user's `scenes/010.png` kept | REF-01 done; user saves `scenes/010.png` | Run again | `010.png` kept | P1 | auto (QA-041) |
| REF-08 | Compare `--out` at a folder of the user's | `mine/notes.md` | `qa-ref compare cmp-out=mine` | Refused; `mine/` unchanged | P0 | auto |
| REF-09 | Over 180 s | ffmpeg | Attach `dai-185s.mp4` | "185.0 s long … at most 180 s. Cut the part to follow"; no ffprobe noise; no folder left | P1 | auto |
| REF-10 | Over 200 MB at attach | Same | Attach `to-210mb.mp4` on Home | "to-210mb.mp4 is larger than 200 MB" on Home; nothing staged | P1 | auto |
| REF-11 | Over 200 MB already in the project | File copied in by Finder (upload refuses it) | `qa-ref file=to-210mb.mp4` | "210 MB … at most 200 MB. Trim or compress"; no folder left | P1 | auto |
| REF-12 | YouTube link | — | Brief with `https://www.youtube.com/watch?v=…` | "does not download videos: save the file … attach"; no folder | P1 | auto |
| REF-13 | Link without `https://` | — | Brief with `youtube.com/watch?v=…` | Same refusal as REF-12 | P1 | auto (QA-038) |
| REF-14 | Text file named .mp4 | — | Attach `ghi-chu.mp4` | "not a video"; no ffprobe internals, OS errors or `file:` paths | P1 | auto (QA-037) |
| REF-15 | PDF by mistake | — | Attach `brief.pdf` | Same as REF-14 | P1 | auto (QA-037) |
| REF-16 | An image | — | Attach `anh.png` | "not a video"/"still image"; no "ffmpeg -i" advice | P2 | auto (QA-037) |
| REF-17 | MP3 without art | — | Attach `nhac.mp3` | "has no video stream"; no folder | P2 | auto |
| REF-18 | Song with cover art | — | Attach `bai-hat.mp3` | Not a video; no empty folder left | P2 | auto (QA-039) |
| REF-19 | Two films, same name | — | Attach `clip.mp4` and `clip.mkv` together | Two reference folders, one per film | P2 | auto (QA-040) |
| REF-20 | Vietnamese name with spaces | — | Attach `phim mẫu 01.mp4` | `reference/phim-mẫu-01/` (NFC), facts name the file | P2 | auto |
| REF-22 | Two runs of one film at once, both through the daemon | Project with `phim.mp4` (170 s, cuts) | `od media reference phim.mp4 --project <id>` twice, the second while the first holds `.od-running` | Second: "another run is writing `<dir>`; wait for it to finish", exit 1, no `400 BAD_REQUEST`; first: full folder, every frame `facts.json` lists is on disk, none else | P0 | auto |
| REF-23 | Two runs at once: the agent (daemon) and a terminal (CLI here) | Same | Daemon run first; `od media reference phim.mp4` in the project folder while it holds the lock | Same as REF-22, across two processes | P0 | auto |
| REF-24 | One film given five ways keeps one folder | `films/clip.mp4` in a project folder | Run from the folder with `films/clip.mp4`, its absolute path, `films/../films/clip.mp4`, `FILMS/CLIP.MP4`; from `films/` with `--out ../reference/clip`; through `--project`; then the folder moved and run again | Every run succeeds into `reference/clip`; no `reference/clip-mp4` or other folder | P0 | auto |
| REF-25 | Two films named `clip.mp4` in two folders | `a/clip.mp4`, `b/clip.mp4` (different films) | Chat: `qa-ref file=a/clip.mp4`, then `file=b/clip.mp4`, then each again | Two folders (`reference/clip`, `reference/clip-mp4`), facts of each film; runs again stay in their folder | P1 | auto |
| REF-26 | A beta.17 folder (mark names only the file, other case) | `reference/CLIP/` with a beta.17 mark `source: CLIP.MP4`, its facts and frames, the user's `notes.md`; film moved to `a/clip.mp4` | Run `a/clip.mp4`, then `b/clip.mp4` | `a` takes `reference/CLIP` back (frames replaced, `notes.md` kept, mark names `a/clip.mp4`); `b` goes to `reference/clip-mp4` | P1 | auto |
| REF-27 | Same, folder written by the installed beta.17 app | `Contour.app` 0.2.0-beta.17 (`od-desktop media reference CLIP.MP4` in the project) | As REF-26 | As REF-26 | P1 | auto (needs the app) |
| REF-28 | Links typed wrong | — | Chat `qa-ref file=…` and CLI with `https//x.com/1`, `http:/youtu.be/x`, `vimeo.com`, `www.example.com/v` | "is a link … does not download … attach"; no `os error`, no `400`; no folder. A missing `clip.mov` is still told as missing | P1 | auto |
| REF-29 | The terminal run killed (kill -9) half way | `phim.mp4` | CLI run, kill -9 once its first frame is on disk, run again | Second run is not refused as busy; the folder holds exactly the frames `facts.json` lists; no `.od-running`, `.od-mark-*` left | P1 | auto |
| REF-30 | The daemon killed (kill -9) during an agent's run | Same, `--project` | kill -9 the daemon once a frame is on disk; start it; run again | As REF-29 | P1 | auto |
| REF-31 | A beta.17 run killed half way, then the update | `Contour.app` 0.2.0-beta.17 | `od-desktop media reference phim.mp4`, kill -9 after its first frame; the new `od` runs it again | Whole folder, no orphan frame (beta.17 has no `.od-running`; its mark lists the frames it began) | P2 | auto (needs the app) |
| REF-M1 | The installed app's binary | `Contour.app` 0.2.0-beta.16 | `od-desktop media reference` / `compare` on the same films, `--out` at user folders and files | Same results as the suite (manual, round 6: its daemon cannot be started apart from the GUI) | P0 | manual |
| REF-M2 | A 170 s 1080p film, 181 MB | Same | `od-desktop media reference` | Done in 15 s, well under the CLI's 120 s wait for the daemon | P2 | manual |

Not covered: a real LLM choosing to run the commands (the mock follows
the skill to the letter; whether Claude does is a model question, see
round 4 in bugs.md for how a real pass is done); a film between 200 MB
minus the multipart overhead and 200 MB, where the composer admits the
file and the upload's 200 MiB body limit may refuse it.

## 17. Desktop app beta.21 (manual, [bugs-desktop-beta21.md](bugs-desktop-beta21.md))

The beta.21 features on the packaged app: the 13 new Motion templates
(Use template, Remix, Studio preview, Export ▸ MP4 size and length), a chat
turn as one timed task (clock while running, frozen after, across a
reload; reply and files outside), comments (add, pick, pick again scrolls
back, Retry after a failed reload), ⌘W and the red button hiding the window
and the Dock bringing it back (also from full screen and during a run),
`target="_blank"` links in the app and in the preview, Help ▸ Check for
Updates. Each row there has its priority (P0 main flow, P1 common error
path, P2 edge) and result; the parts that need no native window have
Playwright scripts in `desktop-beta21/` run against the app's own daemon.

## 18. Desktop app: links and popups from a design (manual)

The shell's `on_new_window` (`apps/desktop/src/main.rs`) has no test: a
Tauri window cannot run headless. `preview.e2e.mjs` covers the bridge
with a stub shell in Chromium; `bridge.e2e.mjs` runs the bridge alone in
Chromium and WebKit (`node crates/od-ui/e2e/bridge.e2e.mjs`, about 10 s,
after `npx playwright install chromium webkit` in `crates/od-ui/e2e`).
On a built app, on each of macOS
(WKWebView), Windows (WebView2) and Linux (WebKitGTK):

- [ ] **on_new_window always Deny**: a design's `window.open`, a
  `target="_blank"` link and a ⌘ / Ctrl-click never open a second app
  window; a web address goes to the system browser, anything else does
  nothing.
- [ ] **1 click = 1 tab**: a button whose page opens a different URL on
  `pointerup`, `mouseup` and `click` opens one browser tab; Enter on a
  button with `window.open` on `keydown` and `keyup` opens one; a held key
  opens one.
- [ ] A `window.open` on load or on a timer opens nothing.
- [ ] Tab out of the design to the app, then Shift+Tab back into a field
  whose page opens a URL on `keyup`: nothing opens.
- [ ] In comment mode, clicking that button picks it and opens nothing.
- [ ] A button whose `click` runs `if (confirm('Open?')) window.open(url)`:
  OK opens one tab; the same with `alert('…')` or `prompt('…')` before
  the `window.open`. Then let go of a key in a field whose page opens a
  URL on `keyup`: nothing opens. Also after Enter in a field whose page
  runs `alert('…')` on `keydown` (close it with Enter). The app's own
  dialog may take focus from the window, which headless browsers' do not
  (`bridge.e2e.mjs` D1–D3 and X1–X3 move the focus by hand).
- [ ] **JS dialogs** (QA-064; macOS: `apps/desktop/src/js_dialogs.rs`):
  `alert('About to open')`, `confirm('Open?')` and `prompt('Name?', 'x')`
  in a design each show a dialog titled "This page says" (the design's
  sandboxed frame; a page of the app's origin shows `127.0.0.1:<port>
  says`) with the message. `confirm`: OK returns `true`, Cancel and Escape
  `false`. `prompt`: the field holds `x` and has the focus; OK returns what
  it holds, Cancel `null`. The page waits until the dialog closes. A
  message of 5000 characters shows its first 2000 and `…`, the buttons on
  screen.
- [ ] **prompt, long text**: in `prompt('Tên?')` type a Vietnamese string
  longer than 15 characters (`Nguyễn Văn An ở Hà Nội`) and press OK: the
  page gets exactly that string (log it or put it in the page).
- [ ] **alert loop**: a button running `for (;;) alert('x')`. The first
  alert has no checkbox; the next ones (within 3 s of the last) show
  "Don't allow this design to show more dialogs". Tick it and press OK: no
  more dialogs, the app answers (menus, ⌘Q). After that the design's later
  `confirm` gives `false` and `prompt` `null`, nothing shown. Reload the
  app: dialogs show again.
- [ ] **blocked in design A, not in B**: block dialogs in design A as in
  the line above, then open design B (Projects or the switcher, no
  reload) and press a button running `confirm('Delete?')`: the dialog
  shows, OK returns `true`. Back to A, nothing reloaded: A's dialogs show
  again too (opening another design resets the block,
  `reset_dialog_gate`).
- [ ] Drag an element of the design (`draggable="true"`) and drop it, then
  let go of a key in a field whose page opens a URL on `keyup`: nothing
  opens.

macOS, beta.32 (2026-11-12): every row above passes
([bugs-desktop-beta32.md](bugs-desktop-beta32.md#18-links-and-popups-macos-window)).
The boxes stay open for Windows and Linux, not run.

## 19. Desktop app: a page that hangs or crashes (manual, macOS)

The shell's answers to a hung or crashed page (QA-067, QA-070 to QA-072;
`apps/desktop/src/main.rs`, `web_process.rs`). Rows from
[bugs-desktop-beta26.md](bugs-desktop-beta26.md#qa-070-and-qa-071-a-design-that-hangs-the-window),
on a QA copy (bundle id `dev.contour.desktop.qa`, its own `OD_DATA_DIR`).

- [ ] **Busy page, Wait** (QA-072): [dialogs25.html](desktop-beta25/dialogs25.html)
  BUSY 20s, ⌘R, Wait in "This page isn't responding". The question may show
  again 5 s later; once BUSY ends and the page reloads, that question
  closes by itself within a second, nothing pressed
  ([busywait.sh](desktop-beta26/busywait.sh): no layer-8 window after the
  reload). A page's own dialog shown above it is never closed by this.
- [ ] **A page's alert above "isn't responding"**:
  [dialogs34.html](desktop-main34/dialogs34.html) BA (busy 15 s, then
  `alert`), ⌘R, with and without Wait
  ([busy34.sh](desktop-main34/busy34.sh) `810 289 [779 384] 60`). The
  page's alert shows on top and stays until OK; then both close and the
  page reloads. Keep the mouse still: busy34.sh logs it, and a moved mouse
  means a person may have pressed something.
- [ ] **Backspace outside a field** (QA-093): in the Studio, click an empty
  part of the chat pane, press ⌫: still in the Studio. Same with Telex on,
  typing `first` into a design's preview.
- [ ] **Crash loop** (two crashes within 10 s): start the QA copy with the
  test hook `OD_QA_CRASH_WEB=1` (read only by a debug build or a copy whose
  bundle id ends `.qa`; a release Contour ignores it), for example
  `OD_QA_CRASH_WEB=1 OD_DATA_DIR=<tmp> ContourQA.app/Contents/MacOS/od-desktop`
  (the binary inside the copy). The log says `QA test hook: a design's page
  crashes its web content process`. Open a project with a design, then ⌘R
  (the hook acts on a page load, not on an in-app route change). Each load
  of a `/projects/<id>` or `/present/<id>` page whose preview runs ends the
  window's own web content process, so: the first crash reloads the page,
  the second within 10 s opens it with `?preview=stopped&why=crashed`, and
  the Studio says the design crashed the page twice in a row. The hook does
  not act on that page, so the loop ends there. Run again in the Studio is a
  route change: no crash until the next page load. Only this copy's web
  content process ends; the owner's Contour is not touched. Without the
  hook, [crashloop.sh](desktop-beta28/crashloop.sh) crashes it for real
  (`kill -9` of the copy's own pid, found by CPU), and
  [crash2dlg.sh](desktop-beta28/crash2dlg.sh) does so with a dialog up at
  the second crash. After a crash WebKit reloads the page into its
  prewarmed process, so the newest WebContent pid is a spare, not the page.
  QA-073 (the second crash panicked the app on beta.27) is fixed in beta.28
  ([bugs-desktop-beta28.md](bugs-desktop-beta28.md)).

## 20. When a call fails (`failures.qa.mjs`)

Goal: when Contour cannot do what the user asked (Design Jury, comments,
manual edit, lint, sketch, GenUI, duplicate, transcript export), the
screen says what did not happen in the user's words, keeps what they typed,
and the same action works when tried again. Expected results from the
QA-029 and QA-079 rule (no HTTP status, no "daemon", no JSON in a message)
and from spec.md (nothing the user made is lost). The failure is a
`page.route` that answers the call with the daemon's own 500
(`{"error":{"code":"INTERNAL_ERROR","message":"database is locked"}}`)
until the test lifts it; "deleted from another tab" deletes through the
API as the call goes. Each test checks the retry first and the wording
last. Bugs: [bugs-web-failures.md](bugs-web-failures.md).

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| FAIL-01 | Design Jury switch not saved | Project with a page | Turn the jury on while `PUT …/critique` fails; again after | The switch goes back off with a plain message; the second press turns it on | P1 | auto (QA-081) |
| FAIL-02 | Design Jury Stop refused | Jury running (`qa-jury-slow`) | Stop while `…/interrupt` fails; Stop again | Plain message, Stop can be pressed again, the second stops the jury | P1 | auto (QA-081) |
| FAIL-03 | Comment not saved | Comment mode, element picked | Add while `POST …/comments` fails; Add again | Plain message in the draft, the note kept, the second Add saves | P1 | auto (QA-081) |
| FAIL-04 | Comments list not loaded | Project opened while `GET …/comments` fails | Open All comments, Retry | "Could not load" with Retry; Retry shows the list | P1 | auto |
| FAIL-05 | Manual edit not written | Manual edit, h1 text typed | Apply while `…/source-edits` fails; Apply again | A message that says the edit was not applied, the text kept, the second Apply writes the file | P1 | auto (QA-082) |
| FAIL-06 | Manual edit in a deleted project | As FAIL-05, project deleted in another tab | Apply | The not-found state, no raw "not found" | P1 | auto |
| FAIL-07 | Lint could not run | Page opened while `…/lint/` fails | Look for Lint, open it, Retry | Something says the check could not run; Retry in the Lint panel runs it | P2 | auto (QA-083) |
| FAIL-08 | New sketch not created | Studio | New ▸ Sketch while `POST …/files` fails; again | Plain message, no editor for a file that is not there; the second opens one | P1 | auto (QA-081) |
| FAIL-09 | Sketch not saved | Sketch open | Draw while `PUT …/content/…` fails; draw again | "Not saved"; the next save holds both strokes, "Saved" | P1 | auto |
| FAIL-10 | Sketch could not be read | Sketch with 2 strokes | Reopen while its read fails once; draw | No blank canvas that replaces the file: the 2 strokes stay | P0 | auto (QA-084) |
| FAIL-11 | GenUI answer not sent | A plugin run asking a confirmation | Continue while `…/respond` fails; again | Plain message, the question stays; the second answer goes | P1 | auto (QA-081) |
| FAIL-12 | Duplicate not made | Studio | Duplicate while `POST …/duplicate` fails; again | Stays on the project with a plain message, no copy; the second opens the copy | P1 | auto (QA-081) |
| FAIL-13 | Duplicate of a deleted project | Deleted in another tab as Duplicate goes | Duplicate | The not-found state, no copy made | P1 | auto |
| FAIL-14 | Transcript export failed | One finished turn | Export ▸ Markdown while `…/transcript` fails; again | Plain message; the second downloads `conversation-….md` | P2 | auto (QA-081) |
| FAIL-15 | Markdown could not be read | Project with `notes.md` | Open it while its read fails once; Retry; Reload preview while the read fails | Plain message with Retry, no editor; Retry shows the text, the file unchanged; a failed reload keeps the text, no message | P1 | auto |

Run: `QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh failures` (~40 s).

## 21. Leaving in the middle of work (`leave.qa.mjs`)

Goal: work the user asked for (a save, an upload, a generation) reaches its
end whatever screen they go to next, and what they typed or drew a moment
before leaving is not lost; nothing crashes while views go. Written after
the beta.34 redesign (tasks tied to their owner, ReadState and Autosave,
StudioPhase Gone). Expected results from spec.md (nothing the user made is
lost), the beta.33 and beta.34 changelog entries, and the original app
(`FileWorkspace.tsx` flushes sketch autosaves on `pagehide`). A slow call
is a `page.route` that holds the request until the test lets it go; "within
the autosave delay" pauses the page clock, so the 700 ms timer cannot fire.
Bugs: [bugs-web-beta34.md](bugs-web-beta34.md).

| ID | Scenario | Precondition | Steps | Expected | P | Status |
|---|---|---|---|---|---|---|
| LV-01 | Markdown typed, another document opened at once | a.md, b.md | Type in a.md, click b.md within the delay | a.md has the text, b.md unchanged | P0 | auto |
| LV-02 | A failed Markdown save is saved on closing the tab | notes.md | Type while saves fail, unfocus, heal, close the tab | The text is in the file | P1 | auto |
| LV-03 | Markdown typed, page reloaded | notes.md | Type, reload within the delay | The text is in the file | P1 | auto (QA-090) |
| LV-04 | Sketch stroke, page reloaded | New sketch | Draw, reload within the delay | The stroke is in the file | P1 | auto (QA-090) |
| LV-16 | Markdown typed, Back to another page load | Studio opened by a page load after Projects | Type, Back | The text is in the file | P1 | auto (QA-090) |
| LV-12 | Markdown typed, Back inside the app | Studio opened from the Projects list | Type, Back | The text is in the file | P1 | auto |
| LV-05 | Markdown typed in a project deleted elsewhere | notes.md open; project deleted by API | Type | The not-found state, no raw "not found", no crash | P1 | auto (QA-091) |
| LV-10 | Ctrl+Tab while typing | notes.md and index.html open | Type, Ctrl+Tab, come back, type | Tab switches, both texts saved, no crash | P0 | auto (QA-091) |
| LV-11 | Ctrl+W while typing | notes.md open | Type, Ctrl+W | Tab closed, text saved, no crash | P1 | auto (QA-091) |
| LV-13 | Another file deleted while typing | notes.md, old.md | Delete old.md by API, type | The row goes, typing goes on and saves | P1 | auto |
| LV-14 | Ctrl+Tab while typing a sketch label | New sketch, Text pen | Type a label, Ctrl+Tab | Tab switches, the label kept, no crash | P1 | auto (QA-091) |
| LV-15 | A brief sent with Enter from Home | Home | Type, Enter | The Studio opens, no error | P0 | auto |
| LV-06 | History closed while its list loads | 2 versions | Open History (held), close it, let it in, open again | The list shows, no error | P2 | auto |
| LV-07 | History A → B → A quickly | a.html 2 versions, b.html 1 | b.html's list held back; pick A again; let it in | A's list | P1 | auto |
| LV-08 | Upload, then the Studio left | Studio | Upload `bìa sách.png` (held), go to Projects, let it in | The file is in the project | P0 | auto |
| LV-09 | Media generate, then Library and Home | A fake image provider that answers late | Generate, Library, Home, answer, back to Media | The image is in the Library, the task "done" in Queue | P1 | auto |
| LV-17 | The daemon stops while a tab is open | Home open | SIGTERM the daemon | It stops within 2 s | P2 | auto (QA-092) |

Run: `QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh leave` (~1.5 min).

## 22. Desktop app: one window per data folder (manual, macOS)

Goal: opening Contour while it runs (Dock, Finder, a second copy, `open
-n`) shows the running window; `od daemon start` on the same data folder
gets a dialog the user can act on; Update now restarts into the new
version. Expected results from the beta.36 changelog entry and
[troubleshooting.md](../troubleshooting.md). Run on a QA build whose
*compiled* identifier is `dev.contour.desktop.qa` (`tauri build --config
'{"identifier":"dev.contour.desktop.qa"}'`): the single-instance socket is
`/tmp/<identifier>_si.sock`, so a copy that only renames the bundle id in
`Info.plist` would talk to the owner's Contour. Single instance is on only
without `OD_DATA_DIR`, so the copy's data folder comes from a `HOME` of
its own, passed with `open --env HOME=<qa>/home` ([qa.sh](main36/qa.sh)).
`LSEnvironment` `HOME` in `Info.plist` is ignored: the copy then opens the
owner's data folder. Never launch the copy from Finder or the Dock while it
is not running, for the same reason. Bugs: [bugs-main36.md](bugs-main36.md).

| ID | Scenario | Steps | Expected | P |
|---|---|---|---|---|
| SI-01 | Second launch while the window shows | Open the copy, then `open -n` it | One process; the window comes to the front | P0 |
| SI-02 | … while the window is closed (⌘W, hidden) | Close the window, open the app again (`open -n`, Finder) | The window shows again | P0 |
| SI-03 | … while minimised | ⌘M, open again | The window is unminimised and in front | P1 |
| SI-04 | … while hidden (⌘H) | ⌘H, open again | The app unhides, window in front | P1 |
| SI-05 | … from a second copy of the app (disk image, Downloads) | Copy the `.app` elsewhere, open the copy | The first copy's window shows; the second process quits | P0 |
| SI-06 | … while the app is quitting | ⌘Q with an agent run going (stopping takes up to 8 s), open again at once | The new launch waits for the old copy and opens a window; never nothing at all, never a lock dialog | P1 |
| SI-07 | Two launches at the same instant, nothing running | `open -n` twice within 50 ms | One window, one daemon; no lock dialog | P2 |
| SI-08 | A copy with its own `OD_DATA_DIR` runs beside the app | App open; start a second copy with `OD_DATA_DIR=<tmp>` | Both run | P1 |
| LK-01 | `od daemon start` holds the data folder (English) | `od daemon start` on the copy's folder, then open the app | Dialog "Contour can't open yet", says to quit the other Contour or stop the command; Quit quits; the log has the URL and folder | P0 |
| LK-02 | Same, Vietnamese | `locale` `vi` in `app-config.json` | "Chưa mở được Contour", the Vietnamese words, button "Thoát" | P1 |
| LK-03 | After stopping the command, the app opens | LK-01, Ctrl+C the daemon, open again | The app opens on the same data | P0 |
| LK-04 | Lock held by the app's own copy that does not let go | A second copy on the same folder blocked past 12 s | "Contour is already open" after ≈ 12 s | P2 |
| LK-05 | An older Contour (beta.35) holds the folder | beta.35 open on the folder, open beta.36 | The dialog names an older Contour too | P1 |
| UP-01 | Update now, then Restart | beta.35 on its own folder, Settings ▸ Update now ▸ Restart | beta.36 opens on the same data, no lock dialog, the window shows once | P0 |
| UP-02 | Restart with a comment not sent | As UP-01 with a comment draft | Restart asks first (beta.35 changelog); Cancel stays | P1 |

## 23. Drafts after beta.35 (`drafts.qa.mjs`, DL-28 on)

Goal: a comment being written (note, images, element) stays with its file
and is never lost without a question; an image uploaded for a draft that
is thrown away is not left behind in `comments/`. Expected results from
the beta.35 changelog ("Cancel on a comment deletes the images uploaded
for it") and spec.md (nothing the user made is lost unseen).

| ID | Scenario | Steps | Expected | P | Status |
|---|---|---|---|---|---|
| DL-28 | A comment with an image, then Leave | Draft + image, Projects ▸ Leave | Projects opens; the image is not left in `comments/` | P1 | auto (QA-096, fixed) |
| DL-29 | Same, image still uploading | Image upload held, Projects ▸ Leave, upload lands | No file left in `comments/` | P1 | auto (QA-096, fixed) |
| DL-30 | A comment draft, then a page reload | Draft with a note, reload | The browser asks before leaving | P1 | auto |
| DL-31 | Drafts on two files | Draft on a.html and b.html, Projects | The question names both files | P1 | auto |
| DL-32 | A file with a comment draft renamed | Draft on a.html, rename it to c.html in the Files panel | The note is kept on c.html, or the rename asks first | P2 | auto (QA-097, fixed) |
| DL-33 | A file with a comment draft deleted | Draft with an image on a.html, delete a.html | The delete asks; afterwards no draft image left in `comments/` | P2 | auto |
| DL-34 | Deck: the slide a comment is on goes | Draft on slide 2, the deck rewritten with 1 slide | The note can still be sent or discarded | P1 | auto (QA-098, fixed) |
| DL-35 | A file with a Manual edit change not applied renamed | Typed in Content, rename page.html | The change is kept, or the rename asks; leaving still asks | P2 | auto (QA-097, fixed) |
| DL-36 | Back, then Leave | Draft + image, browser Back ▸ Leave | No image left in `comments/` | P1 | auto |
| DL-37 | Reload chosen, or the page crashes | Draft + image, reload ▸ Leave; then a crash (no request) | Reload: image deleted. Crash: image not in the ZIP | P1 | auto |
| DL-38 | Add, then leave at once | Comment POST held, Projects, POST lands | The comment has its image; the release after it keeps it | P1 | auto |
| DL-39 | A file with a draft moved into a folder | Files ▸ Move to | Note and image on `refs/a.html` | P2 | auto |
| DL-40 | The folder holding it moved | Folder ▸ Move to | Note on the new path | P2 | auto |
| DL-41 | The agent renames it | `files/rename` from outside the app | Draft and image go; the Studio names the file | P2 | auto |
| DL-42 | Manual image uploaded, another file shown, Leave | Upload held, switch file, Leave | The image not applied is not left in `assets/` | P2 | auto |
| DL-43 | A comment sent with an image, then Leave | Add, Projects | The image stays and ships | P1 | auto |
| DL-44 | A rename that fails | Rename onto a name taken | Draft stays on the old name; a later delete outside the app takes it | P2 | auto |
| DL-45 | Watcher before the rename's answer, image landing meanwhile | Rename answer and upload held | Words typed meanwhile, the image and the tab follow | P1 | auto |
| DL-46 | Deck reloads while typing | Agent rewrites the deck mid-note | No key lost | P1 | auto |
| DL-47 | Manual image written into the page | Upload on the image element, Leave | The image stays and ships | P1 | auto |
| DL-48 | A sent comment's file renamed | Add, rename | The pin stays | P2 | auto |
| DL-49 | A text edit not written, file renamed | Source edit fails, rename | Try again writes the renamed file | P2 | auto |
| DL-50 | Manual change kept on a file not shown, renamed | Type, switch, rename | The change comes back on the new name | P2 | auto |
| DL-51 | Manual image applied, Studio left before the write is saved | `source-edits` held, Projects, then let go | The page's image stays on disk and ships | P1 | auto |

## Not automated, and why

- **Desktop shell (Tauri)**: native menus, Cmd+O folder dialog, Finder
  launch PATH and single-instance with `od`. Playwright drives the browser
  UI only; the desktop app needs a manual pass on a built `Contour.app`.
- **Real agents and real BYOK providers**: the suite uses the mock CLI and
  a fake provider on loopback (no outbound network by design). Round 4
  ran five turns of the real Claude Code by hand (bugs.md, round 4);
  QA-026 and QA-027 were found that way and then reproduced with the mock.
- **MP4 export**: covered for the Motion templates by TPL-17 and TPL-18
  (need Chrome and ffmpeg); other compositions are `motion.e2e.mjs`.
- **ONB-09b in the desktop app**: Check again asks the login shell for its
  `PATH` again only in the app (a CLI daemon has none); the suite covers it
  with a fake shell in `od-server` tests, the real shell needs a manual pass.
- **No Chrome at all**: Chrome is installed on this machine at its standard
  path, so EXP-03 points `OD_CHROME_PATH` at nothing instead; the message
  for a machine without any Chrome ("this export needs Chrome, Edge or
  Chromium") is not exercised. EXP-04 and EXP-05 skip without Chrome.
- **Desktop notifications**: the browser in headless mode never shows OS
  notifications; only the absence of errors is checked.
