# QA bugs

Bugs found by the QA suite (`crates/od-ui/e2e/qa`, plan in
[test-plan.md](test-plan.md)). Each has a test that stays red, marked
`knownBug('QA-NNN')`, until it is fixed; `make e2e-qa` then reports it as
"known bug now passing" so the entry can be closed here.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

| ID | Sev | Summary | Test | Status |
|---|---|---|---|---|
| QA-002 | 🟠 | A message sent right after opening a project can vanish from the chat while the run goes on | `chat.qa.mjs` CHAT-09 | open |
| QA-003 | 🟠 | Tablet preview lays the page out at the pane width, not 820 px | `studio.qa.mjs` STU-11 | open |
| QA-001 | 🟡 | Renaming a project to blank shows a nameless card and no error | `projects.qa.mjs` PRJ-02 | open |

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
