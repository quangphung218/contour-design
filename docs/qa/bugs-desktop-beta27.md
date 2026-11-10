# QA bugs: desktop app 0.2.0-beta.27

Recheck of the beta.27 fixes (QA-072, a page's dialog when its web process
crashes, links and presses in a design) and the crash-loop row never run
before, on the release asset `Contour_0.2.0-beta.27_universal.app.tar.gz`
(macOS 26, Apple silicon, 1440×900 points display), 2026-11-10. The screen
was checked unlocked first (`CGSessionCopyCurrentDictionary`:
`kCGSSessionOnConsoleKey = 1`, no `CGSSessionScreenIsLocked`).

**macOS: one new blocker. A design's web content process that crashes twice
within 10 s makes the whole app panic and quit (QA-073, 🔴), with or without
the `OD_QA_CRASH_WEB` hook.** Everything else asked for passes: QA-072 is
fixed, a dialog or file chooser open at a crash closes by itself, the
QA-070 rows still pass, and every link and label case opens exactly one tab,
none after a timer flood.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 1 | QA-073 |
| 🟠 | 0 | |
| 🟡 | 0 | |
| 🔵 | 0 | |
| **Total** | **1** | |

## How it was run

As on beta.26 ([How it was run](bugs-desktop-beta26.md#how-it-was-run)):
the release asset renamed `ContourQA.app`, bundle id
`dev.contour.desktop.qa`, ad-hoc signed, its own `OD_DATA_DIR`, the opener
log as `OD_OPEN_COMMAND`, the mock agent on `PATH`. Only that copy's pids
were driven, quit or killed. `/api/version`: `0.2.0-beta.27`, `macos`,
`aarch64`. For the crash rows the binary ran directly, to keep its log:

```sh
env RUST_BACKTRACE=1 [OD_QA_CRASH_WEB=1] OD_DATA_DIR=<tmp>/data \
  OD_OPEN_COMMAND=<qa>/opener.sh PATH=<repo>/crates/od-ui/e2e/mock-agent:/usr/bin:/bin \
  <tmp>/ContourQA.app/Contents/MacOS/od-desktop 2> <tmp>/run.log
```

Tools: [go.sh](desktop-beta24/go.sh), [step.sh](desktop-beta24/step.sh),
[cg.swift](desktop-beta24/cg.swift), [mkproj.sh](desktop-beta24/mkproj.sh),
[busywait.sh](desktop-beta26/busywait.sh). Pages:
[dialogs25.html](desktop-beta25/dialogs25.html),
[crash27.html](desktop-beta27/crash27.html) (alert, confirm, prompt, file
input, SPIN 8 s), [links27.html](desktop-beta27/links27.html) (link,
⌘-click, labels, Enter, timer flood and a field whose any keyup opens).

**A crash of only the copy's web content process**:
[crashdlg.sh](desktop-beta27/crashdlg.sh) and
[crashloop.sh](desktop-beta27/crashloop.sh) (copy them next to `go.sh` and
a built `cg`). Each first presses SPIN 8 s in `crash27.html` and takes the
one `com.apple.WebKit.WebContent` process at ~100 % CPU; it refuses unless
exactly one is, and never a pid that predates the copy. Then `kill -9` that
pid, with a dialog up (`crashdlg.sh LABEL X Y`) or twice within 10 s
(`crashloop.sh`). This is what beta.26 lacked (Not run).

## Results

### QA-072: "This page isn't responding" after Wait

| Check | Expected | Result |
|---|---|---|
| BUSY 20 s, ⌘R, Wait (`busywait.sh`) | asks again while busy; closes by itself once the page reloads | **Pass** 2 of 2: ⌘R +1.8 s, question +3.0, Wait +3.5, asks again +8.7 ([01](desktop-beta27/01-qa072-reask.png)), closed +20.2 as BUSY ended; page reloaded (`loaded` new time) |

### A page's dialog when its web process crashes

| Check | Expected | Result |
|---|---|---|
| `alert` up, process killed | dialog closes, page reloads, app answers | **Pass**: closed at the first look after the kill (< 0.5 s) ([02](desktop-beta27/02-alert-open.png)) |
| `confirm` up | the same | **Pass** |
| `prompt` up | the same | **Pass** |
| `<input type=file>` panel ("Open") up | the same | **Pass**: panel gone 0.8 s after the kill, page reloaded ([03](desktop-beta27/03-file-after.png)) |
| then A1 `alert` | dialog shows, Return closes it | **Pass** |

### QA-070 regression

| Check | Expected | Result |
|---|---|---|
| INF `for(;;) alert('x')`, OK, tick, OK | preview stopped, says it kept opening dialogs | **Pass** within 5 s ([04](desktop-beta27/04-inf-stopped.png)) |
| … Run again | design runs | **Pass** (`loaded` new time) |
| HANG `for(;;){}`, ⌘R, Reload | preview stopped, says it stopped responding | **Pass** ([05](desktop-beta27/05-hang-stopped.png)) |

### Links and presses in a design

Opens counted by `step.sh` in the opener log ([opens.txt](desktop-beta27/opens.txt)).

| Check | Expected | Result |
|---|---|---|
| LK click on a `_blank` link | 1 tab | **Pass** (+1) |
| CM ⌘-click on a plain link | 1 | **Pass** (+1) |
| C0 click, handler `window.open` | 1 | **Pass** (+1) |
| L1 click a label around a checkbox (checkbox's click opens) | 1 | **Pass** (+1 `l1-open`, box ticked) |
| L2 click `<label for>` a button (button's click opens) | 1 | **Pass** (+1 `l2-open`) |
| L3 label and its checkbox both open | 1, the label's | **Pass** (+1 `l3-label`) |
| EN click a field, Tab to a `_blank` link, Enter | 1 | **Pass** (+0, +0, +1) |
| FM pointerdown floods 1500 timers, then a key let go (keyup only) in a field whose keyup opens | 0 | **Pass** 2 of 2 (+0; the handler ran: `stray keyup x`) |
| FX Enter floods and alerts; Return closes it; then a keyup | 0 | **Pass** (+0; [06](desktop-beta27/06-fx.png)) |
| control: a whole key press in that field after the flood | 1 | **Pass** (+1 `stray`) |

### Crash loop (two crashes within 10 s)

| Check | Expected | Result |
|---|---|---|
| `OD_QA_CRASH_WEB=1`, open a design, ⌘R | log names the hook; first crash reloads, second opens `?preview=stopped&why=crashed`, Studio says it crashed | **Fail** (QA-073): log `ended … after=Reload`, then `after=StopPreview`, then the app panics and quits (exit 101), 2 of 2 |
| route change into a design with the hook on | no crash | **Pass** |
| real crash: `crashloop.sh` (two `kill -9` 5 s apart, no hook) | the same as the first row | **Fail** (QA-073): same panic |

### Not run

| Check | Why |
|---|---|
| Multi-touch (two or three fingers, one tab) | no touch device on this Mac, and CoreGraphics cannot post touch events; covered only by `bridge.e2e.mjs` M2, MS, TX in Chromium |
| Windows and Linux | no such machine in this pass |

## QA-073 🔴 A design that crashes its page twice in 10 s makes the app panic and quit

- **Steps**: QA copy, open a design, end the window's web content process
  twice within 10 s: `OD_QA_CRASH_WEB=1` and ⌘R, or `kill -9` of the copy's
  own web content process twice
  ([crashloop.sh](desktop-beta27/crashloop.sh)).
- **Expected**: the first crash reloads the page; the second opens the
  project with the preview stopped and "This design crashed the page twice
  in a row" (QA-067, QA-071).
- **Actual**: the first crash reloads; the second panics the app on the
  main thread and it quits, every window gone, 3 of 3 (2 with the hook, 1
  real crash). A design that crashes its page on load therefore takes the
  whole app down each time it is opened, which is what the crash-loop stop
  is for.
  ([qa073-panic.txt](desktop-beta27/qa073-panic.txt)):

  ```text
  WARN od_desktop: the web content process ended by itself after=StopPreview
  thread 'main' panicked at wry-0.55.1/src/wkwebview/mod.rs:1349:40:
  called `Option::unwrap()` on a `None` value
     4: wry::wkwebview::url_from_webview
     7: <tauri::webview::Webview>::url
     8: od_desktop::page_terminated
  ```

- **Cause**: after its web content process ends, the `WKWebView`'s `URL`
  is nil; wry's `url_from_webview` unwraps it instead of returning `Err`.
  `page_terminated` (`apps/desktop/src/main.rs`, `AfterCrash::StopPreview`
  arm) calls `window.url()` there, so the `Err` arm it has never runs. The
  `Reload` arm does not read the URL, so the first crash survives.
- **Fix**: keep the last page the main window loaded (`on_page_load`'s
  `payload.url()`, already read for `qa_crash_web`) in managed state and
  build `stopped_preview_url` from that; never call `window.url()` once the
  process ended. Same pattern, to check or fix in the same PR:
  `stop_hung_page` (`window.url()` before `restart_page`), `check_reload`'s
  `NotAnswered::Restart` arm and `reload_unheard`: each expects an `Err`
  that wry never returns on a nil URL. These ran on a live page in this
  pass and did not panic.
- **Test**: a unit test can't reach WKWebView; an e2e row on the QA copy:
  `crashloop.sh` (or the hook and ⌘R), expect the app alive and the Studio
  showing the "crashed" reason. Since beta.25 (crash loop added with
  QA-067); never run before on the app.

## Still to do

- Rerun the crash-loop row once QA-073 is fixed; then a dialog open during
  the second crash.
- Multi-touch on a touch device.
- §18 and QA-064 on Windows (WebView2) and Linux (WebKitGTK).
