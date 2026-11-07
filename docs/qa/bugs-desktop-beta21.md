# QA bugs: desktop app 0.2.0-beta.21

The features new in beta.21, checked on the installed
`/Applications/Contour.app` 0.2.0-beta.21 (macOS 26, Apple silicon): the 13
new Motion templates, a chat turn shown as one timed task, comments (add,
pick, pick again, Retry), ⌘W and the red button hiding the window, the Dock
bringing it back, `target="_blank"` links, Help ▸ Check for Updates.
Numbering follows [bugs-desktop-beta20.md](bugs-desktop-beta20.md) (last
QA-052).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 0 | |
| 🟠 | 2 | QA-053, QA-054 |
| 🟡 | 1 | QA-055 |
| 🔵 | 5 | QA-056 … QA-060 |
| **Total** | **8** | |

QA-053 is already fixed on `main` (#91, after the beta.21 tag).

## How it was run

Never on the owner's app or data. The installed bundle was copied and the
copy run with its own data dir and the e2e mock `claude` first on `PATH`:

```sh
ditto /Applications/Contour.app <tmp>/ContourQA.app
open -n --env OD_DATA_DIR=<tmp>/data \
  --env PATH=<repo>/crates/od-ui/e2e/mock-agent:/usr/bin:/bin:/usr/sbin:/sbin \
  -a <tmp>/ContourQA.app
```

During the run something outside this test sent a Quit Apple Event to every
running `dev.contour.desktop` twice (12:35:24 and 12:38:09; the copy and an
installed instance quit together, the installed app was then opened again).
The copy was then given its own bundle id (`dev.contour.desktop.qa`,
re-signed ad hoc) so it no longer shares an identity with the installed app;
no further quits after that. Not a Contour bug as far as the logs show (the
sender is not in the unified log).

The window was driven with `osascript` (System Events, aimed at the copy's
pid, keys sent only while that pid is frontmost), CoreGraphics mouse events
(clicks only when the copy's window is the topmost Contour window) and
`screencapture -l<window id>`. Text was pasted from the clipboard: typed
keys go through the machine's Vietnamese input method ("hero" became
"hèo" in one message, not an app bug).

Flows that need no native window were also driven with Playwright against
the copy's own in-process daemon (the same UI bundle and API), scripts in
[desktop-beta21/](desktop-beta21/), run with `BASE=http://127.0.0.1:<port>`
(port from `<data>/logs/daemon.log`, line `in-process daemon listening`):

| Script | What it checks |
|---|---|
| `motion13.mjs` | each of the 13 templates: gallery preview, Remix → Studio preview, Export ▸ MP4 (codec, size, duration), frames at 15/50/90 %, nothing off loopback |
| `retry.mjs` | comments: a reload that fails keeps the list and offers Retry; Retry clears the error (`PROJ=<project id>`) |
| `clock.mjs` | the task's clock counts from the run's start across a reload and stops at the end |

```sh
BASE=http://127.0.0.1:59100 OUT=<dir> node docs/qa/desktop-beta21/motion13.mjs
```

Screenshots in [desktop-beta21/](desktop-beta21/).

## Results

| Area | Item | Pri | Result |
|---|---|---|---|
| Motion | 13 new templates listed, each opens a preview (Video · Motion) | P0 | Pass |
| Motion | Use template (App feature tour 9:16) → Home composer under Motion, brief filled | P0 | Pass, but the ratio and length shown are wrong (QA-055) |
| Motion | Create from that template → Motion project, run, preview | P0 | Pass |
| Motion | Remix each of the 13 → Studio shows the film | P0 | Pass (names: QA-056) |
| Motion | Export ▸ MP4 of each of the 13: h264, size and length as the composition (1920×1080, 1080×1920 ×3, 1080×1080; 5–14 s), 15–54 s each | P0 | Pass 13/13 |
| Motion | Frames at 15/50/90 % differ and are not blank | P1 | Pass 13/13 ([30-motion-13-frames.png](desktop-beta21/30-motion-13-frames.png)) |
| Motion | Gallery, preview and Studio ask nothing off loopback | P1 | Pass |
| Chat | Process line while running: "Step 2 of 3 · Writing the page · 0:03", counting | P0 | Pass ([03-task-running.png](desktop-beta21/03-task-running.png)) |
| Chat | After the run: "Worked for 0:10 · 2 steps" (mock streams 10 s), frozen | P0 | Pass ([04-task-done.png](desktop-beta21/04-task-done.png)) |
| Chat | Reply and Files from this turn outside the task; plan inside, in order | P0 | Pass |
| Chat | Reload mid-run: clock goes on from the run's start (0:04 → 0:04), ends at 0:10 | P1 | Pass (`clock.mjs`) |
| Chat | Plan with steps left keeps the task open, "Continue remaining tasks" shown | P1 | Pass |
| Comments | Add on a section and on an `h2`, Vietnamese and emoji note | P0 | Pass (popover: QA-057, label: QA-060) |
| Comments | Pick a comment in All comments → preview scrolls to it | P0 | Pass (tall element: QA-058) |
| Comments | Scroll away, pick the same comment again → scrolls back | P0 | **Fail** (QA-053) |
| Comments | Off-screen comment pins | P2 | QA-059 |
| Comments | Reload fails → list kept, "Could not refresh the comments… Retry"; Retry clears it | P1 | Pass (`retry.mjs`, [12](desktop-beta21/12-comments-refresh-failed.png)) |
| Window | ⌘W in the Studio with a tab open: closes the tab, app stays | P0 | Pass |
| Window | Red button: window hides, app and run keep going; Dock icon shows it | P0 | Pass (run ended while hidden, Done on return) |
| Window | Full screen → File ▸ Close Window → Dock: comes back windowed | P1 | Pass |
| Links | `target="_blank"` link inside the design preview | P1 | **Fail** (QA-054) |
| Updates | Help ▸ Check for Updates…: "You're up to date. Version 0.2.0-beta.21 is the newest." | P0 | Pass ([20](desktop-beta21/20-check-updates.png)); Update now not reachable (no newer release) |

## QA-053 🟠 Picking the same comment again does not scroll back to it

- **Steps**: page taller than the preview; add a comment on section 1 and
  one on section 10; open All comments; pick comment 1 (the preview scrolls
  to it); scroll the preview down; pick comment 1 again.
- **Expected**: the preview scrolls back to section 1, as on the first
  pick.
- **Actual**: nothing moves; the card stays selected, the outline is gone
  from view.
- **Status**: fixed on `main` by bc92ca29 (#91, "picking the same comment
  again scrolls back to it"), merged after the beta.21 tag; ships with the
  next release. Recheck on beta.22.
- **Screenshots**: [10-comment-pick.png](desktop-beta21/10-comment-pick.png),
  [11-comment-repick-no-scroll.png](desktop-beta21/11-comment-repick-no-scroll.png).

## QA-054 🟠 A `target="_blank"` link inside the design preview does nothing

- **Steps**: a page with `<a href="https://example.com/p1" target="_blank"
  rel="noopener">`; open it in the Studio (preview, not comment mode);
  click the link.
- **Expected**: the page opens in the system browser, as the app's own
  `_blank` links now do (QA-044) and as the web UI opens a new tab.
- **Actual**: nothing happens. `daemon.log`:
  `WARN od_desktop: blocked a navigation away from the app url=https://example.com/p1`.
- **Cause**: the QA-044 fix routes `_blank` clicks through `window.open`
  with a click listener on the app's document
  (`native_shell::route_new_window_links`, `app/mod.rs:340`). A click in
  the preview iframe is in another document, so the listener never sees
  it, and the native side refuses it in `on_navigation`
  (`apps/desktop/src/main.rs:916`, `allowed_navigation`).
- **Fix**: in `on_navigation`, hand http(s) URLs off the daemon origin to
  `open_in_browser` and return false (QA-044's first suggestion), or have
  the preview bridge post `_blank` clicks to the host.
- **Status**: fixed in #94 (the preview bridge posts a keyed `od:open-link`
  for a real click, the Studio calls the new `open_link` shell command;
  `on_navigation` still refuses, since handing off there would also open the
  browser for embedded iframes and script redirects; the shell drops a second
  opening of the same page within 1 s, as Windows and Linux also route the
  click to `on_new_window`); ships with the release after it merges. Recheck
  on beta.22 on macOS, Windows and Linux: one click, one tab.

## QA-055 🟡 Use template on a Motion film shows ratio 1:1 and 3 s

- **Steps**: Templates ▸ App feature tour (9:16) ▸ Use template.
- **Expected**: the Motion bar under the composer shows the film's
  9:16 and 12 s (the template declares `data-width="1080"
  data-height="1920" data-duration="12"`), or at least Motion's defaults
  (16:9, 10 s), as picking the Motion chip by hand does.
- **Actual**: "1:1" and "3s", next to a brief that says "12-second
  vertical tour". Nothing is sent for them (the project's metadata has no
  `videoAspect`/`videoLength`), so the selects show their first option, not
  a value; once the user touches one, that value goes to the agent.
- **Cause**: `home.rs:158` sets the chip from the template
  (`chip.set(chip_for_template(..))`) but not
  `media.set(MediaOptions::defaults(..))`, which the chip buttons do
  (`home.rs:245`, `home.rs:264`).
- **Screenshot**: [01-use-template-1x1-3s.png](desktop-beta21/01-use-template-1x1-3s.png).
- **Status**: fixed in #94 (Use template sets `MediaOptions::defaults` for
  the chip: 16:9, 10 s, not the film's own 9:16, 12 s, which the template
  list does not carry); ships with the release after it merges. Recheck on
  beta.22.

## QA-056 🔵 Remix names the project after the template id

- **Steps**: Templates ▸ Before and after ▸ Remix; go to Projects.
- **Expected**: "Before and after" (the name on the card).
- **Actual**: "motion-before-after"; the same for every template remixed
  from the gallery (`motion-cafe-menu`, `motion-year-recap` …).
- **Cause**: the gallery posts `{}`; `templates.rs:317` falls back to
  `entry.info.name` (the id) instead of the display name.
- **Status**: fixed in #94 (the gallery sends the card's name; the daemon
  falls back to the English display name); ships with the release after it
  merges. Recheck on beta.22.

## QA-057 🔵 The comment box opens below the preview for a tall element

- **Steps**: comment mode; click a section that fills the preview.
- **Actual**: the note box sits under the element
  (`preview_comments.rs:159`, element bottom + 6 px), so at the default
  window size (1360×810) "Add comment" and "Cancel" are below the preview
  edge; the user has to scroll inside the preview to find them.
- **Expected**: keep the box inside the preview (above the element, or
  pinned to the visible bottom).
- **Status**: fixed in #94 (the box is pinned to the preview's visible
  bottom when it would not fit under the element); ships with the release
  after it merges. Recheck on beta.22.

## QA-058 🔵 Picking a comment on a tall element hides its top

- **Steps**: comment on a section taller than the preview; pick it in
  All comments.
- **Actual**: the section is centred, so its heading is above the preview
  and only "Đoạn văn 1." shows.
- **Expected**: an element taller than the preview is aligned to its top.
- **Screenshot**: [10-comment-pick.png](desktop-beta21/10-comment-pick.png).
- **Status**: fixed in #94 (`od:mark` aligns an element taller than the
  frame to its top); ships with the release after it merges. Recheck on
  beta.22.

## QA-059 🔵 The pin of a comment scrolled out of view parks at the top edge

- **Steps**: comment on section 1; scroll the preview to section 10.
- **Actual**: pin "1" stays at the top right of the preview, over
  whatever is shown there, as if it belonged to it.
- **Cause**: `preview_comments.rs:102` clamps the pin's top with
  `(at.y - 10.0).max(0.0)`.
- **Expected**: hide it, or show it as an "above" marker.
- **Status**: fixed in #94 (the pin hides); ships with the release after it
  merges. Recheck on beta.22.

## QA-060 🔵 The comment target label runs the element's texts together

- **Steps**: comment mode; click a `<section>` holding an `h2`, a `p` and
  a link.
- **Actual**: the box reads "<section> Mục 1 — Tiêu đề có dấuĐoạn văn
  1.Link ng": the texts of the children are joined without spaces and cut
  mid-word.
- **Expected**: spaces between blocks (e.g. `innerText` rather than
  `textContent`), cut at a word.
- **Status**: fixed in #94 (`od:select` sends `label` (innerText); cut at a
  word); ships with the release after it merges. Recheck on beta.22.

## Not bugs (checked)

- ⌘W outside the Studio (Home, or the Studio with no tab) does nothing: on
  macOS File ▸ Close Window has no ⌘W on purpose (`main.rs` `menu_spec`,
  QA-042), as in the original's Electron menu. The red button and File ▸
  Close Window hide the window.
- The Studio preview of a 9:16 film fills the frame: the film's stage is
  letterboxed 9:16 and the page background has the same colour.
- "Your first design is ready" came back on a second project: the hint's
  "seen" bit is spent only when it is closed (`first_hint.rs`), and it had
  not been closed.
- After the comments went with a message they moved to Resolved; the Open
  tab says "No comments on this file yet": wording, not counted.
- "The entry file index.html is not the kind this project delivers" on a
  Motion project: the mock agent writes a plain page, not a film.
- Text the agent streams before its next tool call shows under the plan
  while running and moves into the task once that tool call comes: the
  agent's narration between steps belongs to the task.
