# QA bugs: desktop app 0.2.0-beta.23

Recheck of QA-055 and QA-061 … QA-063 ([bugs-desktop-beta21.md](bugs-desktop-beta21.md),
[bugs-desktop-beta22.md](bugs-desktop-beta22.md)), the Next chips after a
run (#106), and the desktop checklist in [test-plan.md §18](test-plan.md),
on the release asset `Contour_0.2.0-beta.23_universal.app.tar.gz`
(macOS 26, Apple silicon), 2026-11-08.

Two passes. Pass 1 (05:12) found the Mac's screen locked
(`CGSSessionScreenIsLocked = 1`) and ran only what works without the window:
QA-055, the Next chips and the bridge in WebKit. Pass 2 (10:10, screen
unlocked, checked the same way) ran QA-061 … QA-063 and every §18 item on the
real macOS window.

**macOS: QA-061, QA-062, QA-063 fixed; §18 passes for links and presses.**
Two new bugs: a design's `alert`/`confirm`/`prompt` never shows in the
desktop window (QA-064), and a `_blank` link inside a `srcdoc` iframe does
nothing (QA-065). §18 on Windows and Linux is still not run.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 0 | |
| 🟠 | 1 | QA-064 (fixed on `js-dialogs`) |
| 🟡 | 0 | |
| 🔵 | 1 | QA-065 |
| **Total** | **2** | |

## How it was run

Never on the owner's app or data. The release asset, extracted, given its
own bundle id, its own data dir, the e2e mock `claude` first on `PATH`, and
`OD_OPEN_COMMAND` pointing at [opener.sh](desktop-beta22/opener.sh):

```sh
gh release download v0.2.0-beta.23 -p '*universal.app.tar.gz' -D <tmp>
tar xzf <tmp>/Contour_0.2.0-beta.23_universal.app.tar.gz -C <tmp>
mv <tmp>/Contour.app <tmp>/ContourQA.app
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier dev.contour.desktop.qa" <tmp>/ContourQA.app/Contents/Info.plist
codesign --force --deep -s - <tmp>/ContourQA.app
open -n --env OD_DATA_DIR=<tmp>/data \
  --env OD_OPEN_COMMAND=<repo>/docs/qa/desktop-beta22/opener.sh \
  --env PATH=<repo>/crates/od-ui/e2e/mock-agent:/usr/bin:/bin:/usr/sbin:/sbin \
  -a <tmp>/ContourQA.app
```

`/api/version` of the copy: `0.2.0-beta.23`, `macos`, `aarch64`. Onboarding
was finished with `PUT /api/app-config {"onboardingCompleted":true,"agentId":"claude"}`.
Then, in Playwright WebKit against the copy's daemon:

```sh
BASE=http://127.0.0.1:<port> OUT=<dir> RUNS=5 node docs/qa/desktop-beta23/recheck.mjs
node crates/od-ui/e2e/bridge.e2e.mjs
```

[recheck.mjs](desktop-beta23/recheck.mjs), output in
[recheck-result.json](desktop-beta23/recheck-result.json).

## Window pass (pass 2)

Same copy of the release asset, a fresh data dir, onboarding finished in
the window (Continue). The link targets point at a local
`python3 -m http.server 58990`; `OD_OPEN_COMMAND` logs every URL the app
hands to the browser, so no real browser opened. Three projects were written
with `POST /api/projects/<id>/files`:
[qa054-links.html](desktop-beta22/qa054-links.html) (QA-061 … QA-063),
[sec18.html](desktop-beta23/sec18.html) (§18 presses, dialogs, drag) and
[loop.html](desktop-beta23/loop.html) (`window.open` every 200 ms).

The window was driven with CoreGraphics events (mouse, keys, autorepeat,
drag) through [go.sh](desktop-beta23/go.sh), which refuses unless the copy's
pid is frontmost, and [cg.swift](desktop-beta23/cg.swift)
(`swiftc -O cg.swift -o cg`); [step.sh](desktop-beta23/step.sh) runs one
action and prints how many lines it added to the opener log:

```sh
PID=<copy's pid> docs/qa/desktop-beta23/step.sh "A plain" 2 click 554 177
```

Coordinates are window points of the 1360×810 window with the Studio
preview at 100 %. "Opens" below is the opener-log count per action.

## Results

| Check | Expected | Result |
|---|---|---|
| QA-055 Templates ▸ App feature tour (9:16) ▸ Use template | Motion bar shows 9:16 and 12s | **Pass** ([01](desktop-beta23/01-qa055-use-template.png)) |
| QA-055 then Create | project metadata `videoAspect 9:16`, `videoLength 12` | **Pass** (`{"kind":"video","videoAspect":"9:16","videoLength":12}`) |
| `/api/design-templates` film data | each Motion film carries its size and length | **Pass** (31 of 185 templates; App feature tour 1080×1920, 12 s) |
| Next chips after a run, 1360×810, two turns, 5 projects | chips inside the chat, no "Jump to latest" | **Pass 5/5** ([02](desktop-beta23/02-next-chips-1360.png)) |
| Next chips after a run, 390×844, 5 projects | same | **Pass 5/5** ([03](desktop-beta23/03-next-chips-390.png)) |
| Preview bridge alone (`bridge.e2e.mjs`), Chromium and WebKit | all cases | **Pass** (C0, T1, K1, N1–N3, R1–R2, G, D1–D3 open one each; G2, S3, TP, FK, TF, TS, SW, P1, X1–X3, Q2 open none) |

### macOS window: QA-061 … QA-063 and the QA-054 links

| Case | Expected | Opens | Result |
|---|---|---|---|
| QA-061 `window.open` 1.5 s after load, open the project | 0 | 0 (`blocked a new window`) | **Pass** |
| QA-061 same, preview reload ×2 | 0 | 0 | **Pass** |
| QA-061 same, entering comment mode (reloads the page) | 0 | 0 | **Pass** |
| QA-061 `window.open` with a new URL every 200 ms ×10, one at 4 s; clicked a button meanwhile; then reload | 0 | 0 (22 blocked: 11 per load) | **Pass** |
| QA-062 `srcdoc` iframe shows in the Studio preview | content visible | visible, no `about:srcdoc` block in `daemon.log` | **Pass** ([04](desktop-beta23/04-qa062-srcdoc-shows.png)) |
| QA-062 same, project thumbnail | content visible | "G link in a nested iframe" shows | **Pass** |
| QA-063 ⌘-click A (`_blank`) | 1 | 1 | **Pass** |
| ⌘-click E (same-frame web link) | 1 | 1 | **Pass** |
| A `_blank` plain click | 1 | 1 | Pass |
| B page cancels on the link; C page cancels on `window` | 0 | 0 | Pass |
| D `mailto:` `_blank`; E same-frame plain click | 0 | 0 | Pass |
| F Unicode href | 1, encoded | 1 (`ti%E1%BA%BFng-vi%E1%BB%87t?q=%C4%91%C3%A0%20n%E1%BA%B5ng&e=%F0%9F%8E%89`) | Pass |
| H script redirect on click | 0 | 0 (refused, logged) | Pass |
| I `window.open` in a click handler | 1 | 1 | Pass |
| G `_blank` link inside the `srcdoc` iframe | 1 | 0 | **Fail** (QA-065) |
| A and I in comment mode | 0, picks the element | 0, comment box on `<a> I window.open on click` | Pass ([05](desktop-beta23/05-comment-mode-pick.png)) |

### macOS window: §18

| §18 item | Expected | Opens | Result |
|---|---|---|---|
| on_new_window always Deny | no second app window; web address to the browser | only the main window ever listed; every new-window request logged `blocked a new window`; web addresses reached the opener | **Pass** |
| 1 click = 1 tab: J opens a different URL on `pointerup`, `mouseup` and `click`, clicked 3 times | 1 each | 1, 1, 1 (`J-pointerup`) | **Pass** |
| Enter on K (`window.open` on `keydown` and `keyup`), twice | 1 each | 1, 1 (`K-keydown-1`) | **Pass** |
| Held Enter on K (8 autorepeats) | 1 | 1 | **Pass** |
| Space on K | 1 | 1 | **Pass** |
| A `window.open` on load or a timer | 0 | 0 | **Pass** (QA-061 rows) |
| Tab out of L (last field) to the app, then Shift+Tab back into L (`window.open` on `keyup`) | 0 | 0; focus went to Files "+" and back to L | **Pass** ([06](desktop-beta23/06-shift-tab-back.png)) |
| Typing a key in L (control: the field's own press) | 1 | 1 (`L-keyup-e`) | Pass |
| Comment mode: click a button with `window.open` | 0, picks it | 0, picks it | **Pass** |
| M `if (confirm('Open?')) window.open(…)`, OK | a dialog; OK opens 1 | no dialog, `confirm` returns `false`, 0 | **Fail** (QA-064, [07](desktop-beta23/07-qa064-confirm-no-dialog.png)) |
| N `alert('…')` then `window.open` | a dialog; then 1 | no dialog; 1 | **Fail** (QA-064); the tab count passes |
| O `prompt('…')` then `window.open` | a dialog; then 1 | no dialog, `prompt` returns `null`; 1 (`O-prompt-null`) | **Fail** (QA-064); the tab count passes |
| After M: let go of a key in L (keyUp only) | 0 | 0; the page saw the `keyup` (`Lku:b`) | **Pass** |
| Enter in P (`alert` on `keydown`), then a lone keyup in L | 0 | 0 (`Lku:c`) | **Pass** (no dialog: QA-064) |
| Drag Q (`draggable`) onto the drop zone, then a lone keyup in L | drop works; 0 | "dropped Q"; 0 (`Lku:d`) | **Pass** ([08](desktop-beta23/08-stray-keyups-drag.png)) |

Every line the app handed to the browser is in
[opens.txt](desktop-beta23/opens.txt). Three `…/self` lines at
`1794107607`–`1794107638` come from a test-tool fault, not the app: an
earlier synthetic ⌘ event left ⌘ held system-wide, so "plain" clicks were
⌘-clicks. [cg.swift](desktop-beta23/cg.swift) now clears the modifiers after
every action; the rerun above has E plain at 0.

### Not run

| Check | Why |
|---|---|
| §18 on Windows (WebView2) and Linux (WebKitGTK) | no such machine in this pass |
| QA-064 on Windows and Linux | same; WebView2 and WebKitGTK draw their own dialogs by default, so likely macOS only, not checked |

## QA-064 🟠 A design's `alert`, `confirm` and `prompt` never show in the macOS app

- **Steps**: a design with a button whose `click` runs
  `if (confirm('Open?')) window.open(url)` ([sec18.html](desktop-beta23/sec18.html), M);
  open it in the Studio of the desktop app; click the button.
- **Expected**: a dialog "Open?"; OK opens the page. The Studio frame has
  `sandbox="allow-scripts allow-forms allow-popups allow-modals"`
  (`crates/od-ui/src/app/studio.rs:1897`), and the same Studio in a browser
  shows all three dialogs: [dialogs.mjs](desktop-beta23/dialogs.mjs) on the
  copy's daemon records `confirm:Open?`, `alert:About to open`,
  `prompt:Name?` in both WebKit and Chromium.
- **Actual**: no dialog at all. `confirm` returns `false` at once (the page
  logs `M-cancel`), `alert` returns without showing, `prompt` returns
  `null` (the page opened `…/O-prompt-null`). A design's delete button
  behind `confirm` never deletes; a form that asks a name with `prompt`
  gets nothing; the user sees no message and no error.
- **Cause**: wry 0.55.1's `WKUIDelegate` (`wry_web_view_ui_delegate.rs`)
  implements only the open panel, media capture and `createWebView`; it
  has no `webView:runJavaScriptAlertPanelWithMessage:…`,
  `…ConfirmPanel…` or `…TextInputPanel…`, and WKWebView without them
  answers as above. The app's own UI uses no JS dialog, so only designs
  are hit.
- **Fix**: give the webview's UI delegate the three methods (an `NSAlert`
  sheet on the window, with an input field for `prompt`), by adding them
  to the `WryWebViewUIDelegate` class at startup (`class_addMethod`) or by
  patching wry; or upstream to wry. Then rerun §18's dialog lines, which
  this bug blocks.
- **Test**: §18 dialog rows above (opener log + page log); reference in
  [dialogs.mjs](desktop-beta23/dialogs.mjs).
- **Status**: fixed on branch `js-dialogs`: at startup the shell adds the
  three methods to the webview's UI delegate class (`class_addMethod`,
  `apps/desktop/src/js_dialogs.rs`) and sets the delegate again, since
  WebKit reads a delegate's methods when it is set. Each shows an app-modal
  `NSAlert` (not a sheet: the completion handler is then called once, before
  the method returns) titled with the asking frame's origin (`127.0.0.1:5173
  says`; a design's sandboxed frame has an opaque origin: "This page says"),
  OK and Cancel, a text field holding the default text for `prompt`.
  Windows (WebView2) and Linux (WebKitGTK) draw these dialogs themselves,
  as neither wry nor Tauri handles `ScriptDialogOpening` / `script-dialog`.
  From the second dialog within 3 s of the last, the alert offers "Don't
  allow this page to show more dialogs"; ticked, the webview's dialogs
  return at once (as cancelled) until its next page load, so a page's
  `alert` loop cannot hold the app.
  Not yet rerun on a built app: §18's dialog rows.

## QA-065 🔵 A `_blank` link inside a design's `srcdoc` iframe does nothing in the app

- **Steps**: [qa054-links.html](desktop-beta22/qa054-links.html) case G,
  `<iframe srcdoc='<a href="http://…/inner" target="_blank">…</a>'>`;
  click the link in the Studio preview.
- **Expected**: the page opens in the browser, as link A does (the same
  link outside the nested frame).
- **Actual**: nothing opens; `daemon.log`:
  `blocked a navigation away from the app url=http://127.0.0.1:58990/inner`.
- **Cause**: the bridge runs in the design's frame only; the nested
  `srcdoc` frame has its own window, so no `od:open-link` is posted, and
  the native side refuses the navigation (by design since QA-061). Noted
  under QA-062's status in [bugs-desktop-beta22.md](bugs-desktop-beta22.md)
  without its own ID; filed now so it is tracked.
- **Fix**: needs a decision, not a one-liner. The nested frame inherits
  the design frame's sandbox (no `allow-same-origin`), so the bridge cannot
  reach its document, and handing the bridge's key to a script inside it
  would hand it to the page. One way: the bridge rewrites each `srcdoc`
  to add a small unkeyed relay that posts the href of a trusted `_blank`
  click to the parent, and the bridge opens it only when the nested frame
  had focus at that moment. Or accept it as a limit and say so in the docs.
- **Test**: links table above, row G.

## Still to do

- §18 on Windows (WebView2) and Linux (WebKitGTK), with the same pages.
- After QA-064 is fixed: §18's confirm/alert/prompt rows again (OK opens
  one tab; Cancel opens none; Enter closes the alert from P, then a lone
  keyup in L opens nothing).

## Not bugs (checked)

- "Worked for 0:00 · 2 steps" after a plan turn: the mock agent answers
  the plan prompt at once; a real run shows its length.
