# QA bugs: desktop app 0.2.0-beta.26

Recheck of QA-070 and QA-071 ([bugs-desktop-beta25.md](bugs-desktop-beta25.md))
and a quick rerun of QA-064 and QA-065, on the release asset
`Contour_0.2.0-beta.26_universal.app.tar.gz` (macOS 26, Apple silicon,
1440×900 points display, Dock at the bottom), 2026-11-10. The screen was
checked unlocked first (`CGSessionCopyCurrentDictionary`: no
`CGSSessionScreenIsLocked`, `kCGSSessionOnConsoleKey = 1`).

**macOS: QA-070 and QA-071 fixed; QA-064 and QA-065 still pass.** Both ways
into the stop (the `alert` loop, and Reload in "This page isn't
responding") now land in the Studio with the preview stopped, within 4 s,
with the right reason, and Run again runs the design. ⌘Q on a hung page
quits at once. One minor finding: after Wait, the question comes back while
the page is still busy and stays on screen after the page has recovered
(QA-072, 🔵).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 0 | |
| 🟠 | 0 | |
| 🟡 | 0 | |
| 🔵 | 1 | QA-072 |
| **Total** | **1** | |

## How it was run

As on beta.25 ([How it was run](bugs-desktop-beta25.md#how-it-was-run)):
the release asset renamed `ContourQA.app`, bundle id
`dev.contour.desktop.qa`, ad-hoc signed, its own `OD_DATA_DIR`, the opener
log as `OD_OPEN_COMMAND`, the mock agent on `PATH`. Only that copy's pid was
driven or quit; the owner's Contour was not touched. `/api/version`:
`0.2.0-beta.26`, `macos`, `aarch64`.

Tools: [go.sh](desktop-beta24/go.sh), [step.sh](desktop-beta24/step.sh),
[cg.swift](desktop-beta24/cg.swift), [mkproj.sh](desktop-beta24/mkproj.sh).
Pages: [dialogs25.html](desktop-beta25/dialogs25.html),
[nested.html](desktop-beta24/nested.html). QA-072's timeline:
[busywait.sh](desktop-beta26/busywait.sh) (next to `go.sh` and a built
`cg`: `PID=<pid> busywait.sh`). URLs
handed to the browser: [opens.txt](desktop-beta26/opens.txt).

## Results

### QA-070 and QA-071: a design that hangs the window

| Check | Expected | Result |
|---|---|---|
| INF `for(;;) alert('x')`, OK, tick "Don't allow…", OK | preview stopped, Studio says why | **Pass**: Studio within 4 s, "This design kept opening dialogs, so its preview was stopped." ([01](desktop-beta26/01-inf-preview-stopped.png)); log `a page is hung in a dialog loop: stopping it` |
| … Run again | design runs again; dialogs show again | **Pass** (`loaded` new time; `confirm` shows a dialog) |
| HANG `for(;;){}`, ⌘R | "This page isn't responding", Wait / Reload | **Pass** ([02](desktop-beta26/02-hang-not-responding.png)) |
| … Reload | preview stopped, says the page stopped answering (QA-071) | **Pass**: "This design stopped responding, so its preview was stopped." ([03](desktop-beta26/03-hang-reload-stopped.png)); log `the page did not answer Reload: restarting it` |
| BUSY 20 s, ⌘R, Wait | page reloads when it answers | **Pass** (reloaded as BUSY ended); but see QA-072 |
| edit typed in Edit element, not applied; ⌘R | "Leave this page?"; Leave reloads | **Pass** ([05](desktop-beta26/05-cmdr-unapplied-ask.png)) |
| HANG, ⌘Q | quits | **Pass** (pid gone within 0.5 s) |
| web process crashes twice within 10 s | preview stopped, "crashed" reason | not run (see Not run) |

### Quick rerun

| Check | Result |
|---|---|
| QA-064: `confirm` Escape → `false`; `prompt('Name?','x')` OK → `"x"`; `prompt('Tên?')` typed `Nguyễn Văn An ở Hà Nội` | **Pass** (`len=22 eq=true`, [06](desktop-beta26/06-qa064-log.png)) |
| QA-064: second dialog within 3 s has the checkbox; tick blocks | **Pass** (INF row) |
| QA-065: NL nested `_blank` 1, NC cancelled 0, NB 1, TOP 1 | **Pass** ([opens.txt](desktop-beta26/opens.txt)) |

### Not run

| Check | Why |
|---|---|
| Crash loop (two crashes in 10 s) | Still no safe way: the copy's web content process cannot be told apart from the owner's Contour by pid, and making the design crash it means passing WebKit's memory limit (~4 GB) twice on a shared 16 GB machine. |
| Windows and Linux | no such machine in this pass |

## QA-072 🔵 "This page isn't responding" asks again and stays up after the page recovers

- **Steps**: [dialogs25.html](desktop-beta25/dialogs25.html), BUSY 20s
  (a 20 s busy loop), ⌘R, Wait in "This page isn't responding".
- **Expected**: after Wait the page reloads when it answers, and no
  question is left on screen for a page that answers.
- **Actual**: 5 s after Wait the same question shows again
  (`RELOAD_ASK_AGAIN`, while BUSY still runs). BUSY ends 15 s later, the page
  reloads underneath and works, yet the question stays up saying the page
  isn't responding ([04](desktop-beta26/04-qa072-stale-not-responding.png),
  watched 45 s). Reload there does nothing (the page answered, so
  `after_not_answering` gives `Nothing`): no harm, but the dialog is wrong
  and Reload does not do what it says. 2 of 2.
  Timeline ([busywait.sh](desktop-beta26/busywait.sh)), seconds from BUSY:
  ⌘R +1.8, question +3.0, Wait +3.5, question again +8.7, page reloads
  ≈ +20, question still up at +48.
- **Fix**: when the page answers the Reload question (`ReloadAsk` closed for
  `token`), close the open alert (`NSApp.abortModal` / end the sheet) from
  the side that marks it answered; or keep the re-ask but close it the same
  way. `check_reload` in `apps/desktop/src/main.rs` shows the alert with
  `web_process::ask_not_responding()` and only reads `open` after the user
  answers.
- **Test**: QA-070/071 table above, row BUSY + Wait; run `busywait.sh` and
  expect no layer-8 window once the page has reloaded.

## Still to do

- Crash-loop row, once a test hook or a safe way to end only the copy's web
  content process exists.
- §18 and QA-064 on Windows (WebView2) and Linux (WebKitGTK).
