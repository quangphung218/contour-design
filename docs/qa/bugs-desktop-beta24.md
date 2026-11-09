# QA bugs: desktop app 0.2.0-beta.24

Recheck of QA-064 and QA-065 ([bugs-desktop-beta23.md](bugs-desktop-beta23.md)),
leaving Studio and Media right after a write (#114, #116, #117), and the
desktop checklist in [test-plan.md §18](test-plan.md), on the release asset
`Contour_0.2.0-beta.24_universal.app.tar.gz` (macOS 26, Apple silicon,
1440×900 points display, Dock at the bottom), 2026-11-09. The screen was
checked unlocked first (`CGSessionCopyCurrentDictionary`: no
`CGSSessionScreenIsLocked`, `kCGSSessionOnConsoleKey = 1`).

**macOS: QA-064 and QA-065 fixed; leaving a screen mid-write passes; §18
passes except the alert-loop line.** Four new bugs: a 5000-character alert
hides its OK button under the Dock (QA-066), an `alert` loop blocked with the
checkbox freezes the window and ⌘Q (QA-067), the preview's reload does not
lift the block (QA-068), and changing a media provider's base URL drops its
saved key without a word (QA-069). §18 on Windows and Linux: not run.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 0 | |
| 🟠 | 1 | QA-067 |
| 🟡 | 2 | QA-066, QA-069 |
| 🔵 | 1 | QA-068 |
| **Total** | **4** | |

## How it was run

Never on the owner's app or data: the release asset, given its own bundle id
and data dir; only that copy's pid was driven or quit.

```sh
gh release download v0.2.0-beta.24 -p '*universal.app.tar.gz' -D <tmp>
tar xzf <tmp>/Contour_0.2.0-beta.24_universal.app.tar.gz -C <tmp>
mv <tmp>/Contour.app <tmp>/ContourQA.app
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier dev.contour.desktop.qa" <tmp>/ContourQA.app/Contents/Info.plist
codesign --force --deep -s - <tmp>/ContourQA.app
open -n --env OD_DATA_DIR=<tmp>/data \
  --env OD_OPEN_COMMAND=<repo>/docs/qa/desktop-beta24/opener.sh \
  --env PATH=<repo>/crates/od-ui/e2e/mock-agent:/usr/bin:/bin:/usr/sbin:/sbin \
  -a <tmp>/ContourQA.app
```

`/api/version`: `0.2.0-beta.24`, `macos`, `aarch64`. Test projects were
written with [mkproj.sh](desktop-beta24/mkproj.sh)
(`BASE=http://127.0.0.1:<port> mkproj.sh NAME FILE`):
[dialogs.html](desktop-beta24/dialogs.html) (QA-064),
[design-b.html](desktop-beta24/design-b.html) (block in A, not in B),
[nested.html](desktop-beta24/nested.html) (QA-065, the NL/NK/NB cases of
`bridge.e2e.mjs`), [sec18.html](desktop-beta23/sec18.html) and
[loop.html](desktop-beta23/loop.html) (§18).

The window was driven with CoreGraphics events through
[go.sh](desktop-beta24/go.sh) (refuses unless the copy's pid is frontmost;
adds `wins`, `sclick` and `type` for Unicode text) and
[cg.swift](desktop-beta24/cg.swift) (`swiftc -O cg.swift -o cg`);
[step.sh](desktop-beta24/step.sh) prints how many lines one action added to
the opener log (`QA_DATA=<tmp>/data PID=<pid> step.sh LABEL WAIT go.sh-args…`).
A dialog is an extra window of the copy's pid at layer 8 (`go.sh wins`); its
text was read from `screencapture -l`. A design's own results show in its page
log (the preview CSP allows no beacon). Coordinates are window points of the
1360×810 window, preview at 100 %.

Leaving a screen mid-write ran twice: by hand in the window, and in
Playwright WebKit on the copy's daemon (same UI bundle), every write's reply
held 800 ms, a loopback fake image provider, `pageerror` and `panicked at`
collected:

```sh
cd crates/od-ui/e2e && npm ci
BASE=http://127.0.0.1:<port> node docs/qa/desktop-beta24/leave.mjs
```

[leave.mjs](desktop-beta24/leave.mjs), output in
[leave-result.json](desktop-beta24/leave-result.json); every URL the app
handed to the browser in [opens.txt](desktop-beta24/opens.txt).

## Results

### QA-064: a design's alert, confirm and prompt (macOS window)

| Check | Expected | Result |
|---|---|---|
| `alert('About to open')` | dialog "This page says" with the message; page waits | **Pass** (page back after 15 s, when OK was pressed) |
| `confirm('Open?')` OK / Escape / Cancel | `true` / `false` / `false` | **Pass** ([01](desktop-beta24/01-confirm-checkbox.png)) |
| `prompt('Name?', 'x')` | field holds `x`, focused; OK → `"x"`, typed → `"Bob"`, Escape → `null` | **Pass** ([02](desktop-beta24/02-prompt-default.png)) |
| 5000-character alert | first 2000 and `…`, buttons on screen | text **Pass**; buttons **Fail**: OK under the Dock (QA-066, [06](desktop-beta24/06-qa066-long-alert.png)) |
| `prompt('Tên?')`, type `Nguyễn Văn An ở Hà Nội`, OK | page gets exactly that | **Pass** (`len=22 eq=true`, [03](desktop-beta24/03-prompt-vietnamese.png)) |
| 40 alerts in a row | first without checkbox, second (within 3 s) with "Don't allow this design to show more dialogs" | **Pass** |
| a dialog more than 3 s after the last | no checkbox | **Pass** |
| tick, OK | loop ends at once (`n=40`), later `confirm` `false` and `prompt` `null` in 0–1 ms, nothing shown | **Pass** ([04](desktop-beta24/04-blocked-log.png)) |
| `for (;;) alert('x')`, tick, OK | no more dialogs; the app answers (menus, ⌘Q) | menus **Pass**; ⌘Q **Fail**, window frozen (QA-067, [07](desktop-beta24/07-qa067-frozen-menu.png)) |
| quit (Contour ▸ Quit), open the app again | dialogs show again | **Pass** |
| block in A, open B from Projects (no reload), B's `confirm('Delete?')` | dialog; OK → `true` | **Pass** ([05](desktop-beta24/05-design-b-after-block.png)) |
| block in B, back to A from the ⌘K palette | A's dialogs show again | **Pass** |
| block, then the preview's reload button | CHANGELOG: "stops the page's dialogs until it reloads" | **Fail**: still blocked (QA-068, [08](desktop-beta24/08-qa068-reload-still-blocked.png)) |

### §18 (macOS window)

| §18 item | Expected | Opens | Result |
|---|---|---|---|
| on_new_window always Deny | one app window; web addresses to the browser | only the main window listed; `blocked a new window` in `daemon.log` | **Pass** |
| J: `window.open` on `pointerup`, `mouseup`, `click`, ×3 | 1 each | 1, 1, 1 (`J-pointerup`) | **Pass** |
| K: Enter ×2, held Enter (8 repeats), Space | 1 each | 1, 1, 1, 1 | **Pass** |
| `window.open` on load and timers, then the preview reloaded | 0 | 0 | **Pass** |
| Tab out of L to the app, Shift+Tab back into L | 0 | 0 (page saw `Lku:Tab`) | **Pass** |
| comment mode: click J; click the nested NL link | 0, picks | 0 | **Pass** |
| M `if (confirm('Open?')) window.open(…)`: OK / Escape | dialog; 1 / 0 | dialog; 1 (`M-confirm`) / 0 | **Pass** |
| N `alert` then `window.open` | dialog; 1 | dialog; 1 | **Pass** |
| O `prompt` then `window.open`: OK / Escape | dialog; 1 each | dialog; `O-prompt-x` / `O-prompt-null` | **Pass** |
| M pressed with Enter, OK with Enter, then a lone keyup in L | 0 | 0 (page saw `Lku:b`) | **Pass** |
| Enter in P (`alert` on keydown), closed with Enter, then a lone keyup in L | 0 | 0 (`Lku:b`) | **Pass** |
| drag Q onto the drop zone, then a lone keyup in L | drop works; 0 | "dropped Q"; 0 (`Lku:b`) | **Pass** |
| JS dialog lines (QA-064 table above) | | | Pass except QA-066, QA-067 |

Tab into L from P is a key pressed in the design, so its own keyup opens 1
(`L-keyup-Tab`), as a browser allows; it was only the way to put focus in L.

### QA-065: `_blank` in a nested `srcdoc` frame (macOS window)

Each case after a press in the design (IDLE) or 6 s apart, as the
activation rule asks ([10](desktop-beta24/10-qa065-nested.png)).

| Case (`bridge.e2e.mjs`) | Expected | Opens | Result |
|---|---|---|---|
| NL nested `_blank` link | 1 | 1 (`nl-link`), 3 of 3 | **Pass** |
| NL2 the same link again at once | 0 (tab spent) | 0, 2 of 2 | **Pass** |
| NC the page cancels the link | 0 | 0 | **Pass** |
| NK Enter on a nested button, `window.open` on keyup | 1 | 1 (`nk-keyup`), 2 of 2 | **Pass** |
| NE Enter held 5.5 s | 0 | 0 | **Pass** |
| NB nested link whose page asks the parent to open too | 1 | 1 (`nb-link`, no `parent-go`) | **Pass** |
| PN design click opens and asks the nested frame to open | 1 | 1 (`pn-open`, no `ng-open`) | **Pass** |
| ⌘-click NL; Enter on NL | 1 each | 1, 1 | **Pass** |
| `srcdoc` read back keeps its doctype | `doctype` | `doctype` | **Pass** |

A click on a nested `<button>` does not focus it in WebKit (as in Safari),
so Enter there reaches nothing; Tab focuses it. That is WebKit's behaviour,
not the app's.

### Leaving a screen right after a write

| Check | Expected | Window | WebKit (`leave.mjs`, reply held 800 ms) |
|---|---|---|---|
| Studio: Send, then Home at once, then back | run made, composer empty, no panic | **Pass** (run done, composer empty, [11](desktop-beta24/11-leave-studio-back.png)) | **Pass** 3/3 (1 run POST, composer `""`, no page error) |
| Media: Add to queue, then Home at once | task made and done | **Pass** (1 task, provider hit once) | **Pass** 3/3 (also with the library project missing) |
| Media: delete 4 picked assets, then Home at once | all deleted | **Pass** (0 left, app alive) | **Pass** 2/2; run 1 see Not bugs |

No `panicked at` and no page error in any WebKit run; the window stayed
responsive after each.

### Not run

| Check | Why |
|---|---|
| §18 and QA-064 on Windows (WebView2) and Linux (WebKitGTK) | not run: no such machine in this pass |
| MP4 export left mid-render | not in this pass's brief |

## QA-066 🟡 A long `alert` is taller than the screen: OK sits under the Dock

- **Steps**: on a 1440×900-point display with the Dock at the bottom (a
  13" MacBook default), a design runs `alert` with 5000 characters
  ([dialogs.html](desktop-beta24/dialogs.html), LONG).
- **Expected**: §18: the first 2000 characters and `…`, the buttons on
  screen.
- **Actual**: the text is cut right (`…`), but the alert window is 873
  points tall from y = 30, its bottom 3 points past the screen; the OK
  button is behind the Dock and cannot be clicked
  ([06](desktop-beta24/06-qa066-long-alert.png)). Enter still closes it;
  a user who reaches for the mouse sees no button and the app is app-modal.
- **Cause**: `MAX_MESSAGE_CHARS = 2000` (`apps/desktop/src/js_dialogs.rs:43`)
  caps characters, not height; `NSAlert` grows with its informative text.
- **Fix**: cap the height, not only the length: put the message in a
  scrolling `NSTextView` as the alert's `accessoryView` (fixed height, about
  300 points) when it is longer than a few lines, or cut at fewer characters
  and lines. Then rerun the LONG row on a 900-point display.
- **Test**: §18 JS dialogs row, "the buttons on screen".

## QA-067 🟠 An `alert` loop, once blocked, freezes the window; ⌘Q does nothing

- **Steps**: a design button runs `for (;;) alert('x')`
  ([dialogs.html](desktop-beta24/dialogs.html), INF). OK the first alert;
  on the second tick "Don't allow this design to show more dialogs", OK.
- **Expected**: §18: no more dialogs, the app answers (menus, ⌘Q);
  CHANGELOG: "an `alert` loop cannot hold the app".
- **Actual**: no more dialogs, but the page now spins (`alert` returns at
  once, forever) in the web content process the app UI shares: the sidebar,
  Projects and every button stop answering
  ([07](desktop-beta24/07-qa067-frozen-menu.png)); the WebContent process
  sits at 47–65 % CPU. ⌘R does nothing, and **⌘Q does nothing** (waited
  15 s). The menu bar still opens; View has only Toggle Full Screen, so the
  one way out is Contour ▸ Quit with the mouse. ⌘Q quits at once when the app
  is not frozen (checked as a control), and the daemon still answered
  `/api/version` throughout.
- **Cause**: the design's sandboxed frame runs on the same WebKit main
  thread as the app's UI (no out-of-process iframes in WKWebView), so a
  page that loops without yielding holds both. Once blocked, `alert`
  returns synchronously, so the loop never yields. ⌘Q is a key equivalent
  WKWebView sends to the busy web process first.
- **Fix**: needs a decision. One way: when the gate refuses many dialogs
  from one webview in a short time (say 50 in 1 s), treat the page as hung:
  reload the preview from the native side by terminating the web content
  process (`WKWebView` reload after `_killWebContentProcess`, or the public
  `webViewWebContentProcessDidTerminate` path) and say so in the Studio.
  Also give View a native "Reload" (⌘R) that does not go through the page,
  so a frozen window has a keyboard way out. Or narrow §18 and the
  CHANGELOG to "no more dialogs; Contour ▸ Quit still works".
- **Test**: §18 "alert loop" row (INF), then the UI (click Projects) and ⌘Q.

## QA-068 🔵 The preview's reload button does not lift a design's dialog block

- **Steps**: block dialogs in a design (LOOP 40, tick, OK); press the
  Studio preview's reload button; press a `confirm` button.
- **Expected**: CHANGELOG beta.24: the checkbox "stops the page's dialogs
  until it reloads"; the page did reload (its log shows a new `loaded` time).
- **Actual**: still blocked: `confirm` returns `false` in 0 ms, no dialog
  ([08](desktop-beta24/08-qa068-reload-still-blocked.png)). The block lifts
  only on opening another design or on a new app page load (quit and open).
- **Cause**: the gate is per `WKWebView` and resets on the webview's own
  page load (`page_loaded`) or `reset_dialog_gate`; a reload of the design's
  iframe is neither.
- **Fix**: have the UI call `reset_dialog_gate` when the preview reloads
  (reload button, file change), as it does when another design opens; or
  change the CHANGELOG and §18 to say "until you open another design or
  restart the app".
- **Test**: QA-064 table, last row.

## QA-069 🟡 Changing a media provider's base URL drops its saved key without a word

- **Steps**: Media ▸ Providers, OpenAI shows "Key saved ··test". Leave the
  API key empty (the hint says "Leave a key empty to keep the saved one"),
  change the base URL to another host or port, Save.
- **Expected**: the key is kept, or the user is told it is not and why.
- **Actual**: "✓ Saved", and the badge turns to "no key"
  ([09](desktop-beta24/09-qa069-key-dropped.png)); `/api/media/config`
  has `configured: false`. Generate then has no key. Changing only a
  trailing `/` keeps the key.
- **Cause**: deliberate in the daemon: "A saved key belongs to the base URL
  it was saved with" (`crates/od-media/src/config.rs:185`, a key must not be
  sent to a new host). The UI hint does not say so and the save reports
  success.
- **Fix**: keep the rule; in the UI, when the base URL's host changes and
  the key field is empty, show "A new base URL needs the API key again"
  beside the field and do not save that provider until a key is typed (or
  save and show the notice). Update the hint text in `i18n.rs`
  (`ProvidersHint`, both languages).
- **Test**: by hand in the window (above); a `library.e2e.mjs` step could
  change the URL host with an empty key and expect the notice.

## Not bugs (checked)

- **Two media tasks from one Generate, once**: the first `leave.mjs` run made
  two tasks 466 ms apart (`65e3c09b…`, `17d1e791…`) for one click, and its
  bulk delete then left the second image, written after the cards were
  counted. Not reproduced since: 2 more WebKit runs (one with the library
  project missing, as in run 1) and 4 tries in the window each made exactly
  one task and one provider call; the UI code sends one POST. Likely the
  test harness (Playwright `route.fetch` on the first request in WebKit);
  noted in case it shows again.
- ⌘K with focus inside the design frame does not open the palette: the keys
  go to the design, as they should; the search icon opens it.
- A lone keyup of `c` or `d` sent by `cg` never reached the page (`b` did);
  a test-tool quirk, the rows use `b`.

## Still to do

- §18 and QA-064 on Windows (WebView2) and Linux (WebKitGTK), with the same
  pages.
- After QA-066 … QA-068: the LONG, INF and reload rows again.
