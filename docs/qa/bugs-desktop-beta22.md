# QA bugs: desktop app 0.2.0-beta.22

Recheck of QA-053 … QA-060 ([bugs-desktop-beta21.md](bugs-desktop-beta21.md))
on the installed `/Applications/Contour.app` 0.2.0-beta.22 (macOS 26, Apple
silicon), with a close look at `target="_blank"` links in the design preview
(QA-054). All 8 are verified; the results are written under each bug there.
Three new bugs came up around QA-054, all older than beta.22 (the code is the
QA-044 fix, 82067a9a). Numbering goes on from QA-060.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 0 | |
| 🟠 | 2 | QA-061, QA-062 |
| 🟡 | 0 | |
| 🔵 | 1 | QA-063 |
| **Total** | **3** | |

## How it was run

Never on the owner's app or data. A copy of the installed bundle with its
own bundle id, its own data dir, the e2e mock `claude` first on `PATH`, and
`OD_OPEN_COMMAND` pointing at [opener.sh](desktop-beta22/opener.sh), which
logs each URL the app hands to the browser to `<data>/opens.log` instead of
opening it:

```sh
ditto /Applications/Contour.app <tmp>/ContourQA.app
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier dev.contour.desktop.qa" <tmp>/ContourQA.app/Contents/Info.plist
codesign --force --deep -s - <tmp>/ContourQA.app
open -n --env OD_DATA_DIR=<tmp>/data \
  --env OD_OPEN_COMMAND=<repo>/docs/qa/desktop-beta22/opener.sh \
  --env PATH=<repo>/crates/od-ui/e2e/mock-agent:/usr/bin:/bin:/usr/sbin:/sbin \
  -a <tmp>/ContourQA.app
```

Link targets point at a local `python3 -m http.server 58990`, so the real
browser's requests can be counted too. The link page is
[qa054-links.html](desktop-beta22/qa054-links.html), written into a project
with `POST /api/projects/<id>/files`. The last round ran the copy without
`OD_OPEN_COMMAND`, so `open` reached Chrome, and the server log counted the
page requests per click.

The window was clicked with CoreGraphics events
([cg.swift](desktop-beta22/cg.swift), [cc.sh](desktop-beta22/cc.sh)) only
while the copy's pid was frontmost and its window the topmost Contour
window. The comment and template fixes were also checked with Playwright
against the copy's in-process daemon:

```sh
BASE=http://127.0.0.1:<port> OUT=<dir> PROJ054=<project id> \
  node docs/qa/desktop-beta22/recheck.mjs
```

## QA-054 link results

Each case clicked once in the Studio preview (not comment mode) unless
noted; "opens" is the count in the opener log.

| Case | Expected | Opens | Result |
|---|---|---|---|
| A `_blank` http link, plain click | 1 | 1 | Pass |
| A, real browser (Chrome), 3 clicks seconds apart | 3 tabs | 3 page requests | Pass |
| A, two clicks 0.7 s apart | 1 or 2 | 1 | Pass (same-page guard, 1 s) |
| A, Tab to it and Enter | 1 | 1 | Pass |
| B page `preventDefault` on the link | 0 | 0 | Pass |
| C page `preventDefault` on `window`, added after the bridge | 0 | 0 | Pass |
| D `mailto:` with `_blank` | 0 | 0 | Pass |
| E same-frame web link | 0 | 0 | Pass (refused, logged) |
| F Unicode href (`tiếng-việt?q=đà nẵng&e=🎉`) | 1, encoded | 1 | Pass |
| H script redirect on click; redirect 3 s after load | 0 | 0 | Pass (refused, logged) |
| Embedded iframe from another origin | does not open | 0 | Pass (CSP `default-src 'self'` blocks it; on purpose) |
| I `window.open` in a click handler | 1 | 1 | Pass |
| `window.open` 1.5 s after load, no click | 0 | 1 on each load and reload | **Fail** (QA-061) |
| G `_blank` link inside a `srcdoc` iframe | 1 | not clickable | **Fail** (QA-062) |
| A with ⌘ held | 1 | 0 | **Fail** (QA-063) |
| A in comment mode | 0, picks the `<a>` | 0 | Pass ([03](desktop-beta22/03-comment-mode-link-picks.png)) |

## QA-061 🟠 A design's `window.open` opens the browser with no click

- **Steps**: a page with
  `setTimeout(() => window.open('http://127.0.0.1:58990/winopen-load'), 1500)`;
  open its project in the Studio; or press the preview's reload.
- **Expected**: nothing opens. A browser blocks a popup the user did not
  ask for (the web UI's Chromium does), and the QA-054 fix only lets a real
  click through.
- **Actual**: the system browser gets the page on every load and reload of
  the preview, entering comment mode included (it reloads the page). A page
  that calls `window.open` with a new URL every 200 ms opened the browser
  10 times in 2 s; the same-page guard (1 s) does not stop different URLs.
- **Cause**: `on_new_window` (`apps/desktop/src/main.rs:982`) hands any
  http(s) URL to `LastOpened::open`, whoever asked. The preview frame's own
  `window.open` reaches it like the app's.
- **Fix**: open from `on_new_window` only when the request comes from the
  app's own document (QA-044's links), and let design links go through the
  bridge's keyed `od:open-link`, which already requires a trusted click. If
  a design's `window.open` in a click handler (case I) should keep working,
  have the bridge wrap `window.open` and post `od:open-link` only while
  `navigator.userActivation.isActive`.
- **Evidence**: opener log, one line per load:
  `http://127.0.0.1:58990/winopen-load`; ten lines
  `loop-1` … `loop-10` within 1.9 s for the loop page.
- **Status**: fixed in #98 (the window opens no new window itself; the
  app's links go through `open_link`, a design's `window.open` only from its
  handler of a real click or key, via the bridge).

## QA-062 🟠 A `srcdoc` iframe in a design is blank in the desktop app

- **Steps**: a page with
  `<iframe srcdoc='<a href="…" target="_blank">G link in a nested iframe</a>'>`;
  open it in the Studio.
- **Expected**: the iframe shows its content, as in the web UI
  ([14](desktop-beta22/14-srcdoc-web-ui.png)) and in a browser.
- **Actual**: the iframe is empty
  ([01](desktop-beta22/01-qa054-test-page.png), the dashed box under F).
  `daemon.log`: `WARN od_desktop: blocked a navigation away from the app
  url=about:srcdoc`, once per load. Same for the project thumbnail.
- **Cause**: `allowed_navigation` (`apps/desktop/src/main.rs:483`) allows the
  daemon's origin and the splash only; `about:srcdoc` has neither, so the
  frame's first load is refused.
- **Fix**: let `about:srcdoc` and `about:blank` through `on_navigation`
  (they never leave the app; the preview's CSP still applies to them).
- **Status**: fixed in #98. A `_blank` link inside the srcdoc frame (case
  G) shows but does not open: the sandboxed nested frame has its own
  origin, out of the bridge's reach.

## QA-063 🔵 ⌘-click on a `_blank` link in the preview does nothing

- **Steps**: hold ⌘ and click link A (`target="_blank"`, http).
- **Expected**: the page opens in the browser, as a plain click does (in a
  browser ⌘-click opens a background tab).
- **Actual**: nothing; `daemon.log`: `blocked a navigation away from the app
  url=http://127.0.0.1:58990/plain`.
- **Cause**: the bridge skips clicks with a modifier
  (`bridges/od-bridge.js:503`, `newWindowLink`), so no `od:open-link` is
  posted, and the native side refuses the navigation.
- **Fix**: accept ⌘ (and Ctrl on Windows/Linux) clicks on web links,
  `_blank` or not, in `newWindowLink`.
- **Status**: fixed in #98.

## Not bugs (checked)

- An iframe from another origin in a design stays empty: the preview's CSP
  (`default-src 'self' data: blob:`) blocks it, so a design asks nothing off
  loopback. Not new.
- Two clicks 0.7 s apart on the same link open one tab: the shell's
  same-page guard (1 s) that keeps Windows and Linux at one tab per click.
  A user who wants a second tab clicks again after a second.
- Chrome tabs could not be counted with AppleScript (it lists only one
  window of the running Chrome); the per-click page requests on the local
  server were counted instead.
