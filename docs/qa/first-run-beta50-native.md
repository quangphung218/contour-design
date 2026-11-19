# QA: re-check of the native first run on 0.2.0-beta.50

Beta.50 ships two fixes from [first-run-beta49-native.md](first-run-beta49-native.md):
QA-112 (the app is now ad-hoc signed with sealed resources, so macOS keeps
"Allow" for Documents) and QA-113 (exports wait for entrance animations).
Released `Contour_0.2.0-beta.50_universal.dmg`, macOS 26, Apple silicon,
2026-11-19, in two sittings: 09:40–09:42 (stopped when the owner came back)
and 11:37–11:48.

**Both fixes hold.** macOS asks for Documents once; after Allow, Check
again passes in 5.4 s and a relaunch tests again with no dialog. The PDF
and PNG of a page whose hero fades and rises in show the headline, lede and
button at their final colour. No new bug was found on this path.

| # | Criterion | Result |
|---|---|---|
| 1 | Agent reading `~/Documents`: macOS asks once → Allow → Check again and relaunch with no second dialog, test passes in its timeout | **Pass** |
| 2 | Design with an entrance animation, Export PDF and PNG through the Save panel: hero title present and at final colour | **Pass** |
| 3 | Anything new a first-time user hits | No new QA bug; three notes below |

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## How it was run

- **Owner's app untouched.** The owner's Contour (`dev.contour.desktop`,
  pid 9163, started 09:57:00) had the same pid and start time at the end.
  It was not quit, focused, read or captured. Windows were picked by pid,
  because agent-desktop reports both apps as "Contour".
- **QA copy, re-sealed.** The `.app` from the DMG was copied with `ditto`
  to `~/Applications/ContourQA50b.app` and its `Info.plist` changed:
  `CFBundleIdentifier` `dev.contour.desktop.qa50b` (never used before),
  `CFBundleName` `ContourQA50b`. That breaks the seal, so the copy was
  signed again the way release does, then opened with its own data
  folder:

  ```sh
  codesign --force --deep --sign - ContourQA50b.app
  open -n --env OD_DATA_DIR=<scratch>/data ContourQA50b.app
  ```

  The first sitting used `dev.contour.desktop.qa50`; its row was reset
  and that id was not reused.
- **Driving.** agent-desktop on the copy's window only. The macOS
  Documents dialog was answered by an accessibility press on its Allow
  button (no mouse or key events). The prompt went in by clipboard paste
  (the owner's clipboard was restored), because accessibility `type` and
  `set-value` do not reach the WKWebView text area. `HIDIdleTime` was
  over 60 s before each step except right after the tool's own input.
- **Cleanup.** `tccutil reset All dev.contour.desktop.qa50b` (0 rows
  left), `lsregister -u`, and the copy, its data folder, caches, WebKit
  storage, preferences, DMG and exported files were removed.

### Signature checks

| App | `codesign --verify --deep --strict` | `codesign -dv` |
|---|---|---|
| In the DMG, untouched | valid on disk, satisfies its Designated Requirement | `Signature=adhoc`, `Sealed Resources version=2 files=5594`, id `dev.contour.desktop` |
| Copy, `Info.plist` edited | `invalid Info.plist (plist or signature have been modified)` | (expected: the seal now covers Info.plist) |
| Copy, after `codesign --force --deep --sign -` | valid on disk, satisfies its Designated Requirement | `Sealed Resources version=2 files=5594`, id `dev.contour.desktop.qa50b` |

In beta.49 the same check said "code object is not signed at all" and
`Sealed Resources=none`.

## Timeline

| Time | Step | Result |
|---|---|---|
| 09:40 | Download the DMG, `SHA256SUMS` | OK |
| 09:41 | Open the quarantined copy | "“ContourQA50” Not Opened / Apple could not verify…", Move to Trash, Done. Unchanged and documented; `xattr -dr com.apple.quarantine` as the README says |
| 09:42 | First run, copy `qa50` | One Documents dialog, answered Allow, Claude Code test passed in 12.9 s. Stopped here (owner back) |
| 11:39:15 | First run, copy `qa50b` | One Documents dialog ([03](first-run-beta50-native/03-documents-dialog.png)). Left unanswered on purpose |
| 11:40:05 | | "Test timed out after 45002 ms. macOS may be waiting on a permission dialog for Contour. Allow Contour in the macOS dialog, then Check again." |
| 11:41:36 | Allow | TCC `kTCCServiceSystemPolicyDocumentsFolder`, auth 2 |
| 11:41:57 | Check again | "Claude Code replied in 5416 ms: 'ok'". No dialog |
| 11:42:18 | Quit (SIGTERM) and open again | The screen tests by itself and passes at 11:42:24. No dialog; the TCC row is still the one from 11:41:36 |
| 11:42:47 | Continue → Home | |
| 11:43:30 | "Landing page for a small bakery with a hero whose title and lede fade in and rise on load, then a menu and opening hours", Create | |
| 11:44:55 | | Page written, "Your first design is ready" (85 s). The hero uses `opacity: 0` + `animation: rise-in 900ms … forwards`, delays up to 520 ms ([04](first-run-beta50-native/04-entrance-animation-css.txt)) |
| 11:45:26 | Export ▸ PDF → Save panel (sheet, Downloads) → Save | Panel in 4 s, file 1 s after Save. 3 pages, Letter |
| 11:46:42 | Export ▸ Image ▸ PNG → Save | Panel in 3 s, file under 1 s after Save. 1920×2676 |

## Criterion 2: what the exports show

Both exports have the hero in full: eyebrow, headline "Bread worth
*getting up* for.", lede, "See today's menu" button and the opening note
([PDF page 1](first-run-beta50-native/01-pdf-page1-hero.png),
[PNG top](first-run-beta50-native/02-png-hero.png)). `pdftotext` finds
the headline and lede on page 1.

The darkest pixel of each animated element, against the brand name in
the nav, which has no animation:

| Element | PDF (150 dpi) | PNG |
|---|---|---|
| Nav brand (not animated) | rgb(27, 21, 13) | rgb(27, 21, 13) |
| Headline | rgb(27, 21, 13) | rgb(27, 21, 13) |
| Headline accent (*getting up*) | rgb(154, 80, 72) | rgb(154, 80, 72) |
| Lede | rgb(98, 93, 86) | rgb(98, 93, 86) |
| Button | – | rgb(27, 21, 13) |

Same colour in both files and the same as unanimated text, so the capture
was taken after the animation ended. In beta.49 the PDF had no headline
and the PNG had it faded.

## Criterion 3: what a first-time user meets, by frequency

| # | Note | Who meets it | Effect |
|---|---|---|---|
| 1 | The first agent test still runs out at 45 s if the Documents dialog is not answered in time | Every new user whose agent reads `~/Documents`, if they read the dialog slowly | Low now: the message names the dialog and says to Check again, which then passes in about 5 s |
| 2 | Each release has a new ad-hoc identity, so macOS asks for Documents once after each update | Every update, same users as #1 | One dialog per update; expected with ad-hoc signing (CHANGELOG beta.50) |
| 3 | Gatekeeper still blocks the first open; the README's `xattr` step is needed | Every new install | Unchanged; the troubleshooting title now has the new wording (QA-115 fixed) |

Still open from beta.49: QA-114 (⌘Q during a run reads "Lost the
connection to Contour", `crates/od-ui/src/i18n.rs:1560`). Not re-tested.

## Backlog

- The two `state.json write failed … No such file or directory` warnings
  after the first connection test are still in the log (not shown to the
  user).
- Not checked here: JPEG, WebP and the per-slide capture, which the
  beta.50 fix also covers.
