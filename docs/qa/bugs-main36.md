# QA bugs: main after beta.36 (76c545d0)

QA pass on `main` at 76c545d0 (beta.36 and #197, tests only): the desktop
app's one window per data folder and the data lock (new in beta.36,
[test-plan.md](test-plan.md) §22), comment drafts after #189 to #193 (§10,
§21, new §23), and the web suites not run for a while (§5 Export, §6
Manage projects, §7 Library, §9 Automations, §12 MCP). macOS 26, Apple
silicon, 2026-11-14.

**Three bugs in comment drafts, none in the single-instance and lock
work.** A draft thrown away with Leave keeps its image in `comments/`
(QA-096), and that image then ships in the project's ZIP export. Renaming
a file throws its comment draft away without a word (QA-097). A draft on a
deck slide the agent removed cannot be seen or discarded, yet every way
out of the Studio still asks about it (QA-098). Every single-instance and
lock row that could run passed; the rows that need to see or press a
window (the window coming to the front, the dialog's words, Update now)
were not run: the Mac's screen was locked for the whole pass
(`CGSSessionScreenIsLocked = 1`), so nothing could be captured or clicked.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-097 | 🟠 | Renaming (or moving) a file throws away its comment draft (note and images) and its Manual edit change not applied, without asking | DL-32, DL-35 | `preview_comments.rs:212` `forget_gone_drafts` treats the old path as a file deleted; the rename dialog does not ask | QA-098 (work keyed by where it was made); `manual_edit.rs:195` in the same Effect |
| QA-098 | 🟠 | A comment draft on a deck slide the agent removed cannot be seen, sent or discarded, yet leaving the Studio asks about it every time | DL-34 | `preview_comments.rs:148` `on_slide`: the box shows only over its own slide index, which no longer exists; `unsent_files` (`:244`) still counts it | QA-097 |
| QA-096 | 🟠 | A comment draft thrown away by leaving (Leave, a reload, closing the tab or the app) keeps its images in `comments/`, and an image still uploading lands there too; they then ship in the project's ZIP export | DL-28, DL-29, `GET …/archive` | Only Cancel and a file gone delete a draft's images (`preview_comments.rs:298`, `:238`); leaving drops the drafts with the Studio. The upload's late cleanup (`comments_panel.rs:423`) runs in a Studio task that stops once the Studio is left | Cleanup tied to one way out; known since QA-095, recorded here with evidence |

## How it was run

### Desktop (§22)

A release build of the worktree whose **compiled** identifier is
`dev.contour.desktop.qa`:

```sh
(cd crates/od-ui && trunk build --release)
cd apps/desktop && npx --yes @tauri-apps/cli@2.12.4 build --bundles app \
  --config '{"identifier":"dev.contour.desktop.qa","productName":"ContourQA"}'
```

The single-instance plugin's socket is `/tmp/<identifier>_si.sock`, built
from the identifier compiled into the binary, not from `Info.plist`: a copy
that only renames the bundle id (as earlier passes did) would hand every
second launch to the owner's Contour. Single instance is on only without
`OD_DATA_DIR`, so the copy finds its data folder through a `HOME` of its
own, passed by `open --env` ([qa.sh](main36/qa.sh)):

```sh
open [-n] --env HOME=<qa>/home --env PATH=<qa>/mock:/usr/bin:/bin \
  --env OD_RESCAN_INSTALL_DIRS=0 <qa>/ContourQA.app
```

**Trap**: `LSEnvironment` `HOME` in the copy's `Info.plist` is ignored
(LaunchServices sets `HOME`), so the first launch opened the owner's data
folder; it was quit after 1 s (see the report). `open --env HOME=` is kept.
The `PATH` and `OD_RESCAN_INSTALL_DIRS=0` keep the agent the mock: without
them the app found the real `claude` in `~/.local/bin`.

Tools in [main36](main36/):

- [cg.swift](main36/cg.swift): main34's, plus `watch PID SECS` (frontmost
  app, topmost window, the copy's window shown or hidden, on each change)
  and `quit PID` (the Quit Apple event to that one process).
- [second.sh](main36/second.sh) `LABEL [APP]`: a second launch with Finder
  in front, watched for 6 s.
- [quitreopen.sh](main36/quitreopen.sh) `DELAY [N]`: a run of the mock
  agent that ignores SIGTERM, Quit, a second launch `DELAY` s later, then
  the pids, the lock's URL and whether a daemon answers, every 0.5 s.
- [twin.sh](main36/twin.sh): two `open -n` at once with nothing running.
- [mock-claude](main36/mock-claude): `qa-stubborn` ignores SIGTERM for 20 s.

Copy the scripts next to the built `ContourQA.app`, `cg` and `home/` (see
the head of `qa.sh`); they find each other there.

### Web (§5, §6, §7, §9, §10, §12, §23)

```sh
crates/od-ui/e2e/qa/run.sh drafts                  # builds the UI and od
QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh export  # then projects, library, automations, mcp
```

## Results

### §22 Desktop: one window per data folder

| ID | Check | Result |
|---|---|---|
| SI-01 | Second launch (`open -n`) while the window shows | **Pass** (process): the second process quits within 2 s, the first logs `opened again: showing the window`. Whether the window comes to the front: **not run** (screen locked) |
| SI-02 / 03 / 04 | … window closed, minimised, hidden (⌘H) | **Not run** (screen locked) |
| SI-05 | A second copy of the `.app` elsewhere | **Pass** (process): handed to the first, one process left |
| SI-06 | Quit with an agent that ignores SIGTERM, open again 0 / 0.2 / 1 s later | **Pass** 3 of 3: the new launch waits, takes the lock at +3 s, its daemon answers; no lock dialog |
| SI-07 | Two `open -n` at the same instant, nothing running | **Pass** 6 of 6: one process, one socket, one daemon |
| SI-08 | A copy with its own `OD_DATA_DIR` beside the app | **Pass**: both run, each its own folder |
| LK-01 | `od daemon start` holds the folder, open the app | **Pass** (log): `the data folder is held holder=Other`, at once, with the lock path and URL in the log. The dialog's words and Quit: **not run** (screen locked) |
| LK-02 | Same in Vietnamese | **Not run** (screen locked) |
| LK-03 | Stop the daemon (Ctrl+C), open again | **Pass**: the app opens on the same data in 1 s |
| LK-04 | The lock held under the app's own URL, never let go | **Pass** (log): `holder=App` after 13 s (limit 12 s) |
| LK-05 | beta.35 holds the folder, open beta.36 | **Pass** (log): `holder=Other` at once, so the dialog that names an older Contour |
| UP-01 / 02 | Update now ▸ Restart from beta.35 | **Not run** (screen locked: Settings cannot be pressed) |

**Not a bug, a trap for whoever drives it**: `tell application id
"dev.contour.desktop.qa" to quit` sent at the instant a second copy starts
can reach the new process, which then quits at once without answering; the
running app never gets the Quit (the event times out after 2 min). Sent to
the pid (`cg quit PID`, as the Dock does) it always quits. A person's ⌘Q
goes to the app itself.

### §23 Comment drafts after beta.35

| ID | Check | Result |
|---|---|---|
| DL-28 | A comment with an image, Projects ▸ Leave | **Fail: QA-096**: `comments/ref.png` stays |
| DL-29 | The same while the image is still uploading | **Fail: QA-096**: `comments/slow.png` lands and stays |
| DL-30 | A reload with a note not sent | **Pass**: the browser asks |
| DL-31 | Drafts on a.html and b.html, then Projects | **Pass**: the question names both |
| DL-32 | Rename a.html (with a draft) to c.html | **Fail: QA-097**: note and image gone, no question |
| DL-33 | Delete a.html (with a draft and an image) | **Pass**: the image goes too |
| DL-34 | Deck: draft on slide 2, the agent rewrites the deck with 1 slide | **Fail: QA-098** ([01](main36/01-qa098-deck-slide-gone.png)) |
| DL-35 | Manual edit: text typed in Content, page.html renamed | **Fail: QA-097**: leaving no longer asks; the change is gone |

### Suites run again

| Suite | Result |
|---|---|
| drafts (§10, §23) | §10: 25 of 25 pass. §23 (DL-28 to DL-35): 3 pass, 5 known bugs. The first run of §23 failed on a missing import in the test, fixed |
| export (§5) | 14 of 14 pass |
| projects (§6) | 25 of 25 pass |
| library (§7) | 18 of 18 pass |
| automations (§9) | 7 of 7 pass |
| mcp (§12) | 2 of 2 pass |

## QA-097 🟠 Renaming a file throws its drafts away without asking

- **Steps**: open a.html, comment mode, click an element, type "Keep this
  note", attach an image. In the Files panel: a.html ▸ ⋯ ▸ Rename ▸
  `c.html` ▸ Rename. Open c.html.
- **Expected**: the draft follows the file (c.html shows the box with the
  note and image), or the rename asks first, as leaving does. The beta.35
  changelog: "a comment being written … stay[s] with [its] file".
- **Actual**: no question; c.html has no draft; the image is deleted from
  `comments/` (DL-32). The same for a Manual edit change typed and not
  applied: after the rename, leaving no longer asks, the change is gone
  (DL-35). Moving a file to a folder takes the same path (not run).
- **Cause**: `crates/od-ui/src/app/preview_comments.rs:212`
  `forget_gone_drafts`: an Effect on `ctx.files` drops every draft whose
  path is no longer listed, and deletes its images; `manual_edit.rs:195`
  `drop_gone` does the same for Manual edit. A rename is seen as a delete.
- **Fix**: on a rename or move done in the app, move the drafts to the new
  path (`files_panel` knows old and new); keep the drop for a file really
  gone. A file renamed outside the app (the agent, Finder) can still ask
  before dropping, or keep the draft under the old name until left.
- **Test**: DL-32, DL-35 (`drafts.qa.mjs`, `knownBug: 'QA-097'`).

## QA-098 🟠 A draft on a deck slide that is gone cannot be reached

- **Steps**: a deck with 3 slides; go to slide 2, comment mode, click the
  heading, type a note. The agent rewrites the deck with 1 slide.
- **Expected**: the note is kept and can be sent or discarded: the box
  shows on the slide that is left, with "pick the element again" (as for
  an element gone, beta.35), or "Discard it".
- **Actual**: the deck shows 1 / 1, no box, no "Go to slide 2"
  ([01](main36/01-qa098-deck-slide-gone.png)); comment mode does not show
  it anywhere. Projects still opens "leave with work not saved?" naming
  `deck.html`, and so do Restart after an update and Quit: the user can only
  leave and lose it, never see what is lost.
- **Cause**: `crates/od-ui/src/app/preview_comments.rs:148` `on_slide`
  shows the box only when the draft's slide equals the slide shown; the
  draft keeps index 1 of a deck that now has index 0 only. `unsent_files`
  (`:244`) counts it for every leave question. `DraftElsewhere` (`:663`)
  shows only for a draft `waiting` on a new pick, not this one.
- **Fix**: when the deck's slide count is known and the draft's slide is
  past it, treat the draft as one whose element is gone (show it on the
  slide shown, `comment-reselect`), or offer Discard.
- **Test**: DL-34 (`knownBug: 'QA-098'`).

## QA-096 🟠 Images of a draft thrown away stay in `comments/` and ship in the ZIP

- **Steps**: comment mode, click an element, type, attach `ref.png`
  (it uploads to `comments/ref.png`). Projects ▸ Leave. Or: the upload
  held, then Projects ▸ Leave, then the upload lands.
- **Expected**: Leave throws the comment away as Cancel does, and Cancel
  deletes its images (beta.35 changelog). Nothing the user threw away is
  left in the project.
- **Actual**: `comments/ref.png` stays (DL-28), and `comments/slow.png`
  lands after Leave and stays (DL-29). Nothing points to them, and
  `GET /api/projects/:id/archive` (Export ▸ ZIP) puts them in the ZIP:
  `comments/orphan.png` next to `index.html` (checked by API on the QA
  copy). A screenshot the user thought discarded goes to whoever gets the
  ZIP.
- **Cause**: `crates/od-ui/src/app/preview_comments.rs:298` (Cancel) and
  `:238` (file gone) call `drop_draft_images`; no way out of the Studio
  does (`leave_guard.rs:267` leave, `beforeunload`, the desktop's Quit and
  Restart). The upload's own cleanup for a draft gone
  (`comments_panel.rs:423`) runs inside `ctx.tasks.spawn`, which stops at
  the first wake-up after the Studio is left (`tasks.rs:69`), so a late
  upload is never deleted.
- **Fix**: on Leave, delete the images of the drafts left behind (a
  request that outlives the Studio: `spawn_then`'s work part); run the
  upload's cleanup in work that outlives the Studio too. For a reload or a
  crash, have the server skip `comments/` files no comment points to in
  the ZIP, or sweep them when the project opens.
- **Test**: DL-28, DL-29 (`knownBug: 'QA-096'`).
- **Siblings, same class (cleanup only on one way out)**: the reload,
  closing the tab and the desktop's Quit drop drafts the same way (not
  run separately); Manual edit's uploaded image (`manual_edit.rs:205`
  `take_upload`) for a change never applied (not run).
