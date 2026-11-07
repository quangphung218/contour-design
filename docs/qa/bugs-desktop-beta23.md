# QA bugs: desktop app 0.2.0-beta.23

Recheck of QA-055 and QA-061 … QA-063 ([bugs-desktop-beta21.md](bugs-desktop-beta21.md),
[bugs-desktop-beta22.md](bugs-desktop-beta22.md)), the Next chips after a
run (#106), and the desktop checklist in [test-plan.md §18](test-plan.md),
on the release asset `Contour_0.2.0-beta.23_universal.app.tar.gz`
(macOS 26, Apple silicon), 2026-11-08.

**Partial run.** The Mac's screen was locked for the whole session
(`CGSSessionScreenIsLocked = 1`, checked at 05:12 and again after a
9-minute wait to 05:23): window screenshots come out black and CoreGraphics
clicks do not reach the window. Everything that needs the real window
(QA-061, QA-062, QA-063 and §18) is **not run** and needs a second pass
with the screen unlocked. What runs on the app's in-process daemon (the
window's UI bundle and API, driven in WebKit, the window's engine) was run.

No new bugs. Numbering for this release starts at QA-064.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 0 | |
| 🟠 | 0 | |
| 🟡 | 0 | |
| 🔵 | 0 | |
| **Total** | **0** | |

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

## Results

| Check | Expected | Result |
|---|---|---|
| QA-055 Templates ▸ App feature tour (9:16) ▸ Use template | Motion bar shows 9:16 and 12s | **Pass** ([01](desktop-beta23/01-qa055-use-template.png)) |
| QA-055 then Create | project metadata `videoAspect 9:16`, `videoLength 12` | **Pass** (`{"kind":"video","videoAspect":"9:16","videoLength":12}`) |
| `/api/design-templates` film data | each Motion film carries its size and length | **Pass** (31 of 185 templates; App feature tour 1080×1920, 12 s) |
| Next chips after a run, 1360×810, two turns, 5 projects | chips inside the chat, no "Jump to latest" | **Pass 5/5** ([02](desktop-beta23/02-next-chips-1360.png)) |
| Next chips after a run, 390×844, 5 projects | same | **Pass 5/5** ([03](desktop-beta23/03-next-chips-390.png)) |
| Preview bridge alone (`bridge.e2e.mjs`), Chromium and WebKit | all cases | **Pass** (C0, T1, K1, N1–N3, R1–R2, G, D1–D3 open one each; G2, S3, TP, FK, TF, TS, SW, P1, X1–X3, Q2 open none) |
| QA-061 `window.open` on load or timer opens nothing | 0 opens | **Not run** (screen locked) |
| QA-062 `srcdoc` iframe shows in the Studio preview | content visible, no `blocked a navigation … about:srcdoc` | **Not run** (screen locked) |
| QA-063 ⌘-click on a `_blank` link | 1 open | **Not run** (screen locked) |
| §18 on_new_window always Deny (macOS) | no second window; web address to the browser | **Not run** (screen locked) |
| §18 1 click = 1 tab (pointerup/mouseup/click; Enter keydown/keyup; held key) (macOS) | 1 open each | **Not run** in the window; bridge part passes in WebKit (above) |
| §18 Tab out / Shift+Tab back, comment mode, confirm/alert/prompt, drag and drop (macOS) | as in §18 | **Not run** (screen locked) |
| §18 on Windows (WebView2) and Linux (WebKitGTK) | | **Not run** (no such machine in this pass) |

The beta.23 source has the three fixes the native checks would exercise:
`allowed_navigation` lets `about:srcdoc` and `about:blank` through
(`apps/desktop/src/main.rs:487`), `on_new_window` always answers `Deny`
(`apps/desktop/src/main.rs:990`), and the bridge counts ⌘ and Ctrl clicks
as a new tab (`bridges/od-bridge.js:515`). Reading the code is not a run;
they stay "Not run" until the window pass.

## Still to do

Rerun with the screen unlocked, same copy and data dir: the
[qa054-links.html](desktop-beta22/qa054-links.html) page for QA-061 … QA-063
(opener log and `daemon.log`), then each §18 item with CoreGraphics events
([cg.swift](desktop-beta22/cg.swift), [cc.sh](desktop-beta22/cc.sh)), only
while the copy's pid is frontmost.

## Not bugs (checked)

- "Worked for 0:00 · 2 steps" after a plan turn: the mock agent answers
  the plan prompt at once; a real run shows its length.
