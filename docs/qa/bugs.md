# QA bugs

Bugs found by the QA suite (`crates/od-ui/e2e/qa`, plan in
[test-plan.md](test-plan.md)). Each has a test that stays red, marked
`knownBug('QA-NNN')`, until it is fixed; `make e2e-qa` then reports it as
"known bug now passing" so the entry can be closed here.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

Retested on `main` 17f9c2a (PR #6 merged): none of QA-001…005 is fixed by
it; PR #6 touched none of their code.

Round 2 (branch `quangphung218/qa-e2e` at 9829e47, after the fixes of
QA-001…010): QA-011…017 come from the two reviews of those fixes
(correctness and UX), each reproduced by a red test. QA-002's guard for a
conversation switched while loading now has a test (CHAT-10, passes; it
goes red when the guard's `return` is removed).

Round 2 fixes (5728ac3…bae2d3d): QA-011…017 fixed; `make e2e-qa` twice
at bae2d3d, 101 of 102 pass. STU-14 (QA-015) stays marked: the fix works,
the test reads the frame mid-transition (see QA-015).

Round 3 (branch at c357aa5, after the round 2 fixes): STU-14 was a test
bug (it read the frame mid-transition); it now waits for two equal reads
and passes, so QA-015 is closed. QA-018…025 come from the two reviews of
the round 2 fixes and the debugger's open points, each with a red test.
Expected results are Atlat's calls for this round. DL-21 is a new guard
(no bug): a sent and answered message must not come back to the composer;
it goes red when the `clear_draft_if` of `send` is removed.

Round 3 fixes (5203152…0a68f88): QA-018…025 fixed, one commit each;
`make e2e-qa` twice at 0a68f88, 112 of 112 pass, no test still marked.

| ID | Sev | Summary | Test | Status |
|---|---|---|---|---|
| QA-018 | 🔴 | With a BYOK provider saved without a key on a host the daemon calls public (`http://ollama:11434`, `gpu-box`), every Settings save (theme, language…) is refused with 400 | `settings.qa.mjs` SET-07, SET-08 | Fixed |
| QA-019 | 🟠 | A message steered into the running turn comes back in the composer after a reload | `drafts.qa.mjs` DL-20 | Fixed |
| QA-022 | 🟠 | The browser's Back with an edit not applied leaves at once; the typed text is lost | `drafts.qa.mjs` DL-24 | Fixed |
| QA-020 | 🟡 | Duplicate project with an edit not applied skips the app's leave question (only the browser's "leave site?" asks) | `drafts.qa.mjs` DL-22 | Fixed |
| QA-024 | 🟡 | Tablet on a phone-width window is stuck at the fit (35%): + is off, the page cannot be zoomed in to read | `studio.qa.mjs` STU-15 | Fixed |
| QA-021 | 🔵 | Ctrl/Cmd-click on an in-app link with an edit not applied is stopped and asked as if this tab were leaving | `drafts.qa.mjs` DL-23 | Fixed |
| QA-023 | 🔵 | The leave question opens with the focus on the dialog box, not on Cancel | `drafts.qa.mjs` DL-25 | Fixed |
| QA-025 | 🔵 | When + is off at the fit, its tooltip still says "Zoom in", not why | `studio.qa.mjs` STU-16 | Fixed |
| QA-011 | 🟠 | Desktop app: choosing Leave on the "edit not applied" question does not leave; every in-app link is dead while an edit is unapplied | `drafts.qa.mjs` DL-13 | Fixed |
| QA-014 | 🟠 | A message queued while a run is going is lost on a reload (queue only in memory, draft cleared at queue time) | `drafts.qa.mjs` DL-19 | Fixed |
| QA-013 | 🟠 | Esc that closes a drop-down menu also stops a running Design Jury and turns off comment mode or the tool | `export.qa.mjs` EXP-10, EXP-11 | Fixed |
| QA-012 | 🟡 | Leaving through the command palette, the quick menu or the chat's Settings button does not ask about an edit not applied | `drafts.qa.mjs` DL-15…DL-18 | Fixed |
| QA-015 | 🟡 | Zoom label says 100% while Tablet is drawn shrunk to fit; + changes the label, not the page | `studio.qa.mjs` STU-14 | Fixed (test fixed in round 3) |
| QA-016 | 🔵 | BYOK on a phone: "API key required" sits in the button row, not with the key field, and pushes Save onto its own line | `onboarding.qa.mjs` ONB-09 | Fixed |
| QA-017 | 🔵 | Home: while design systems load, the kept pick shows its raw id | `home.qa.mjs` HOME-12 | Fixed |
| QA-006 | 🟠 | Edit panel: an Apply in one tab wipes text typed in another tab, and text typed while the Apply is saving | `drafts.qa.mjs` DL-01, DL-02, DL-03 | Fixed |
| QA-008 | 🟠 | Drafts are not kept: composer text and the Home brief and design system vanish on reload or navigation | `drafts.qa.mjs` DL-05, DL-06, DL-07 | Fixed |
| QA-007 | 🟡 | Closing or reloading the page with a manual edit not applied gives no warning | `drafts.qa.mjs` DL-04 | Fixed |
| QA-002 | 🟠 | A message sent right after opening a project can vanish from the chat while the run goes on | `chat.qa.mjs` CHAT-09 | Fixed |
| QA-003 | 🟠 | Tablet preview lays the page out at the pane width, not 820 px | `studio.qa.mjs` STU-11 | Fixed |
| QA-005 | 🟠 | Onboarding saves and selects an API provider with an empty key | `onboarding.qa.mjs` ONB-05 | Fixed |
| QA-004 | 🟡 | Esc does not close the Export menu (nor the other drop-down menus) | `export.qa.mjs` EXP-09 | Fixed |
| QA-001 | 🟡 | Renaming a project to blank shows a nameless card and no error | `projects.qa.mjs` PRJ-02 | Fixed |
| QA-009 | 🔵 | Leaving a Markdown file with an unsaved edit panics in wasm (the edit is saved) | `drafts.qa.mjs` DL-08 | Fixed |
| QA-010 | 🔵 | Cancel in the delete-conversation dialog throws a JS error | `drafts.qa.mjs` DL-10 | Fixed |

---

## QA-001 🟡 Renaming a project to blank shows a nameless card and no error

- **Steps**: Projects ▸ a card ▸ Rename ▸ clear the name, type spaces ▸ Enter.
- **Expected**: the old name stays on the card and the user is told a name
  is required (the daemon already answers `400 name required`).
- **Actual**: the card shows an empty title (no link to open the project by
  name) and no message. A full page reload brings the old name back.
- **Evidence**: `PATCH /api/projects/:id {"name":"   "}` → `400 {"error":
  {"code":"BAD_REQUEST","message":"name required"}}`; screenshot from the
  run in `crates/od-ui/e2e/test-results/qa/shots/PRJ-02.png`.
- **Test**: `projects.qa.mjs` PRJ-02.
- **Code**: `crates/od-ui/src/app/projects.rs` `ProjectCard::rename`
  ignores the PATCH result (`let _ = api::patch…`) and keeps the typed value
  in the card's `name` signal. Trim and refuse empty input before sending,
  and on error restore the old name and show the error. Duplicate and delete
  on the same card ignore errors the same way.

## QA-002 🟠 A message sent right after opening a project can vanish from the chat

- **Steps**: open a project (new or existing) and send a message as soon as
  the composer shows (within about 100 ms of the page load; a fast typist,
  a paste, or a prompt chip).
- **Expected**: the user message and the streaming answer show in the chat.
- **Actual**: intermittent, about one project in three on a loaded machine
  (every run of the suite so far hit it within 6 tries). The run starts (`POST /api/runs`
  → 202), the agent writes `index.html` and the preview updates, but the
  chat stays on "Start the conversation" with no user or assistant message
  and no Stop button. A reload shows the turn: it was stored.
- **Evidence**: network log of a failing try (scratch probe): messages
  `GET …/conversations/:cid/messages` answered at 346 ms, `POST /api/runs`
  at 358 ms, run events stream attached at 367 ms, nothing re-renders the
  transcript. Screenshot `test-results/qa/shots/CHAT-09.png`.
- **Test**: `chat.qa.mjs` CHAT-09 (6 tries, fails on the first empty chat).
  The other chat tests wait for the conversation to load before sending.
- **Code** (likely cause, not proven): `crates/od-ui/src/app/studio.rs` `open_conversation` clears
  `items`, awaits comments then messages, and then **replaces** `items`
  with the stored list. A send that lands before that `items.set` (the
  JSON decode is a separate await) is overwritten, and the live turn's
  signal is no longer in the list. Merge instead of replace, or disable
  send until the conversation has loaded.

## QA-003 🟠 Tablet preview lays the page out at the pane width, not 820 px

- **Steps**: window 1360×860 (a 13–14" laptop), open a project with a page,
  preview toolbar ▸ Tablet.
- **Expected**: the page is laid out at 820 px wide, as the original app
  does (preset 820×1180), scaled or scrolled to fit the pane, so tablet
  media queries apply.
- **Actual**: the frame box says `width:820px`, but the iframe is clamped
  to the pane (650 px), so the page renders at 650 px: Tablet looks like a
  narrow desktop and tablet breakpoints are never hit. Phone (390 px) is
  right; at 1920 px wide Tablet is right too.
- **Evidence**: inside the frame `window.innerWidth` = 650 after Tablet at
  1360×860; 820 at 1920×1000.
- **Test**: `studio.qa.mjs` STU-11.
- **Code**: `crates/od-ui/src/app/studio.rs` `zoomed()`: at zoom 100 it
  returns `width:<device>` without `max-width:none` (the other branch has
  it), so `.frame-wrap { max-width: 100% }` (`crates/od-ui/styles/app.css:860`) wins. Add
  `max-width:none`, and scale the frame down when it is wider than the pane.

## QA-004 🟡 Esc does not close the Export menu (nor the other drop-down menus)

- **Steps**: Studio ▸ Export ▸ press Esc.
- **Expected**: the menu closes, as in the original app (`FileViewer.tsx`
  closes its share/export, present and zoom menus on Escape) and as
  features.md says for Esc ("close a modal, leave present or a mode").
- **Actual**: the menu stays open; only a click on the page closes it. A
  keyboard-only user has no way to dismiss it.
- **Test**: `export.qa.mjs` EXP-09.
- **Code**: `crates/od-ui/src/app/export_menu.rs` (no key handler; the
  `.menu-scrim` only handles clicks). Same pattern in `capture.rs`,
  `deck.rs` (present menu) and `files_panel.rs` (New menu, row menu).

## QA-005 🟠 Onboarding saves and selects an API provider with an empty key

- **Steps**: first launch with no agent CLI ▸ API key ▸ provider Anthropic
  (base URL and model are prefilled) ▸ leave API key empty ▸ Save and use.
- **Expected**: Save is refused ("API key required") and Continue stays
  off, as in the original app (`EntryShell.tsx` and `SettingsDialog.tsx`
  require `apiKey.trim()` for providers that need a key).
- **Actual**: "✓ Saved" shows, `agentId` becomes `byok` with no key, and
  Continue is enabled. The user reaches Home and every brief fails later.
- **Evidence**: `GET /api/app-config` after the save: `agentId: "byok"`,
  `byok.hasApiKey` false. Screenshot `test-results/qa/shots/ONB-05.png`.
- **Test**: `onboarding.qa.mjs` ONB-05.
- **Code**: `crates/od-ui/src/app/byok_form.rs` `save` only blocks Azure
  without a base URL; `needs_key()` exists but is not used to disable Save.
  The daemon accepts the `PUT /api/app-config` too.

## QA-006 🟠 Edit panel: an Apply in one tab wipes text typed in another tab

Reported by the PR #6 reviewer (`manual_model.rs` `written`), reproduced
end to end here.

- **Steps A**: open a page ▸ Edit (manual edit) ▸ click a paragraph ▸
  Content: type new text, do not apply ▸ Attributes: add a title ▸ Apply ▸
  back to Content. Same with Style ▸ font size ▸ Apply instead of Attributes.
- **Steps B**: Attributes: change ▸ Apply, and while the save is in flight
  go to Content and type.
- **Expected**: what the user typed and did not apply stays in its field
  (the doc of `refreshed` promises "a field the user changed since it was
  last clean keeps what they typed").
- **Actual**: the Content field is back to the source text
  ("Fresh leaves daily."), the typed text is gone, no message.
- **Test**: `drafts.qa.mjs` DL-01 (Attributes), DL-02 (Style), DL-03 (typing
  while the save is held by the test until the user has typed).
- **Code**: `crates/od-ui/src/manual_model.rs:270` `written` marks every
  draft clean with the values present when the response arrives; called
  from `crates/od-ui/src/app/manual_edit.rs:147`, `:284`, `:423`. Take the
  drafts **before** `spawn_local`, and mark clean only the fields of that
  write (Content: text/href/src/alt; Attributes: attrs; Html: html; Style:
  none).

## QA-007 🟡 Closing or reloading with a manual edit not applied gives no warning

- **Steps**: manual edit ▸ type new text in Content (no Apply) ▸ close the
  tab, reload, or quit.
- **Expected**: the browser asks before leaving, as the original app does
  (`FileWorkspace.tsx` adds a `beforeunload` guard and an in-app navigation
  guard while a manual edit is unsettled).
- **Actual**: the page closes at once; the typed text is lost.
- **Test**: `drafts.qa.mjs` DL-04.
- **Code**: no `beforeunload` listener anywhere in `crates/od-ui/src`. Add
  one while the Edit panel has dirty drafts (`DraftBook`), and in the
  desktop app on window close.

## QA-008 🟠 Drafts are not kept: composer text and the Home brief vanish

- **Steps**: (a) Studio: type a message, do not send ▸ reload, or go to
  Projects and back. (b) Home: type a brief and pick a design system ▸
  Settings ▸ Home.
- **Expected**: the text comes back, as in the original app: the composer
  draft is stored per project and conversation
  (`od:chat-composer:draft:<project>:<conversation>`, `ChatPane.tsx:2654`),
  and Home keeps the prompt and design system
  (`open-design:home-composer:prompt` / `:design-system`, `HomeView.tsx:351`).
- **Actual**: the composer is empty after a reload or a round trip; Home's
  brief is empty and the design system is back to none.
- **Test**: `drafts.qa.mjs` DL-05, DL-06 (composer), DL-07 (Home).
- **Code**: `crates/od-ui/src/app/composer.rs` keeps the draft only in a
  signal (`localStorage` holds just the chat width); `home.rs` the same for
  `brief` and `design_system`.

## QA-009 🔵 Leaving a Markdown file with an unsaved edit panics in wasm

- **Steps**: open a `.md` file ▸ type ▸ at once click Projects (or press
  Cmd+,).
- **Expected**: the edit is saved, no error.
- **Actual**: the edit is saved (by the blur save), but the console shows
  `panicked at reactive_graph … doc_view.rs:133:50, you tried to access a
  reactive value which was defined at doc_view.rs:105:16, but it has already
  been disposed`. The app keeps working in the debug build.
- **Test**: `drafts.qa.mjs` DL-08 (the content check passes; the panic fails
  it).
- **Code**: `crates/od-ui/src/app/doc_view.rs` `on_cleanup` (line 252) calls
  `save_now`, whose `spawn_local` reads `path.get_value()` after the
  component is disposed. Read `path` (and `ctx` values) before spawning, or
  use `try_get_value`.

## QA-010 🔵 Cancel in the delete-conversation dialog throws a JS error

- **Steps**: Studio ▸ conversation menu ▸ Delete ▸ Cancel.
- **Expected**: the dialog closes, nothing else.
- **Actual**: the dialog closes and the conversation is kept, but the page
  throws `closure invoked recursively or after being dropped` twice.
- **Test**: `drafts.qa.mjs` DL-10.
- **Code**: `crates/od-ui/src/app/chat_ops.rs:153-170`: Cancel sets
  `confirming` to false inside its click handler, `<Show>` drops the dialog,
  and the same click then bubbles to the dropped `.dialog`/`.overlay`
  handlers. Close on the next tick (the file already has `next_tick`) or stop
  propagation on the buttons.

## QA-011 🟠 Desktop app: choosing Leave on the "edit not applied" question does not leave

Reported by the UX review of the QA-007 fix; reproduced in Chromium with
the dialog plugin's override.

- **Steps**: Contour.app ▸ a page ▸ Edit ▸ click a paragraph ▸ type in
  Content, do not apply ▸ click Projects in the sidebar ▸ the native dialog
  "An edit is not applied yet. Leave and lose it?" ▸ choose Leave (OK).
- **Expected**: the app goes to Projects (the user agreed to lose the edit).
- **Actual**: the page stays where it is. Every in-app link stays dead until
  the edit is applied or the text deleted.
- **Cause**: `tauri-plugin-dialog` (`apps/desktop/src/main.rs:278`) replaces
  `window.confirm` with an `async` function, so it returns a Promise.
  `guard_unapplied` (`crates/od-ui/src/app/manual_edit.rs:503`) calls
  `confirm_with_message`, which reads the Promise as `false`; the click is
  then always `prevent_default`ed, whatever the user picks.
- **Test**: `drafts.qa.mjs` DL-13 (init script: `window.confirm = async () =>
  true`, as the plugin; the test accepts any way of asking: native, desktop
  or an in-page dialog). DL-14 checks the browser path (Cancel stays with the
  text, OK leaves) and passes.
- **Fix hint**: always stop the click, ask with the app's own dialog
  (`Dialog`/`ConfirmBar`; `lib_ui.rs:82` already avoids `window.confirm`),
  and on Leave call `use_navigate()` to the link's `href`. The same helper
  serves QA-012.
- **Fixed** (5728ac3, bae2d3d): the guard moved to the shell (`leave_guard.rs`). A leaving link click with unsaved work is always stopped and the app's own dialog asks; Leave navigates to the link's path. No `window.confirm` any more.

## QA-012 🟡 Leaving through the palette, quick menu or chat buttons does not ask

Reported by the correctness review of the QA-007 fix. Same flaw as QA-007:
the guard only sees clicks on `a[href]`.

- **Steps**: a page ▸ Edit ▸ type in Content, do not apply ▸ any of:
  Cmd/Ctrl+K ▸ "Projects" (or "Settings"); quick menu ▸ a Settings section;
  after a failed turn, the chat's "Settings" button.
- **Expected**: the same question as a sidebar link: Cancel stays with the
  text, OK leaves (as the original's in-app navigation guard).
- **Actual**: the Studio unmounts at once with no question; the typed text
  is gone.
- **Test**: `drafts.qa.mjs` DL-15 (palette ▸ Projects), DL-16 (palette ▸
  Settings), DL-17 (quick menu ▸ Models), DL-18 (chat ▸ Settings).
- **Code**: direct `navigate()` calls that skip the guard:
  `crates/od-ui/src/app/palette.rs:236` (`Action::Go`), `quick_menu.rs:135`,
  `chat_view.rs:83` (MCP) and `:707` (Settings). Not tested: the chat's MCP
  path (`/mcp` in the composer). Fix with one shared "navigate unless an
  edit is unapplied" helper that asks first (see QA-011).
- **Fixed** (9a85549): the palette, quick menu, chat (Settings, MCP) and composer (MCP) go through `use_leave()`, which asks the same question first. Not covered: the palette's switch to another project reloads the page (`location.assign`), so only `beforeunload` asks.

## QA-013 🟠 Esc that closes a drop-down menu also stops a Design Jury or comment mode

Reported by the UX review of the QA-004 fix.

- **Steps A**: Studio ▸ comment mode on ▸ Export ▸ Esc.
- **Steps B**: Design Jury on ▸ send a brief ▸ while the jury runs, Export ▸
  Esc.
- **Expected**: Esc closes the menu only (one Esc, one thing closed, as in
  the original's menus).
- **Actual**: A: the menu closes and comment mode turns off. B: the menu
  closes and the jury is interrupted (`data-status="interrupted"`), its
  rounds stop. The user has to start the critique again.
- **Test**: `export.qa.mjs` EXP-10 (comment mode), EXP-11 (jury; the QA
  mock `claude` has a `qa-jury-slow` request that thinks until interrupted).
- **Code**: `crates/od-ui/src/app/menu_scrim.rs:10` handles Esc on `window`
  without stopping it; `agent_w3.rs:466` interrupts the jury on any Esc
  unless a `[role=dialog]` is open, and `studio.rs:932` turns off comment
  mode or the tool. Stop the event in `MenuScrim` (capture phase,
  `stop_immediate_propagation`), or have those listeners skip it while a
  `.menu-scrim` is shown.
- **Fixed** (dda7917): `MenuScrim` listens in the capture phase on the window and calls `stop_immediate_propagation` on the Esc it uses.

## QA-014 🟠 A message queued while a run is going is lost on a reload

Reported by the correctness review of the QA-008 fix.

- **Steps**: Studio ▸ send a long request ▸ while it runs, type a second
  message ▸ Enter (it shows under "Queued") ▸ reload the page.
- **Expected**: the second message is not lost before it is really sent:
  still queued, back in the composer, or sent when the run ends (the QA-008
  rule: the draft goes once the send succeeds).
- **Actual**: after the reload the run comes back, but the queued message
  is nowhere: not queued, not sent, the composer empty, no stored draft.
- **Test**: `drafts.qa.mjs` DL-19 (it waits 20 s for the run to end and a
  queued send to happen). Red in 7 of 8 tries (both full runs included); one early try passed in the
  first full-file run, cause not found (see the round 2 report).
- **Code**: `crates/od-ui/src/app/studio.rs:364-367` `send` calls
  `clear_draft_if` when it queues; the queue (`ctx.queue`) lives only in
  memory. Clear the draft only when `send_next` really sends (the `Ok`
  branch at `studio.rs:442` does it already), or store the queue.
- **Fixed** (14d3ea6): `send` no longer clears the stored draft when it queues; the draft goes when the run is created. After a reload the queued text is back in the composer. Left: deleting a queued message keeps its stored draft, so a reload brings it back to the composer.

## QA-015 🟡 Zoom label says 100% while Tablet is drawn shrunk to fit

Reported by both reviews of the QA-003 fix. Not a regression: the original
shows the chosen zoom too (`FileViewer.tsx:2501`), but QA-003's fit made
the gap visible.

- **Steps**: window 1360×860 ▸ a page ▸ Tablet ▸ read the zoom label ▸
  press +.
- **Expected**: the label shows the size the page is drawn at; + either
  makes the page bigger or is off when the fit already caps it.
- **Actual**: the label says 100% while the 820 px page is drawn at 63%
  of its size; + changes the label to 125% and the page does not change.
- **Test**: `studio.qa.mjs` STU-14 (drawn width of the frame ÷ 820 vs
  the label).
- **Code**: `crates/od-ui/src/app/studio.rs:1481` prints `ctx.zoom`, not the
  fitted `zoom` memo (`fit_zoom`, `:1271`). Show the fitted value (or mark
  it "fit"), and disable + (`at_max_zoom`) when the fit caps it.
- **Fixed** (759581a): the label shows the fitted zoom, and the zoom buttons and keys step from it, off when a step would not change the drawn size. **STU-14 stays `knownBug`, test timing**: it reads the drawn width as soon as it is under 100 %, while `.frame-wrap` is still in its 0.3 s width transition (650 → 820 px under the new scale). A probe every 100 ms after Tablet read 63, 76, 79, 79, 79… with the label at 79, and + off. The test should wait for the width to settle (e.g. two equal reads) before comparing; QA to change it.
- **Test fixed** (round 3): STU-14 reads the drawn width only once two reads 100 ms apart are equal (`settledDrawn`), after Tablet and after +. It passes; `knownBug` removed.

## QA-016 🔵 BYOK on a phone: "API key required" sits in the button row

Reported by the UX review of the QA-005 fix.

- **Steps**: window 390×844 ▸ onboarding ▸ API key ▸ Anthropic ▸ leave the
  key empty.
- **Expected**: the hint shows with the API key field (below it, linked by
  `aria-describedby`), and Save ends the button row.
- **Actual**: the hint is a span in the actions row; at 390 px it takes the
  row and pushes "Save and use" to a line of its own, left-aligned. A
  screen reader does not tie it to the field.
- **Test**: `onboarding.qa.mjs` ONB-09.
- **Code**: `crates/od-ui/src/app/byok_form.rs:332`. Move it into the
  API key `.field` (`:273`) with an id, and point `aria-describedby` of
  `byok-key` at it.
- **Fixed** (5b38cb5): the line is in the API key field, with an id the input names in `aria-describedby`. Also (56ffb95, QA-005 follow-up): the daemon answers 400 to a BYOK provider on a public host saved with no key, sent or stored; loopback, LAN and `.local` hosts still save without one.

## QA-017 🔵 Home: while design systems load, the kept pick shows its raw id

Reported by the UX review of the QA-008 fix.

- **Steps**: Home ▸ pick a design system ▸ reload on a slow connection.
- **Expected**: until the list arrives, the picker shows the system's name
  or a "Loading…" word (and may be disabled).
- **Actual**: it shows the id (e.g. `agentic`) until the list arrives.
- **Test**: `home.qa.mjs` HOME-12 (holds `/api/design-systems`).
- **Code**: `crates/od-ui/src/app/home.rs:407` renders `id` as the option
  text. Show a loading label (or keep the name with the id in
  `localStorage`).
- **Fixed** (724cdab): while the list loads the kept pick reads "Loading…" and the picker is disabled.


## QA-018 🔴 With a keyless BYOK provider on a "public" host, every Settings save is refused

Reported by the correctness review of 56ffb95 (the QA-005 daemon rule).

- **Steps A** (config from before the rule, or Ollama in docker): a BYOK
  provider stored with `baseUrl: http://ollama:11434/v1` and no key ▸
  Settings ▸ Appearance ▸ Dark ▸ Save (or Language ▸ Tiếng Việt ▸ Save).
- **Steps B** (new setup): Settings ▸ API ▸ preset Ollama ▸ base URL
  `http://gpu-box:11434/v1` ▸ Save (the form allows it: Ollama needs no key).
- **Expected** (Atlat's call): the save goes through (200), no error.
- **Actual**: `PUT /api/app-config` → 400 `byok.apiKey required: ollama
  needs an API key`; the save bar shows the error. B: the form says Save is
  fine, the daemon refuses it.
- **Test**: `settings.qa.mjs` SET-07 (config file written with the keyless
  Ollama, daemon restarted, then theme and language saves), SET-08 (B).
- **Code**: `crates/od-server/src/routes/ui.rs` `put_config` (`:310-326`):
  `saves_byok` is true for any patch carrying `byok`, and
  `AppConfig::to_patch` (`crates/od-ui/src/settings.rs:87`) sends the whole
  config (`extra`) on every save. `self_hosted` (`:214`) calls one-word hosts
  (`ollama`, `gpu-box`), Tailscale `100.64/10`, `*.ts.net` and `fc00::/7`
  public. The original has no daemon rule; its UI checks per preset
  (`apps/web/src/components/byok/validation.ts:127`). Either drop the host
  rule, or check only a `byok` whose route (protocol, baseUrl) changes and
  count those hosts as self-hosted.
- **Fixed** (5203152): the daemon no longer guesses public hosts. It refuses a `byok` without a key only when the patch moves it to another protocol or base URL that is a cloud preset needing a key (the preset table now lives in `od-contracts`), and only while BYOK is the agent picked. A whole-config save that keeps the route (theme, language) goes through; a stale keyless provider sent with a local agent too (`make e2e` manage hit this: the UI kept a provider the test had removed).

## QA-019 🟠 A steered message comes back in the composer after a reload

Reported by the correctness review of 14d3ea6 (QA-014 fix).

- **Steps**: Studio ▸ send a long request ▸ while it runs type "make it
  blue" ▸ Enter (queued) ▸ Steer ▸ the turn shows "Steered: make it blue" ▸
  reload (or Projects and back).
- **Expected** (Atlat's call): the composer is empty; the message was
  delivered.
- **Actual**: the composer holds "make it blue" again; Enter sends it a
  second time.
- **Test**: `drafts.qa.mjs` DL-20. Guard of the main path: DL-21 (send,
  wait for the answer, reload: composer empty; passes).
- **Code**: `crates/od-ui/src/app/studio.rs` `steer` (`:503`): its `Ok`
  branch drops the item from the queue but keeps the stored draft; only
  `send`'s `Ok` (`:461`) calls `clear_draft_if`. Call
  `clear_draft_if(draft_key(…), &item.text)` there too, and when a queued
  item is deleted (`chat_view.rs:496`).
- **Fixed** (4f686d3): Steer's `Ok` and the queue's delete go through `drop_queued`, which clears the stored draft as a send does.

## QA-020 🟡 Duplicate project with an edit not applied skips the app's question

Reported by the correctness review of 9a85549 (QA-012 fix).

- **Steps**: a page ▸ Edit ▸ type in Content, do not apply ▸ files panel
  "…" ▸ Duplicate.
- **Expected** (Atlat's call): the app's "Leave this page?" dialog, as for
  any other way out; Cancel stays with the text, Leave opens the copy.
- **Actual**: no app dialog. The copy is made and the page does a full
  load; in a browser only the native "leave site?" (`beforeunload`) asks,
  and in the desktop app `beforeunload` is not checked.
- **Test**: `drafts.qa.mjs` DL-22 (the test dismisses the native prompt
  and counts it; it saw 1).
- **Code**: `crates/od-ui/src/app/project_actions.rs:113` `duplicate`
  calls `location().set_href`. Go through `use_leave()` (or ask before the
  POST). Same pattern: the palette's switch to another project
  (`palette.rs:246`).
- **Fixed** (3645cd3): Duplicate (before the POST) and the palette's switch to another project go through `leave_then`: the app's question first, the full load once Leave is chosen, without a second native prompt.

## QA-021 🔵 Ctrl/Cmd-click on a link with an edit not applied is stopped

Reported by the correctness review of 5728ac3 (QA-011 fix).

- **Steps**: a page ▸ Edit ▸ type in Content ▸ Ctrl-click (Cmd-click on a
  Mac) Projects in the sidebar.
- **Expected** (Atlat's call): not stopped in this tab: a new tab opens on
  Projects, this one stays with the text and no question.
- **Actual**: the leave dialog shows; picking Leave navigates this tab.
- **Test**: `drafts.qa.mjs` DL-23.
- **Code**: `crates/od-ui/src/app/leave_guard.rs:65` `on_click` only
  skips `button() != 0`. Also return early on `ctrl_key()`, `meta_key()`,
  `shift_key()` (and `alt_key()`).
- **Fixed** (4c69489): a click with Ctrl, Cmd, Shift or Alt is passed through.

## QA-022 🟠 The browser's Back with an edit not applied leaves without asking

Reported by the UX review and the debugger (open point 5). Not a
regression: main does the same.

- **Steps**: Projects ▸ open a project ▸ Edit ▸ type in Content ▸ the
  browser's Back (or the mouse back button, or ⌘[).
- **Expected** (Atlat's call): the same question; Cancel stays on the
  page with the text, Leave goes back.
- **Actual**: Projects shows at once; the typed text is gone.
- **Test**: `drafts.qa.mjs` DL-24 (opens `/projects` first, then
  `history.back()`).
- **Code**: `leave_guard.rs` has no `popstate` handling. On `popstate`
  with unsaved work: push the current URL back, then open the dialog;
  Leave does `history.back()` (or navigates to the target) with the guard
  off.
- **Fixed** (2bc7e54): while an edit is not applied the page holds a second history entry of its own URL, so Back stays in this document even when the page was opened by URL; the guard stops the router, pushes the entry back and asks; Leave goes back to the target. One Back more is needed after an edit is applied (the extra entry stays).

## QA-023 🔵 The leave question opens with the focus on the dialog box

Reported by the UX review of 5728ac3.

- **Steps**: a page ▸ Edit ▸ type in Content ▸ click Projects.
- **Expected** (Atlat's call): the focus is on Cancel, so Enter or Space
  keeps the work (the safe answer is the default).
- **Actual**: the focus is on the dialog itself (`data-testid=
  "leave-dialog"`); the first Tab goes to the unlabeled × button.
- **Test**: `drafts.qa.mjs` DL-25.
- **Code**: `leave_guard.rs` `LeaveGuard` view; focus `leave-stay` on
  open (`Modal` in `lib_ui.rs` focuses its box; add an autofocus prop or
  focus the button on mount).
- **Fixed** (6d7c49e): `Modal` takes an `autofocus` selector; the leave question focuses Cancel.

## QA-024 🟡 Tablet on a phone-width window is stuck at the fit

Reported by the UX review of 759581a (QA-015 fix). Main let − go to 75%
with a scrolling pane.

- **Steps**: window 390×844 ▸ a page ▸ Tablet.
- **Expected** (Atlat's call): the page can still be zoomed in to read: +
  is on at the fit, a step makes it bigger, and the pane scrolls.
- **Actual**: the page is drawn at 35%; + and − are off and the 35% label
  (Reset) does nothing.
- **Test**: `studio.qa.mjs` STU-15.
- **Code**: `crates/od-ui/src/app/studio.rs` `next_zoom` / `fit_cap`
  (`:334-340`, `:1291`): + is refused once the fit caps the zoom. Let + go
  past the fit (the pane already has `overflow: auto`, `app.css:856`), at
  least when the fit is below the smallest step.
- **Fixed** (c8df340): when the fit is below the smallest step, + goes past it (50 %, then up) and the stage scrolls; − from 50 % comes back to the fit; Reset or a pane wide enough clears it.

## QA-025 🔵 When + is off at the fit, its tooltip still says "Zoom in"

Reported by the UX review of 759581a.

- **Steps**: window 1360×860 ▸ a page ▸ Tablet ▸ hover +.
- **Expected** (Atlat's call): the tooltip says why + is off (e.g. "Fits
  the pane").
- **Actual**: "Zoom in", same as when it is on.
- **Test**: `studio.qa.mjs` STU-16 (steps + up until it is off, then
  compares `data-tip` with the button's label).
- **Code**: `studio.rs:1502`, the `zoom-in` button: a fixed
  `data-tip=t(T::ZoomIn)`. Make it reactive on `can_zoom(true)` and say why.
- **Fixed** (0a68f88): the zoom-in button's `data-tip` reads "Fits the pane" (18 locales) while the fit stops it.
