# QA bugs: desktop app on main before beta.34 (10b53672)

Manual pass on macOS of the desktop app built from `main` at 10b53672
(PR #175 WebKit callback queue, #178, #179, #182 fixes for QA-089 to
QA-092): [test-plan.md](test-plan.md) §19 (a page that hangs or crashes),
§21 by hand in WKWebView (typing, then leaving), the History fix (QA-089)
and the comment draft fix (7e650cec). macOS 26, Apple silicon, 2026-11-13.

**Everything the brief asked for passes except one row: a comment draft
(note and image) is thrown away when the agent rewrites the file it is on
(QA-095). §19 passes: the "isn't responding" alert closes by itself, a
page's own alert above it is left alone, and the crash loop stops the
preview. Text typed and left at once (Ctrl+Tab, Cmd+W, leaving the Studio,
closing the window) is saved, for Markdown and sketch labels. History shows
no version of a.html over b.html. Two desktop bugs found on the way: on
WKWebView, Backspace outside a text field goes Back, and the Vietnamese
Telex input method sends a Backspace for every accent (QA-093); a new
sketch label field does not take the focus (QA-094), so the letters go to
the page.** No `panicked` line in the app's log.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause |
|---|---|---|---|---|
| QA-095 | 🟠 | A comment draft (typed note, attached image) disappears when the agent or the watcher rewrites the file it is on; the image stays behind in `comments/` | manual, macOS window | `studio.rs:2078`: the Effect on `src` clears `comment_draft` on every new document, a reload of the same file included |
| QA-093 | 🟠 | Backspace with no text field focused goes Back (leaves the Studio). With the Telex input method every accent typed (s f r x j, aa, ee…) sends a Backspace, so typing Vietnamese where no field has the focus walks back through the app | manual, macOS window | WKWebView's backspace-goes-back; the app has no keydown guard |
| QA-094 | 🟡 | The sketch's text label field shows but has no focus, except for the first one after a page load: what is typed goes to the page (and, with QA-093, can leave the Studio) | manual, macOS window | `preview_tools.rs:1638` uses the `autofocus` attribute, which a document honours once |

## How it was run

`make app-debug` in the worktree, then the bundle copied as
`ContourQA.app`, bundle id `dev.contour.desktop.qa`, ad-hoc signed, its own
`OD_DATA_DIR`, the opener log as `OD_OPEN_COMMAND`, the mock agent on
`PATH`, the binary run directly to keep its log (as on
[beta.28](bugs-desktop-beta28.md#how-it-was-run)). Only that copy's pid was
driven and quit (⌘Q); the owner's Contour was not running during the pass
(it had exited at 14:41:54, before the copy started). `/api/version`:
`0.2.0-beta.33`, `macos`, `aarch64`.

Tools: beta.24's [go.sh](desktop-beta24/go.sh), [mkproj.sh](desktop-beta24/mkproj.sh),
[opener.sh](desktop-beta24/opener.sh); new in [desktop-main34](desktop-main34/):

- [cg.swift](desktop-main34/cg.swift): beta.24's, plus `key … ctrl`, a real
  modifier key down and up around the key (Telex turned ⌘W sent with flags
  only into the letter "ư"), and `loc` (mouse position).
- [busy34.sh](desktop-main34/busy34.sh) `BTN_X BTN_Y [WAIT_X WAIT_Y] [SECS]`:
  press a busy button, ⌘R, optionally Wait, and log every layer-8 window
  with the time, the mouse position and the frontmost pid.
- [dialogs34.html](desktop-main34/dialogs34.html): dialogs25 plus BA (busy
  15 s, then `alert`; when the alert returns it opens
  `https://qa.invalid/ba?…`, so [opens.txt](desktop-main34/opens.txt) logs
  how long it stayed up).
- [typeleave.sh](desktop-main34/typeleave.sh): click a field, type, then a
  key or a click at once (printed: ms after the last letter).
- [keylog.html](desktop-main34/keylog.html): logs every `keydown` the page gets.
- [put.sh](desktop-main34/put.sh): write one file of a project.

**Traps**: the preview iframe is sandboxed, so `localStorage` throws, and
`window.name` holds the app's own `__odGuard`; do not use either to keep
state across a reload. Projects sort by last change, so a card's position
moves after a write. The machine's Telex input method rewrites typed text
(s f r x j w, double vowels): use test words without them.

## Results

### §19: a page that hangs or crashes

| Check | Expected | Result |
|---|---|---|
| BUSY 20 s, ⌘R, nothing pressed | the alert closes by itself when the page answers | **Pass**: alert +3.9 s, closed +21.1 s (BUSY ends ≈ +20.8), page reloaded |
| BUSY 20 s, ⌘R, Wait | asks again; closes by itself once the page reloads | **Pass** 2 of 2: Wait +4.5, asks again +9.8 / +9.6, closed +21.4 / +21.5, page reloaded |
| BA (busy 15 s, then `alert`), ⌘R, nothing pressed | the page's alert shows above "isn't responding" and is never closed by it; OK lets the page answer | **Pass** ([01](desktop-main34/01-page-alert-over-not-responding.png)): page alert +15.9 s, both up 8 s more; OK → both gone < 0.5 s, page reloaded ([02](desktop-main34/02-after-ok-reloaded.png)) |
| BA, ⌘R, Wait | the same, under the re-asked alert | **Pass** in the 6 runs nobody touched: both held ≥ 35–60 s until OK. In 4 other runs both closed 6–40 s after the page alert, `alert` returning normally ([opens.txt](desktop-main34/opens.txt)); in 2 of them the log shows the mouse moved by a person (734,395 and 779,389, both inside the page alert), the other 2 were before the mouse was logged. Deliberate clicks off the buttons (4 points, both windows) closed nothing |
| Crash loop, `OD_QA_CRASH_WEB=1`, Crash34 open, ⌘R | log names the hook; two crashes; Studio says it crashed twice; Run again runs it | **Pass** 2 of 2 ([log](desktop-main34/crash-loop.txt)): crashes 0.12 s apart, `after=Reload` then `after=StopPreview`; "This design crashed the page twice in a row" ([15](desktop-main34/15-crash-loop-stopped.png)); Run again: design runs, no crash ([16](desktop-main34/16-crash-run-again.png)) |

### Typing, then leaving (WKWebView)

Each leave sent 140–200 ms after the last letter (under the 700 ms autosave
delay); the file read back through the API.

| Check | Expected | Result |
|---|---|---|
| Markdown, Ctrl+Tab | next tab; text in the file | **Pass** |
| Markdown, Cmd+W | tab closed; text in the file | **Pass** |
| Markdown, Projects in the sidebar (leave the Studio) | text in the file | **Pass** (170 ms) |
| Markdown, close the window (red button) | text in the file | **Pass**: the window hides, the page lives on, text saved |
| Sketch label, Ctrl+Tab | next tab; label in the file | **Pass** ([03](desktop-main34/03-sketch-ctrl-tab-saved.png)) |
| Sketch label, Cmd+W | label in the file | **Pass** once the field has the focus; see QA-094 |
| Sketch label, leave the Studio | label in the file | **Pass** (175 ms) |
| Sketch label, close the window | label in the file | **Pass** |

### History (QA-089)

| Check | Expected | Result |
|---|---|---|
| a.html: View v2, Ctrl+Tab to b.html | b.html, its own versions, no "Viewing v2" | **Pass** ([10](desktop-main34/10-history-viewing-v2.png), [11](desktop-main34/11-history-ctrl-tab-b.png)) |
| a.html: View v1, Cmd+5 (b.html) | the same | **Pass** ([12](desktop-main34/12-history-cmd5-b.png)) |

### Comment on the preview (7e650cec)

| Check | Expected | Result |
|---|---|---|
| Element picked, note typed, `other.html` written in the project folder | note kept | **Pass** |
| Attach image (open panel), `other2.html` written while the panel is up | note and image kept | **Pass** |
| `other3.html` written | note and image kept | **Pass** ([13](desktop-main34/13-comment-kept-on-refresh.png)) |
| `index.html` (the file commented on) rewritten, as an agent does | note and image kept | **Fail**: QA-095 |

## QA-095 🟠 A comment draft goes when the agent rewrites its file

- **Steps**: open `index.html`, comment mode, click an element, type a
  note, attach an image. Rewrite `index.html` on disk (or let the agent
  edit it).
- **Expected**: the draft box stays with the note and the image (the
  element may have moved; it can be picked again), as it does when another
  file changes (7e650cec).
- **Actual**: the preview reloads and the draft box is gone with what was
  typed; picking the element again opens an empty box
  ([14](desktop-main34/14-qa095-draft-gone.png)). The attached image stays
  in `comments/` with nothing pointing at it. Nothing says the draft was
  dropped. 1 of 1.
- **Cause**: `crates/od-ui/src/app/studio.rs:2078`, the Effect that tracks
  `src` ("A new document") sets `ctx.comment_draft` to `None`. A rewrite of
  the same file is a new `src`, so it counts as a new document.
- **Fix**: clear the draft only when the shown path changes, not on a
  reload of the same file; keep the note and images (they live in
  `DraftBox`'s own signals, so they also need to move to `StudioCtx` or the
  box must not be rebuilt). If the element is gone after the reload, keep
  the box and say the element must be picked again.
- **Test**: `preview.e2e.mjs` comments: after `comment-image-input`, write
  the shown file through the API, then expect `comment-note` to keep its
  value and `comment-image` to still show.
- **Siblings**: the same Effect also clears `ctx.deck` and
  `ctx.export_fit`, which are rebuilt by the page; only the draft is user
  work.

## QA-093 🟠 Backspace outside a text field goes Back; Telex makes it fire while typing

- **Steps**: in the Studio, click an empty part of the chat pane (no field
  focused) and press Backspace (⌫). Or, with the macOS Vietnamese Telex
  input method, click into a design's preview and type `first`.
- **Expected**: nothing happens (Chrome and Firefox dropped
  Backspace-goes-back; the web app does not leave on it).
- **Actual**: the app goes Back: from the Studio to Projects
  ([08](desktop-main34/08-qa093-before-backspace.png) →
  [09](desktop-main34/09-qa093-after-backspace.png)). With Telex, the page
  first gets `f`, `i` ([06](desktop-main34/06-qa093-keylog.png)); at `r`
  Telex sends a Backspace to put the accent on, and the app goes back to
  Projects ([07](desktop-main34/07-qa093-r-went-back.png)); `s` goes back
  again, to the page before. Nothing typed was lost in this pass (the
  Markdown and sketch saves on leaving pass, above), but the user is
  thrown out of the Studio mid-sentence. Seen 5 times.
- **Where**: this is WKWebView (Safari's old Backspace = Back); the web
  app on Chromium does not do it. macOS's delete key sends `Backspace`, so
  the Studio's own `Delete` handlers (a tab, a terminal:
  `studio.rs:1898`, `studio.rs:1926`) are not reached on a Mac keyboard and
  the same key goes Back instead.
- **Fix**: in the desktop app, a window `keydown` listener that calls
  `preventDefault()` on `Backspace` when the target is not editable
  (`input`, `textarea`, `contenteditable`), as `studio::is_editing`
  decides; or turn off WebKit's backspace navigation in the shell if the
  preference is reachable. Check the design iframe too: the key reached the
  app from inside the preview.
- **Test**: desktop manual row in §19 (keylog page, Backspace in the
  Studio's chat pane: still in the Studio).

## QA-094 🟡 A new sketch label field has no focus

- **Steps**: new sketch, Text pen, click the canvas, type, Enter (a first
  label). Click the canvas again and type.
- **Expected**: the second field takes the letters, as the first did.
- **Actual**: the field shows "Type, then Enter" with no caret; the letters
  go to the page ([04](desktop-main34/04-qa094-label-not-focused.png)). Only
  a click into the field lets one type
  ([05](desktop-main34/05-qa094-typed-after-click.png)). After a page load
  (⌘R) the first label gets the focus again, the second does not. With
  Telex the lost letters can also go Back (QA-093). 5 of 5 for a second
  label in the same page load.
- **Cause**: `crates/od-ui/src/app/preview_tools.rs:1638` relies on the
  `autofocus` attribute of a field built later; browsers apply `autofocus`
  once per document.
- **Fix**: focus the field from a `NodeRef` in an Effect when `text_at`
  becomes `Some`, as `lib_ui.rs` does for dialogs.
- **Siblings, same pattern, not checked by hand**: `home.rs:1084` (the
  "from folder" path field) and `chat_ops.rs:132` (rename a conversation)
  also use `autofocus=true` on a field shown later.
- **Test**: LV-14 uses `fill()`, which focuses the field itself, so it
  cannot see this; add a second label typed with `keyboard.type` after a
  click on the canvas.
