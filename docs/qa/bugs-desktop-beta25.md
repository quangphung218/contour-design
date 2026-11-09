# QA bugs: desktop app 0.2.0-beta.25

Recheck of QA-066 … QA-069 ([bugs-desktop-beta24.md](bugs-desktop-beta24.md))
and a quick rerun of the rows that passed on beta.24 (QA-064, QA-065, leaving
a screen mid-write), on the release asset
`Contour_0.2.0-beta.25_universal.app.tar.gz` (macOS 26, Apple silicon,
1440×900 points display, Dock at the bottom), 2026-11-10. The screen was
checked unlocked first (`CGSessionCopyCurrentDictionary`: no
`CGSSessionScreenIsLocked`, `kCGSSessionOnConsoleKey = 1`).

**macOS: QA-066, QA-068 and QA-069 fixed; QA-064, QA-065 and leaving a
screen still pass. QA-067 is not fixed: stopping a hung design now
deadlocks the app's main thread (QA-070, 🔴).** The window turns white for
good, and ⌘R, ⌘Q, the menu bar and even `SIGTERM` do nothing; only Force
Quit ends it. Both ways into the stop (the `alert` loop, and Reload in
"This page isn't responding") hit it, 2 of 2. The parts of QA-067 that do
not stop the page pass: ⌘Q on a hung page, ⌘R on a busy page (the
question, Wait), ⌘R with an edit not applied.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 | 1 | QA-070 |
| 🟠 | 0 | |
| 🟡 | 1 | QA-071 |
| 🔵 | 0 | |
| **Total** | **2** | |

## How it was run

As on beta.24 ([How it was run](bugs-desktop-beta24.md#how-it-was-run)):
the release asset renamed `ContourQA.app`, bundle id
`dev.contour.desktop.qa`, ad-hoc signed, its own `OD_DATA_DIR`, the opener
log as `OD_OPEN_COMMAND`, the mock agent on `PATH`. Only that copy's pid was
driven, quit or killed. `/api/version`: `0.2.0-beta.25`, `macos`, `aarch64`.

The window was driven with the beta.24 tools
([go.sh](desktop-beta24/go.sh), [step.sh](desktop-beta24/step.sh),
[cg.swift](desktop-beta24/cg.swift), [mkproj.sh](desktop-beta24/mkproj.sh)).
Pages: [dialogs25.html](desktop-beta25/dialogs25.html) (beta.24's
`dialogs.html` plus BUSY, a 20 s busy loop, and HANG, `for(;;){}`),
[design-b.html](desktop-beta24/design-b.html),
[nested.html](desktop-beta24/nested.html). In Playwright WebKit on the
copy's daemon (`cd crates/od-ui/e2e && npm ci` first):

```sh
BASE=http://127.0.0.1:<port> node docs/qa/desktop-beta24/leave.mjs
E2E_DIR=$PWD/crates/od-ui/e2e BASE=http://127.0.0.1:<port> PROJECT=<id> \
  SHOTS=<dir> node docs/qa/desktop-beta25/stopped.mjs
```

Output: [leave-result.json](desktop-beta25/leave-result.json); URLs handed
to the browser: [opens.txt](desktop-beta25/opens.txt). The hung app's main
thread was read with `sample <pid> 1`
([qa070-main-thread.txt](desktop-beta25/qa070-main-thread.txt)).

## Results

### QA-066: a long dialog keeps its buttons on screen

| Check | Expected | Result |
|---|---|---|
| 5000-character `alert` on a 900-point display | buttons on screen | **Pass**: alert 434 points tall (y 124–558), OK clicked with the mouse |
| the message | scrolls in a fixed box, first 2000 characters and `…` | **Pass** ([01](desktop-beta25/01-qa066-long-alert-end.png)) |
| long `confirm`, `prompt` | the same box | not run (no such button on the page) |

### QA-068: reload lifts the dialog block

| Check | Expected | Result |
|---|---|---|
| LOOP 40, tick, OK; `confirm` | blocked (`false`, 0–1 ms) | **Pass** (`C2=false 1ms`) |
| then the preview's reload button; `confirm` | dialog shows | **Pass** ([02](desktop-beta25/02-qa068-confirm-after-reload.png)) |
| block again, then the file changes (written through the API); `confirm` | dialog shows | **Pass** (page reloaded on its own, dialog shown) |

### QA-067: a design that hangs the window

| Check | Expected | Result |
|---|---|---|
| INF `for(;;) alert('x')`, tick, OK | page process ended, project opens again with the preview stopped | **Fail** (QA-070): window white for good ([03](desktop-beta25/03-qa070-blank-window.png)) |
| … then ⌘R, ⌘Q, Contour menu | a way out | **Fail** (QA-070): none answers ([04](desktop-beta25/04-qa070-menu-does-not-open.png)) |
| Studio with the preview stopped: notice and Run again | says why; Run again runs it | **Pass in WebKit only**, opened as `/projects/<id>?preview=stopped` ([07](desktop-beta25/07-preview-stopped.png)); the window never got there (QA-070); wording see QA-071 |
| ⌘R on a healthy page | reloads, no question | **Pass** |
| BUSY 20 s, ⌘R | "This page isn't responding", Wait / Reload | **Pass** ([05](desktop-beta25/05-not-responding.png)) |
| … Wait | page reloads when it answers | **Pass** (reloaded 20 s later, as BUSY ended) |
| HANG `for(;;){}`, ⌘R, Reload | page restarted with the preview stopped | **Fail** (QA-070): same deadlock (`check_reload → restart_page`) |
| HANG, ⌘Q | quits | **Pass** (at once) |
| edit typed in Edit element, not applied; ⌘R | "Leave this page?"; Leave reloads | **Pass** 2 of 2 ([06](desktop-beta25/06-cmdr-unapplied-edit.png)); see Not bugs |
| web process crashes twice within 10 s | preview stopped | not run (no safe way to crash only the copy's process) |

### QA-069: a new base URL drops the saved key

| Check | Expected | Result |
|---|---|---|
| Media ▸ Providers hint | says a new base URL drops the key | **Pass** |
| OpenAI key saved, then base URL `http://127.0.0.1:9/v1`, key empty, Save | no "✓ Saved"; says the key was removed | **Pass**: "Saved, but OpenAI needs its API key again." and "New base URL: the saved key was removed. Enter the API key again." under the field ([08](desktop-beta25/08-qa069-key-dropped-notice.png)) |
| Settings ▸ Media, key from the environment | the same; env key not reported | not run |

### Quick rerun of rows that passed on beta.24

| Check | Result |
|---|---|
| QA-064: `confirm` Escape → `false`; `prompt('Name?','x')` OK → `"x"`; `prompt('Tên?')` typed `Nguyễn Văn An ở Hà Nội` | **Pass** (`len=22 eq=true`, [09](desktop-beta25/09-qa064-log.png)) |
| QA-064: second dialog within 3 s has the checkbox; tick blocks | **Pass** |
| QA-065: NL nested `_blank` 1, NC cancelled 0, NB 1 (no `parent-go`), TOP 1 | **Pass** ([opens.txt](desktop-beta25/opens.txt)) |
| Leaving Studio / Media / bulk delete mid-write (`leave.mjs`, WebKit, 1 run) | **Pass**: 1 run POST, composer `""`; 1 task `done`; 4 of 4 deleted; no page error |

### Not run

| Check | Why |
|---|---|
| §18 and QA-064 on Windows and Linux | no such machine in this pass |
| QA-067 crash loop (two crashes in 10 s) | needs a crash of only the copy's web process |
| QA-069 in Settings ▸ Media | time |

## QA-070 🔴 Stopping a hung design deadlocks the app: white window, no way out but Force Quit

- **Steps**: open a design whose button runs `for (;;) alert('x')`
  ([dialogs25.html](desktop-beta25/dialogs25.html), INF). OK the first
  alert; tick "Don't allow this design to show more dialogs" on the second,
  OK. Same with HANG (`for(;;){}`), ⌘R, then Reload in "This page isn't
  responding".
- **Expected**: CHANGELOG beta.25: the app ends the page's web process,
  opens the same project again with the preview stopped and says so in the
  Studio, with Run again; ⌘Q quits.
- **Actual**: the log says `a page is hung in a dialog loop: stopping it`
  (or `the page did not answer Reload: restarting it`), then nothing more.
  The window turns white and stays white (watched 2 min 45 s,
  [03](desktop-beta25/03-qa070-blank-window.png)). ⌘R does nothing, ⌘Q
  does nothing (waited 15 s), the menu bar does not open
  ([04](desktop-beta25/04-qa070-menu-does-not-open.png)), System Events
  cannot read the menu bar, and `kill <pid>` (SIGTERM) is ignored; only
  `kill -9` (Force Quit) ends it. The in-process daemon still answers
  `/api/version`. 2 of 2: once from each way in. Worse than beta.24, where
  Contour ▸ Quit with the mouse still worked.
- **Cause**: a re-entrant lock on the main thread
  ([qa070-main-thread.txt](desktop-beta25/qa070-main-thread.txt), same in
  two samples 1 min apart): `restart_page` (`apps/desktop/src/main.rs:709`)
  calls `window.with_webview(… web_process::kill …)`. Tauri's
  `with_webview` holds `window_id.lock()` while it sends the message
  (`tauri-runtime-wry` 2.11.4, `lib.rs:1618`), and on the main thread the
  message runs at once. `_killWebContentProcessAndResetState` calls WebKit's
  `processDidTerminate` synchronously, so `on_page_terminated`
  (`main.rs:751`) runs inside that closure; its `app.run_on_main_thread`
  also runs at once on the main thread, and `page_terminated` then calls
  `window.navigate`, which takes `window_id.lock()` again
  (`lib.rs:1680`): `__psynch_mutexwait` for ever. `on_hung` already avoids
  this trap with a `thread::spawn` first (its comment: "on the main thread
  `run_on_main_thread` would run it at once"); `on_page_terminated` does
  not. The unit tests (`a_hung_page_restarts_on_its_studio_…`) do not run
  WebKit, so they miss it.
- **Fix**: in `on_page_terminated`, leave WebKit's call first, as `on_hung`
  does: `std::thread::spawn` then `run_on_main_thread(page_terminated)`.
  Siblings to check: every dispatcher call (`navigate`, `reload`, `url`,
  `with_webview`) reachable from inside a `with_webview` closure; today that
  is `restart_page` → kill (from `on_hung` and from `check_reload`). Also
  add a desktop smoke step that runs INF on the packaged app and expects the
  Studio's "Preview stopped".
- **Test**: QA-067 table above, rows INF and HANG + Reload.

## QA-071 🟡 "Not responding ▸ Reload" says the design "kept opening dialogs"

- **Steps**: a design runs `for(;;){}` (no dialog), ⌘R, Reload in "This
  page isn't responding".
- **Expected**: the Studio says the preview was stopped because the page
  stopped answering.
- **Actual**: both ways land on the same `?preview=stopped` address
  (`check_reload` and `stop_hung_page` both use `stopped_preview_url`), and
  the Studio has one text: "Preview stopped. This design kept opening
  dialogs, so its preview was stopped." ([07](desktop-beta25/07-preview-stopped.png)).
  For a busy loop with no dialog that is wrong, and the user looks for a
  dialog that never showed. Seen in WebKit with `?preview=stopped`; not seen
  in the window, where QA-070 stops it first.
- **Fix**: carry the reason in the address (`preview=stopped&why=hung` or
  `why=dialogs`) and give the busy case its own text, e.g. "This design
  stopped answering, so its preview was stopped.", in both languages
  (`PreviewStopped` in `i18n.rs`). Or one neutral text for both.
- **Test**: after QA-070, the HANG + Reload row, read the notice.

## Not bugs (checked)

- The first ⌘R with an edit not applied showed no question and did not
  reload; the next two did ask. Not reproduced; the first came right after
  typing Vietnamese with `cg type`, so likely the test tool.
- `cg type " sửa"` showed `sửà` in the field once: the test tool's
  key-by-key Unicode, not the app (the `prompt` row types the same way
  and gets the exact string).

## Still to do

- After QA-070: INF and HANG + Reload rows in the window, the crash-loop
  row, and QA-071's text.
- §18 and QA-064 on Windows (WebView2) and Linux (WebKitGTK).
