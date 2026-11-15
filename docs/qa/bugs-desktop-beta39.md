# QA bugs: desktop, beta.39 (single instance and the data lock)

QA pass on the installed beta.39 (`/Applications/Contour.app`) and a
release build of `main` at ad54d66d (beta.39) with its own identifier.
These are the [test-plan.md](test-plan.md) §22 rows that
[bugs-main36.md](bugs-main36.md), [bugs-main37.md](bugs-main37.md) and
[bugs-main39.md](bugs-main39.md) could not run because the screen was
locked. macOS 26, Apple silicon, 2026-11-16. The screen was unlocked and
nobody was at the Mac: `HIDIdleTime` was checked before each step and
stayed above 60 s.

**One minor bug; every single-instance and lock row passes.** A second
launch (`open -a`, `open -n`, the Dock's reopen) brings the running
window to the front within 0.5 s, whether it shows, is closed, minimised
or hidden. It does so with one process and one daemon. The lock dialogs
have the right words in English and Vietnamese, and their button quits
the app. One bug: the splash behind them is always in Vietnamese
(QA-104).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-104 | 🟡 | The start-up splash says "Đang mở Contour…" to every user. An English user sees it behind the English lock dialog, and alone for the 12 s wait of LK-04 | LK-01, LK-04 | `apps/desktop/static/index.html:40` is fixed text with `lang="vi"`; the page has no language logic | None: the other Vietnamese strings in `apps/desktop/src` are in tests |

## How it was run

The installed app (owner's, pid 8954) was never quit, killed or
restarted, and its data folder was not touched. On it, only a second
launch was made (SI-01).

The QA copy was built as in [bugs-main36.md](bugs-main36.md). Its
compiled identifier is `dev.contour.desktop.qa`, so its socket is
`/tmp/dev_contour_desktop_qa_si.sock`, not the owner's
`/tmp/dev_contour_desktop_si.sock`. It ran with its own `HOME`, from
[qa.sh](main36/qa.sh):

```sh
(cd crates/od-ui && trunk build --release)
cd apps/desktop && npx --yes @tauri-apps/cli@2.12.4 build --bundles app \
  --config '{"identifier":"dev.contour.desktop.qa","productName":"ContourQA"}'
cargo build --release -p od-cli --bin od
```

Tools in [desktop-beta39](desktop-beta39/). Copy them next to
`ContourQA.app`, `qa.sh`, `home/` and `mock/` (see main36):

- [cg.swift](desktop-beta39/cg.swift) is main36's, with two fixes:
  - `watch` turns the run loop, so `frontmostApplication` is no longer
    stale. main36's `usleep` loop kept reporting the app that was in
    front when it started.
  - Read-only commands no longer post a `flagsChanged` event. That event
    reset `HIDIdleTime`, so the "is someone at the Mac" guard read 13 s.
- [allwin.swift](desktop-beta39/allwin.swift) lists every on-screen
  window with its owner. The lock dialog is not the app's window (see the
  trap below).
- [si.sh](desktop-beta39/si.sh) `LABEL shown|closed|minimised|hidden`
  puts the copy's window in that state through Accessibility, with no HID
  event. It brings Finder to the front, makes a second launch (`open -n`)
  and watches for 6 s.
- [lk.sh](desktop-beta39/lk.sh) `LABEL LOCALE|-` holds the folder with
  `od daemon start --port 7499`, sets `locale` in `app-config.json` and
  opens the copy. It reads the dialog's words, takes a screenshot and
  presses the button. With `APP_HOLDER=1`, it first copies the od's URL
  into `daemon.lock.app`, so the holder counts as the app (LK-04).
- [own.sh](desktop-beta39/own.sh) `LABEL CMD…` makes a second launch of
  the owner's app and watches pid 8954. It aborts if `HIDIdleTime` is
  under 60 s.

```sh
QA_REPO=$PWD ./si.sh SI-02 closed
QA_REPO=$PWD OD_BIN=$PWD/target/release/od ./lk.sh LK-02 vi
APP_HOLDER=1 QA_REPO=$PWD OD_BIN=$PWD/target/release/od ./lk.sh LK-04-vi vi
```

**Trap for whoever drives it.** The lock dialog is a
`CFUserNotificationDisplayAlert`: rfd's path for a dialog shown off the
main thread. Its window belongs to `UserNotificationCenter`, not to the
app. The app's own window at that time is the splash. Reading or pressing
the app's `window 1` therefore hits the splash: the first LK-01 run
pressed the splash's close button. Address the dialog as `process
"UserNotificationCenter"`.

A side effect of this: when the app is killed while the dialog shows,
the dialog stays on screen. Its button then only closes it. A person
can do this only with Force Quit, so it is recorded here, not as a bug.

## Results

### §22 Desktop: one window per data folder

| ID | Check | Result |
|---|---|---|
| SI-01 (owner) | Installed beta.39 running, Finder in front: `open -a Contour` | **Pass**: Contour in front, one process, one listening daemon |
| SI-01 (owner) | Same, `open -n /Applications/Contour.app` | **Pass**: Contour in front at +0.5 s, the second process gone, log `opened again: showing the window` |
| SI-01 | QA copy, window shows, `open -n` | **Pass**: in front at +0.1 s, one process |
| SI-02 | Window closed (close button), `open -n` | **Pass**: the window shows again and is in front at +0.5 s |
| SI-03 | Minimised, `open -n` | **Pass**: unminimised at +0.6 s, in front at +1.1 s |
| SI-04 | Hidden, `open -n` | **Pass**: unhidden and in front at +0.5 s |
| SI-09 (new) | Closed, minimised or hidden, then `open` without `-n` (the Dock's or Finder's reopen, no new process) | **Pass** 3 of 3: in front at +0.5 s |
| LK-01 | `od daemon start` holds the folder, system language en-VN, no `locale` | **Pass**: "Contour can't open yet" with the expected words, in front after 1 s ([01](desktop-beta39/01-lk01-held-en.png)). Quit quits the app; the od keeps running; the log has the lock path and URL. Splash behind it: **QA-104** ([05](desktop-beta39/05-qa104-splash-vi-dialog-en.png)) |
| LK-02 | Same, `locale` `vi` | **Pass**: "Chưa mở được Contour", the Vietnamese words, button "Thoát" quits ([02](desktop-beta39/02-lk02-held-vi.png)) |
| LK-03 | Ctrl+C (SIGINT) the od, open again | **Pass**: opens on the same data in 1 s, no dialog |
| LK-04 | Lock held under the app's own URL, English | **Pass**: "Contour is already open" after 13 s (limit 12 s); Quit quits ([03](desktop-beta39/03-lk04-app-en.png)) |
| LK-04 | Same, Vietnamese | **Pass**: "Contour đang mở", button "Thoát" quits ([04](desktop-beta39/04-lk04-app-vi.png)) |
| LK-06 (new) | Dialog showing, then `open -n` and `open` again | **Pass**: still one process and one dialog; the log says `opened again`. The splash comes to the front, and the dialog (layer 8) stays above it |

Not run: SI-05 to SI-08 and LK-05, which passed in main36 with no change
to that code since; UP-01 and UP-02, since the owner updated from
beta.38 to beta.39 by hand (brief).

## QA-104 🟡 The splash speaks Vietnamese to every user

- **Steps**:
  1. Run on a Mac whose language is English (here en-VN, `locale` `en` in
     `app-config.json`).
  2. Hold the data folder with `od daemon start`.
  3. Open Contour.
- **Expected**: the splash in the UI's language ("Opening Contour…"), as
  the dialog in front of it is.
- **Actual**: "Đang mở Contour…" behind "Contour can't open yet"
  ([05](desktop-beta39/05-qa104-splash-vi-dialog-en.png)). For the LK-04
  case, this Vietnamese line is all the user sees for 12 s before the
  dialog. On a normal start it shows for about 1 s.
- **Where**: `apps/desktop/static/index.html:40`, with `<html lang="vi">`.
  The splash loads before the UI and its language files.
- **Fix**: choose the text the way `data_lock::lang_of` does: the
  `locale` setting, else the system language. Two ways:
  - Inject it from `main.rs` when the window is made (an
    `initialization_script` that sets `lang` and the text).
  - Keep both strings in the page and pick by `navigator.language`. This
    ignores the `locale` setting.
- **Caught by**: LK-01 and LK-04 ([lk.sh](desktop-beta39/lk.sh)).
