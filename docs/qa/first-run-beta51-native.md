# QA: native check of 0.2.0-beta.51

Released `Contour_0.2.0-beta.51_universal.dmg`, macOS 26, Apple silicon,
2026-11-20 17:15–17:48. Stopped at 17:48 when the owner came back, before
the restart step of criterion 4.

**Check setup and quit-while-running work. Two exports are wrong.** The
PDF of a slide deck fills only the top-left two thirds of each page
(QA-119), and the editable PowerPoint loses the deck's colours (QA-120).
PowerPoint as images and MP4 are correct.

| # | Criterion | Result |
|---|---|---|
| 1 | Settings ▸ About ▸ Check setup runs, every check has a result, a failing check says how to fix it | **Pass** |
| 2 | Design with an entrance animation, Export PDF, PPTX and MP4 through the Save panel, content right | **Fail**: PDF (QA-119) and PPTX editable (QA-120). PPTX images and MP4 pass |
| 3 | ⌘Q during a run (QA-114): dialog counts agents, Cancel keeps the run, Quit and stop agents exits, turn reads "Stopped", files kept | **Pass** |
| 4 | Update now beta.50 → beta.51, Restart, app reports beta.51 | **Not finished**: check, download and install pass; Restart not run (see below) |

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## New bugs

| ID | Sev | Bug |
|---|---|---|
| QA-119 | 🟠 | PDF of a slide deck: each 960×540 pt page holds the slide at 640×360 pt in its top-left corner; the rest is white |
| QA-120 | 🟠 | PowerPoint ▸ Editable text: slide background is the deck's dark shell colour and every text run has no colour |
| QA-121 | 🔵 | A run stopped by quitting reads "Stopped" in the chat but "Canceled" on its Home and Projects card |
| QA-122 | 🟡 | Check setup says "macOS asks nothing" while the first agent test of the same copy raised the Documents dialog |

### QA-119 🟠 Deck PDF is printed at two thirds of the page

- Steps: Home ▸ Slide deck, any prompt (here a 3-slide bakery deck); the
  page uses the built-in deck frame (`.deck-stage` 1920×1080, `.slide`
  sections). Export ▸ PDF ▸ Save.
- Expected: 3 pages, each filled by its slide.
- Actual: 3 pages of 960×540 pt; the slide is drawn at 640×360 pt in the
  top-left corner, the right third and bottom third are white
  ([page 1](first-run-beta51-native/01-pdf-deck-page1.png)). Text is all
  there (`pdftotext`). PowerPoint ▸ Slides as images of the same deck is
  full-size, so the page itself lays out fine.
- Where to look: `crates/od-render-cdp/src/lib.rs` `CdpRenderer::pdf`, the
  deck branch (`scale = css_width / s.width`, paper 13.33 in) together with
  `script::print_slides` and the deck frame's own `@media print` rules
  (`width: 1920px !important` on `.slide` and `.deck-stage`). The slide
  comes out at 1280/1920 of the intended size, as if the scale is applied
  once more than needed.

### QA-120 🟠 Editable PowerPoint drops the deck's colours

- Steps: same deck, Export ▸ PowerPoint ▸ Editable text ▸ Save; open in
  Keynote, PowerPoint or LibreOffice.
- Expected: cream slide (`--bg`), dark text (`--fg`), rust accents, as in
  the preview and the image PPTX.
- Actual: every slide's background is `#08090D`, and no `<a:r>` carries a
  colour, so text falls back to the theme's ([slide 2 in
  LibreOffice](first-run-beta51-native/03-pptx-editable-slide2.png)).
  The text itself is complete on all 3 slides.
- Two causes, both in `crates/od-export`:
  - `pptx_layout.js:16-19` takes the background from the `.slide`, then
    `body`; the deck frame paints `body` with `--shell: #08090d` and the
    slide colour sits on `.deck-stage`, which is not looked at.
  - `pptx_editable.rs:131` `parse_color` reads only `#hex`, `rgb()` and
    `rgba()`. The deck uses `oklch()`, which Chrome keeps in computed
    styles, so every colour is dropped.
- The lint chip "Fix 1 issue: slide theme missing" was shown under the
  turn; it may be related, but the export should not need it.

### QA-121 🔵 "Stopped" in the chat, "Canceled" on the card

- After Quit and stop agents and a relaunch, the turn reads "Stopped —
  Contour was quit while this turn ran…" ([06](first-run-beta51-native/06-reopen-stopped.png)),
  but the Home card reads "Canceled 2 min ago". The run's state is
  `canceled`, `failureCategory: app_quit`.
- Where to look: `crates/od-ui/src/app/manage.rs:71` maps `Canceled` to
  `T::StatusCanceled` without looking at `app_quit` (#263 changed only the
  turn).

### QA-122 🟡 Check setup says macOS asks nothing, macOS asked

- The data folder was under `/private/tmp`, no project in Documents. On the
  first launch the Claude Code test passed (38954 ms), and the TCC table
  has `kTCCServiceSystemPolicyDocumentsFolder` for the copy, allowed at
  17:16:08. Check setup at 17:16:47 then said "No project folder is in
  Documents, Desktop or Downloads; macOS asks nothing."
- The beta.50 copy, same setup, showed the Documents dialog during its
  first test at 17:42 (answered Don't Allow; its test then timed out at
  45 s).
- So the agent CLI itself reads `~/Documents` on this machine, which the
  check does not cover. Either check that too or word the line as "no
  project folder of Contour's is in…".
- Not known: who answered the 17:16 dialog. The tester did not see it;
  `HIDIdleTime` dates a physical input to about 17:16:02, which was not the
  tester's.

## Criterion 1: Check setup

Clicked at 17:16:47, all seven results in about 3 s:

```
✓ Daemon 0.2.0-beta.51 answers at http://127.0.0.1:51729.
✓ Data folder <scratch>/qa51data is writable.
✓ Claude Code 2.1.329 (Claude Code) at ~/.local/bin/claude.
✓ /Applications/Google Chrome.app/Contents/MacOS/Google Chrome opens a blank page for export.
✓ ffmpeg version 9.0.2 at /opt/homebrew/bin/ffmpeg.
✓ No project folder is in Documents, Desktop or Downloads; macOS asks nothing.
✓ 32195 MB free on the disk of <scratch>/qa51data.
```

Failing path: the copy opened again with `OD_CHROME_PATH=/usr/bin/false`
and `OD_FFMPEG_PATH=/nonexistent/ffmpeg`:

```
✗ /usr/bin/false could not show a blank page: cannot start /usr/bin/false: Input/Output
  error while resolving websocket URL from browser process, stderr: BrowserStderr(""):
  unexpected end of stream.
  Open the browser once by hand (macOS may need to approve it), or point OD_CHROME_PATH to another one.
! No ffmpeg, so no MP4 export: OD_FFMPEG_PATH points to /nonexistent/ffmpeg, which does not exist.
  Install ffmpeg (macOS: `brew install ffmpeg`) or set OD_FFMPEG_PATH.
```

Each failing line has a fix under it. Two notes, not filed: the Chrome
line shows the library's `BrowserStderr("")`, and "set OD_CHROME_PATH" is
hard to act on for a Finder-launched macOS app.

## Criterion 2: exports

Two sends to Claude Code, because PowerPoint needs slides and MP4 needs a
`[data-composition-id]` root, so no one page offers all three (the menu
hides what does not fit, QA-031):

1. Slide deck, "A 3-slide deck for a neighborhood bakery … title and text
   fade in and rise on load … (start at opacity 0)". Done in 78 s. Every
   text block is `.rise`: `opacity: 0; animation: rise-in 900ms … both`,
   delays to 680 ms.
2. Motion, "A 6-second 1920x1080 motion clip … 'Linden Street Bakery'
   fades in and rises, then 'Fresh bread from 7:00' slides in". Done in
   92 s.

| Export | Save panel after click | File after Save | Content |
|---|---|---|---|
| Deck ▸ PDF | 4 s | 1 s | 3 pages 960×540 pt, all text, slide at two thirds size (**QA-119**) |
| Deck ▸ PowerPoint ▸ Slides as images | 3 s | under 1 s | 3 slides, 3 PNGs 1920×1080 filling 16:9; darkest text pixel rgb(28, 20, 14) = `--fg`, so captured after the animation ([02](first-run-beta51-native/02-pptx-images-slide1.png)) |
| Deck ▸ PowerPoint ▸ Editable text | 2 s | under 1 s | 3 slides, every text run present, colours wrong (**QA-120**) |
| Motion ▸ MP4 video | 28 s | under 1 s | H.264 1920×1080, 30 fps, 180 frames, 6.000 s, AAC audio; frames at 0.2 / 1.5 / 3 / 5.5 s show the title then the line arriving ([04](first-run-beta51-native/04-mp4-frames.png)) |

The MP4 Save panel opens only after the 28 s render, and it proposes
`index` as the file name (the page is `motion-source/linden-street-bakery/index.html`).

The deck's own `@media print` turns the animation off, so the PDF does not
test the animation wait; the image PPTX does.

## Criterion 3: quit while an agent runs

| Time | Step | Result |
|---|---|---|
| 17:35:44 | Prototype, "A 5-page site for a bakery. Write index.html first…" | Run starts |
| 17:36:36 | | `index.html` written |
| 17:37:05 | ContourQA51 ▸ Quit Contour (the ⌘Q menu item) | "Quit Contour? 1 agent is still running. Quitting stops it; files it already wrote are kept." Cancel / Quit and stop agents ([05](first-run-beta51-native/05-quit-dialog.png)) |
| 17:37:22 | Cancel | Dialog closes; the run writes `about.html` at 17:37:52 |
| 17:37:59 | Quit Contour again | Same dialog |
| 17:38:10 | Quit and stop agents | App gone at 17:38:11; no `claude` process from the run left; `index.html`, `styles.css`, `about.html` md5 unchanged |
| 17:38:24 | Open the copy again | Turn: "Worked for 2:27 · 3 steps", three files listed, "Stopped — Contour was quit while this turn ran, so it stopped here. Files it already wrote are kept.", Retry ([06](first-run-beta51-native/06-reopen-stopped.png)) |

The quit went through the app menu item by accessibility press, because
agent-desktop refuses to send ⌘Q. The item is the one ⌘Q triggers.

## Criterion 4: Update now from beta.50

| Time | Step | Result |
|---|---|---|
| 17:39:33 | Open the beta.50 copy (`dev.contour.desktop.qa50u`) | Documents dialog during the first test (QA-122); Don't Allow; test timed out at 45 s; Skip for now |
| 17:43:37 | Settings ▸ About ▸ Check for updates | "Version 0.2.0-beta.51 is available" in 2 s |
| 17:43:52 | Update now | "Updating to version 0.2.0-beta.51", then "Version 0.2.0-beta.51 is installed. Restart Contour to start using it." Later / Restart ([07](first-run-beta51-native/07-update-installed.png)); seen at 17:47:49 |
| | Bundle on disk | `~/Applications/ContourQA50u.app` replaced in place: version 0.2.0-beta.51, `codesign --verify --deep --strict` valid, adhoc, 5594 files, id `dev.contour.desktop` |

**Why Restart was not run.** The update put the release bundle in place,
with the release id `dev.contour.desktop`. Restarting it would run the
owner's identity (its WebKit storage, preferences and TCC rows), which
this test may not touch. The bundle was given the QA id again and
re-signed before Restart; after that, macOS accessibility could no longer
read the still-running beta.50 window (`AXWindows` error -25211), and the
owner came back at about 17:48:30, so the test stopped. This is a limit of
the isolation, not a product failure. To finish it, either test Update now
on a machine or user account without the owner's Contour, or re-sign the
bundle before the beta.50 copy starts the download.

## How it was run

- **Owner's app untouched.** `dev.contour.desktop` was not running and was
  not opened. No file under `~/Library` for `dev.contour.desktop` changed
  during the test (checked with `find -newermt`).
- **QA copies.** `ditto` from the DMG to `~/Applications/ContourQA51.app`
  and `ContourQA50u.app`, `Info.plist` `CFBundleIdentifier`
  `dev.contour.desktop.qa51` / `.qa50u` and `CFBundleName` changed,
  `codesign --force --deep --sign -`, then
  `open -n --env OD_DATA_DIR=<scratch>/data <copy>`.
- **Driving.** agent-desktop on the copy's window; prompts by clipboard
  paste (the owner's clipboard was saved and restored), because
  accessibility `type` does not reach the WKWebView text area. Window
  captures with `screencapture -l <window id>` of the copy only.
- **Cleanup.** `lsregister -u`, copies, data folders, caches, WebKit
  storage, preferences, DMGs and exported files removed; `tccutil reset All`
  for both ids (0 rows left in the user TCC table).

### Signature checks

| App | `codesign --verify --deep --strict` | `codesign -dv` |
|---|---|---|
| beta.51 in the DMG, untouched | valid on disk, satisfies its Designated Requirement | `Signature=adhoc`, `Sealed Resources version=2 files=5594`, id `dev.contour.desktop` |
| Copy, `Info.plist` edited | `invalid Info.plist (plist or signature have been modified)` | |
| Copy, after `codesign --force --deep --sign -` | valid on disk, satisfies its Designated Requirement | `Signature=adhoc`, `files=5594`, id `dev.contour.desktop.qa51` |
| beta.50 copy after Update now | valid on disk | `Signature=adhoc`, `files=5594`, id `dev.contour.desktop` |

## Not checked

- Restart after Update now (criterion 4).
- PNG, JPEG and WebP exports; PDF of a non-deck page (passed in beta.50).
- Next-step chips: they come from the agent's reply, so they follow the
  agent's language, not the app's; on this machine Claude Code answered in
  Vietnamese while the app was in English.
