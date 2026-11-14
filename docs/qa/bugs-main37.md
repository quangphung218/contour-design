# QA bugs: main after beta.37 (f28385df)

QA pass on `main` at f28385df (beta.37, #198, and #200): the web suites
not run for a while (§1 Onboarding, §2 Home, §3 Chat, §4 Studio, §13 and
§15 Templates, §14 Files from this turn, §16 Reference, §20 Failures, §21
Leaving), §10 and §23 again after #198 and #200, and a new §24 on what
#198 touched: files and folders renamed or moved once comments are on
them, ZIP, Library assets and Duplicate. Web only, automated: the Mac's
screen was locked, so no desktop row was run. macOS 26, Apple silicon,
2026-11-15.

**Three bugs, none in what #198 and #200 set out to fix.** The paths of
comment images do not follow a rename or move (QA-099): moving the
`comments` folder breaks the images of the comments sent, and a comment
being written can no longer be sent ("file not found"). A comment sent
on a deleted file comes back on any file later renamed (or written) at
that name (QA-100). And the Lint button has not shown on opening a page
since beta.34 (QA-101, QA-083 back): the lint runs, its answer is
dropped. Everything else passed: drafts across renames, moves, ZIP,
Library and Duplicate keep draft images out as #198 says.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-099 | 🟠 | The paths of comment images do not follow a rename or move: a comment sent loses its image, and a comment draft whose image moved cannot be sent ("Could not save the comment: Not found: file not found") | MV-03, MV-04, MV-10 | Server: `follow_renames` (`od-store/src/store.rs:499`) moves `preview_comments.file_path` and the draft rows, not `preview_comment_images.path` (`od-store/src/comments.rs:24`). UI: `rename_drafts` (`od-ui/src/app/preview_comments.rs:103`) moves the draft to its file's new name, not its `images` | Work keyed by path (QA-097, QA-098): what #198 made follow the file, not what the comment points to |
| QA-100 | 🟠 | A comment sent on a file that is then deleted comes back, pinned, on another file renamed to that name, and is still attached to the next message | MV-05 | `delete_file` (`od-store/src/store.rs:472`) and `delete_folder` (`:562`) leave the file's `preview_comments` rows; any file later at that path shows them. #198's `follow_renames` (`:499`) adds the rename as one more way to put a file there | Same rows: a page the agent writes at the deleted name gets them too (not tested) |
| QA-101 | 🟠 | Opening a page, or switching to another, never shows the Lint button: the lint runs and answers, the answer is dropped. Only Reload preview or an agent turn shows it. QA-083 (a lint that failed says so) is back with it | FAIL-07, STU-23 | `watch_lint` (`od-ui/src/app/preview_tools.rs:1771`): its first Effect calls `refresh_lint` (`:1775`), then its second, on the same file change, calls `forget_document` (`:1791`), which bumps `lint_seq` and clears `lint` (`:264`); the answer's `seq` no longer matches. Since 845e5e2c (beta.34), which added the `lint_seq` bump to `forget_document` | Any state `forget_document` resets after a load started in the first Effect |

## How it was run

```sh
crates/od-ui/e2e/qa/run.sh onboarding                 # builds the UI and od
QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh home       # then chat, studio, templates,
                                                      # turnfiles, reference, failures,
                                                      # leave, drafts, filemoves
```

One file at a time, in that order (`run.sh` clears `test-results/qa`
on each call).

## Results

| Suite | Result |
|---|---|
| onboarding (§1) | 14 of 14 pass |
| home (§2) | 17 of 17 pass |
| chat (§3) | 12 of 12 pass |
| studio (§4) | 20 of 20 pass; STU-23 (new) a known bug: QA-101 |
| templates (§13, §15) | 21 of 21 pass |
| turnfiles (§14) | 13 of 13 pass |
| reference (§16) | 29 of 31 pass, 2 skipped (REF-27, REF-31 need beta.17 installed) |
| failures (§20) | 20 of 21 pass; FAIL-07 fails 4 of 4 runs: QA-101 (was QA-083, fixed in beta.32) |
| leave (§21) | 17 of 17 pass |
| drafts (§10, §23) | 50 of 50 pass (DL-01 to DL-52, after #198 and #200) |
| filemoves (§24, new) | 6 of 10 pass, 4 known bugs (QA-099 × 3, QA-100) |

### §24 Files moved, exported and copied with comments

| ID | Check | Result |
|---|---|---|
| MV-01 | Comment sent with an image, its file moved into a folder | **Pass**: pin on `refs/a.html`, image loads |
| MV-02 | Comment sent, the folder holding its file moved | **Pass** |
| MV-03 | Comment sent with an image, `comments/` moved into `old/` | **Fail: QA-099**: the image answers 404 |
| MV-04 | Comment sent with an image, that image renamed | **Fail: QA-099** |
| MV-05 | Comment sent on a.html, a.html deleted, b.html renamed to a.html | **Fail: QA-100** ([02](main37/02-qa100-stale-pin.png)) |
| MV-06 | Folder ZIP and selection ZIP with a draft image | **Pass**: left out; in once sent |
| MV-07 | Library assets with a draft image | **Pass**: left out; listed once sent |
| MV-08 | Duplicate with a sent and a draft image | **Pass**: the sent one copied, the draft one not |
| MV-09 | Draft image's folder moved, then Leave | **Pass**: out of the ZIP, deleted on Leave |
| MV-10 | Draft image's folder moved, then Add comment | **Fail: QA-099** ([01](main37/01-qa099-draft-folder-moved.png)) |

## QA-099 🟠 Comment images do not follow a rename or move

- **Steps (draft)**: a project with `a.html` and `old/z.html`. Open
  a.html, comment mode, click an element, type a note, attach an image
  (it lands in `comments/`). Files ▸ `comments` ▸ ⋯ ▸ Move to ▸ `old`.
  Add comment.
- **Expected**: the comment is saved with its image, now at
  `old/comments/…` (#198 keeps drafts across moves).
- **Actual**: "Could not save the comment: Not found: file not found"
  under the note; the image chip is blank
  ([01](main37/01-qa099-draft-folder-moved.png)). The only way out is to
  remove the image and attach it again.
- **Steps (sent)**: the same, Add comment first, then move `comments` (or
  rename the image). The comment's image answers 404 in the Comments tab.
- **Cause**: the server follows the move for the draft's row
  (`drafts::renamed`) and the comment's file, not for
  `preview_comment_images.path`; `follow_renames`
  (`od-store/src/store.rs:499`) needs the same `UPDATE` on that table,
  for every pair and for a folder's prefix. The UI's `rename_drafts`
  (`od-ui/src/app/preview_comments.rs:103`) moves the draft as a whole
  but keeps `images` at their old paths; it needs `drafts::moved` on each.
- **Tests**: `filemoves.qa.mjs` MV-03, MV-04, MV-10 (`knownBug('QA-099')`).

## QA-100 🟠 A deleted file's comments come back on the next file at its name

- **Steps**: a project with `a.html` ("Alpha") and `b.html` ("Beta").
  Comment on a.html, Add. Delete a.html. Rename b.html to `a.html`.
- **Expected**: Beta shows no pin; the comment about the deleted page is
  gone or listed as lost, and not attached to the next message.
- **Actual**: Beta shows pin 1, and the chip "p: About Alpha only" sits
  above the composer, attached to the next message
  ([02](main37/02-qa100-stale-pin.png)).
- **Cause**: deleting a file or a folder (`od-store/src/store.rs:472`,
  `:562`) keeps its `preview_comments` rows keyed by path; whatever file
  is next at that path inherits them. Before #198 a rename did not move
  comments at all; now `follow_renames` (`:499`) leaves the old rows at
  `to` and adds the moved ones. Delete (or mark lost) a file's comments
  when it is deleted, and refuse or merge rows already at `to` on rename.
- **Test**: `filemoves.qa.mjs` MV-05 (`knownBug('QA-100')`).

## QA-101 🟠 The Lint button does not show on a page just opened

- **Steps**: a project with `index.html` and `b.html`. Open the project.
  Wait (the `GET …/lint/index.html` answers 200). Click b.html.
- **Expected**: the Lint button (count or a check mark) in the tool bar,
  for each page; when the lint call fails, the button with "Lint failed"
  and Retry (QA-083, fixed in beta.32).
- **Actual**: no Lint button after opening, after switching, after a page
  reload. It shows only after Reload preview or an agent turn (why
  studio.e2e's lint rows still pass on CI). FAIL-07 fails 4 of 4 runs.
- **Cause**: `watch_lint` (`od-ui/src/app/preview_tools.rs:1771`) has two
  Effects on the shown file. The first starts the lint (`:1775`) with
  `seq = n + 1`; the second, created after it, runs next on the same
  change and calls `forget_document` (`:1791`), which bumps `lint_seq`
  to `n + 2` and sets `lint` to `None` (`:264`). The answer is then
  dropped as stale. Bumping `lint_seq` there came with 845e5e2c
  (beta.34). Reset first, then lint (one Effect, or the lint Effect after
  the forget Effect).
- **Tests**: `failures.qa.mjs` FAIL-07, `studio.qa.mjs` STU-23
  (`knownBug('QA-101')`).

## Not run

- Desktop rows (§17 to §19, §22): the screen was locked.
- MV-05's sibling, the agent writing a new page at the deleted name:
  same rows, not automated.
