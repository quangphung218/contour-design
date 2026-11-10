# QA bugs: desktop app 0.2.0-beta.28

Recheck of QA-073 (a design that crashes its page twice in 10 s made the app
panic and quit) and the crash-loop rows on the release asset
`Contour_0.2.0-beta.28_universal.app.tar.gz` (macOS 26, Apple silicon,
1440×900 points display), 2026-11-10, time-boxed to 40 minutes. The screen
was checked unlocked first (`CGSessionCopyCurrentDictionary`:
`kCGSSessionOnConsoleKey = 1`, no `CGSSessionScreenIsLocked`).

**macOS: QA-073 is fixed. Every crash-loop row passes: the app stays up, the
preview stops and the Studio says "This design crashed the page twice in a
row, so its preview was stopped.", with the hook, with real `kill -9`, and
with a dialog open at the second crash.** QA-070 (INF and HANG) still
passes. No new bugs; no `panicked` line in either run's log.

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

As on beta.27 ([How it was run](bugs-desktop-beta27.md#how-it-was-run)):
the release asset renamed `ContourQA.app`, bundle id
`dev.contour.desktop.qa`, ad-hoc signed, its own `OD_DATA_DIR`, the opener
log as `OD_OPEN_COMMAND`, the mock agent on `PATH`, the binary run directly
to keep its log. Only that copy's pids were driven, quit (⌘Q) or killed; the
owner's Contour and its web content process were never touched.
`/api/version`: `0.2.0-beta.28`, `macos`, `aarch64`.

```sh
env RUST_BACKTRACE=1 [OD_QA_CRASH_WEB=1] OD_DATA_DIR=<tmp>/data \
  OD_OPEN_COMMAND=<qa>/opener.sh PATH=<repo>/crates/od-ui/e2e/mock-agent:/usr/bin:/bin \
  <tmp>/ContourQA.app/Contents/MacOS/od-desktop 2> <tmp>/run.log
```

Tools as on beta.27, plus in [desktop-beta28](desktop-beta28/):

- [crashloop.sh](desktop-beta28/crashloop.sh): beta.27's, but `$NOT` is now
  a required space-separated list of every WebContent pid that existed
  before the copy started (beta.27 defaulted to one hard-coded pid).
- [crash2dlg.sh](desktop-beta28/crash2dlg.sh) `LABEL X Y`: two real crashes
  within 10 s, the second while the dialog at window X,Y is up, on
  [crash28.html](desktop-beta28/crash28.html) (`crash27.html` plus S1, a
  1.2 s busy loop). beta.27's `crashdlg.sh` cannot do this: it waits out the
  8 s SPIN, so a second crash lands outside the 10 s window.

**Trap found while writing it**: after the first crash WebKit reloads the
page into its *prewarmed* web content process and spawns a new spare. The
newest WebContent pid is that spare, not the page; killing it does nothing
(the first attempt did exactly that: one `ended` line, the alert stayed).
`crash2dlg.sh` therefore finds the page's process again by pressing S1 and
taking the one pid that goes hot.

## Results

### Crash loop (two crashes within 10 s): QA-073 recheck

| Check | Expected | Result |
|---|---|---|
| (a) `OD_QA_CRASH_WEB=1`, open C28 (route change) | no crash | **Pass**: page `loaded`, no `ended` line |
| (a) … ⌘R | log names the hook; first crash reloads, second stops the preview; app alive; Studio says it crashed the page twice | **Pass** 2 of 2 (second after Run again): `ended … after=Reload`, 60 ms later `after=StopPreview`, app alive ([01](desktop-beta28/01-hook-crashed.png)) |
| (b) real crash: `crashloop.sh` (two `kill -9` 5 s apart, no hook) | the same | **Pass**: `after=Reload`, `after=StopPreview`, "app alive" ([02](desktop-beta28/02-kill-crashed.png)) |
| (c) `crash2dlg.sh c-alert 506 182`: second crash with A1 `alert` up | dialog closes, preview stops with the crashed reason, app alive | **Pass**: alert window gone ≤ 0.7 s after the kill, log `closing its panel` ([03](desktop-beta28/03-alert-open.png), [04](desktop-beta28/04-alert-after.png)) |
| (c) the same with C1 `confirm` up | the same | **Pass** (gone ≤ 0.7 s) |

### QA-070 regression

| Check | Expected | Result |
|---|---|---|
| INF `for(;;) alert('x')`, OK, tick "Don't allow this design to show more dialogs", OK | preview stopped, says it kept opening dialogs | **Pass** at once (log `a page is hung in a dialog loop: stopping it`, [05](desktop-beta28/05-inf-stopped.png)) |
| … Run again, A1 | design runs; dialog shows, Return closes it | **Pass** (`loaded` new time, `A1 back`) |
| HANG `for(;;){}`, ⌘R | "This page isn't responding", Wait / Reload | **Pass** (≤ 1 s) |
| … Reload | preview stopped, says it stopped responding | **Pass** (log `the page did not answer Reload: restarting it`, [06](desktop-beta28/06-hang-stopped.png)) |

### Not run

| Check | Why |
|---|---|
| `prompt` and file chooser up at the second crash | time box; `alert` and `confirm` share the panel code path that passed |
| QA-072, links and presses, multi-touch | out of this pass's scope (crash loop only) |
| Windows and Linux | no such machine in this pass |

## Still to do

- Multi-touch on a touch device.
- §18 and QA-064 on Windows (WebView2) and Linux (WebKitGTK).
