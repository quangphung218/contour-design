# QA bugs: main after #204–#209 (09524e1c, before beta.39)

QA pass on `main` at 09524e1c (#204 to #209 merged): §24 Files moved
(MV-01 to MV-16), §10 and §23 Drafts, §3 Chat, §4 Studio, §20 Failures
and §14 Files from this turn again, and a new §25 on what #204 to #209
changed: a page's history across `page.html` → `page.md` → `page.html`,
a rename only in case, deleting the entry page, folders with diacritics
(`Ảnh/`), links made by the agent's shell (delete, move, write and rename
through them, a linked folder, `.file-versions` a link) and the chat end
kept in view after a run. Web only, automated: the Mac's screen was
locked, so no desktop row was run. macOS 26, Apple silicon, 2026-11-15.

**Two bugs, both where a version history is closed or linked.** A
history closed by #204/#205 (its page replaced outside the app, or
deleted) is still listed as the file's own, with its last version marked
current, and Restore on it writes the old page over the file shown; the
page that was there is in no version, so it is lost (QA-102). With
`.file-versions` a link, deleting a file deletes it and then says "Could
not delete" (QA-103): #208 made writes refuse before they touch the
disk, deletes still refuse after. Everything else #204 to #209 set out to
do holds: renames and moves, case-only renames, `Ảnh/`, every link
refusal, the entry page, and the chat end in view.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-102 | 🔴 | A closed history (its page replaced outside the app, or deleted) is listed as the file's own, last version marked current; Restore writes the old page over the file and the file's content is in no version | FO-03, FO-17 | `versions::list_versions` (`od-store/src/versions.rs:179`) ignores `deleted_at`; `create_version` (`:253`) lets a restore of the closed history's own entry through (`restoring_own`), clears `deleted_at` and reopens it, without a snapshot of the file it overwrites | "Closed but still served": #205's PR body leaves the listing as in the original app; the closure only acts at the next save |
| QA-103 | 🟠 | With `.file-versions` a link, deleting a file or a folder deletes it (and its comments), then says "Could not delete: … .file-versions holds a link" | FO-18 | `Store::delete_file` (`od-store/src/store.rs:513`) and `delete_folder` (`:663`) delete on disk and forget the rows, then `versions::mark_deleted` (`:528`, `:676`) reaches `store_dir`, which refuses the link | #208's "refuse first" covered `write_file`/`create_file_exact`, #209 `upload_file`; delete is the one left. Any kind of file (also `.txt`, images, a draft image's ×), and the 404 path then answers 400, not 404 |

## How it was run

```sh
crates/od-ui/e2e/qa/run.sh filemoves                  # builds the UI and od
QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh fileops    # then chat, studio, turnfiles,
                                                      # failures, drafts
```

One file at a time, in that order (`run.sh` clears `test-results/qa`
on each call).

## Results

| Suite | Result |
|---|---|
| filemoves (§24) | 16 of 16 pass (MV-11 to MV-16 included; QA-099 and QA-100 stay fixed) |
| fileops (§25, new) | 19 of 22 pass, 3 known bugs (QA-102 × 2, QA-103) |
| chat (§3) | 12 of 12 pass |
| studio (§4) | 21 of 21 pass (STU-23: QA-101 fixed in beta.38) |
| turnfiles (§14) | 13 of 13 pass |
| failures (§20) | 21 of 21 pass (FAIL-07: QA-101 fixed) |
| drafts (§10, §23) | 51 of 51 pass (DL-01 to DL-53) |

### §25 File operations after beta.38

| ID | Check | Result |
|---|---|---|
| FO-01 | page.html (v1, v2) → page.md → page.html, then a save | **Pass**: both listed, the save adds a third |
| FO-02 | Same, page.md written in the app between | **Pass** |
| FO-03 | Same, page.md replaced outside the app between | **Fail: QA-102**: History lists the old page's versions, v2 current |
| FO-04 | about.html → About.html (comment, version) | **Pass**: disk, list, comment, pin and versions say About.html |
| FO-05 | Delete the entry page, the agent writes it again | **Pass**: entryFile cleared, opens on index.html |
| FO-06 | Delete a page already gone outside the app, new page there | **Pass**: the new page's history has no old version |
| FO-07 | Delete folder `Ảnh/` with a comment and its image | **Pass**: comment gone, no pin on a new Ảnh/a.html |
| FO-08 | Same, `Ảnh` made by a shell in NFD | **Pass** |
| FO-09 | Rename to bìa.html, move Ảnh into Lưu-trữ | **Pass**: comment on `Lưu-trữ/Ảnh/bìa.html` |
| FO-10 | Delete a link to a folder (inside and outside the project) | **Pass**: refused, both folders intact |
| FO-11 | Delete, rename, write a link to a file | **Pass**: refused; the message names the link |
| FO-12 | Write, upload, move, rename into a linked folder | **Pass**: refused, nothing outside |
| FO-13 | Move a linked folder, or a folder in it | **Pass** |
| FO-14 | `.file-versions` a link: write a page | **Pass**: refused before the page changes |
| FO-15 | `.file-versions` a link: Files ▸ Upload a page, then an image | **Pass**: "Upload failed", no page left; the image uploads |
| FO-16 | Links in the Files panel and Move to | **Pass**: not offered |
| FO-17 | Page deleted in the app, written again by the shell | **Fail: QA-102** |
| FO-18 | `.file-versions` a link: Files ▸ Delete | **Fail: QA-103** ([02](main39/02-qa103-delete-said-failed.png)) |
| CS-01 | Run ends with the deliverable warning, 1360×600 | **Pass**: last lines in view, no Jump to latest |
| CS-02 | Same on a phone (390×740) | **Pass** |
| CS-03 | Reader at the top as the run ends | **Pass**: stays at the top, Jump to latest |
| CS-04 | Message queued at the end | **Pass** |

## QA-102 🔴 A closed history is offered, and Restore loses the page shown

- **Steps (A)**: a project with `page.html` and two versions ("v1",
  "v2"). Files ▸ Rename `page.md`. The agent's shell replaces it
  (`rm page.md`, writes another page there). Files ▸ Rename `page.html`.
  Open it, History.
- **Steps (B)**: the same page with two versions. Files ▸ Delete
  `page.html`. The agent's shell writes a new `page.html`. Open it,
  History.
- **Expected**: History has none of the old page's versions (#205: "A
  `page.md` replaced outside the app … no longer takes the old page's
  history"); at most the new page as v1.
- **Actual**: History lists v1 to v4 of the old page, v4 marked current,
  each with Restore ([01](main39/01-qa102-closed-history-listed.png)).
  Restore v1 writes "Old one" over the new page and the new page is in no
  version ([probe](main39/qa102-restore-probe.txt): step 5 "any with the
  shell page? false"). The history is closed on disk (`deletedAt` set);
  the next save wipes it, which is why FO-01's check after a save passes.
- **Cause**: closing a history (`deleted_at`) only acts in
  `create_version` and `ensure_current_version`. `list_versions`
  (`od-store/src/versions.rs:179`, through `Store::list_versions`,
  `store.rs:737`) returns its entries and `current` as if live.
  `create_version` (`versions.rs:253`) treats a restore of one of the
  closed history's own entries as `restoring_own`, keeps the entries,
  clears `deleted_at` and writes the old content, with no snapshot of the
  file it replaces. Fix: list a closed history as empty (or a baseline of
  the file as it is, as for an empty one), and refuse a restore from a
  closed history, or snapshot the working file first.
- **Tests**: `fileops.qa.mjs` FO-03, FO-17 (`knownBug('QA-102')`).

## QA-103 🟠 With `.file-versions` a link, Delete deletes and says it could not

- **Steps**: a project with `b.html` and `notes.txt`. The agent's shell
  makes `.file-versions` a link (`rm -r .file-versions; ln -s /tmp/x
  .file-versions`). Files ▸ `b.html` ▸ Delete ▸ Delete.
- **Expected**: either refused with the file still there (as a write is
  since #208, an upload since #209), or deleted with no error.
- **Actual**: the dialog says "Could not delete: The request was not
  accepted: .file-versions holds a link; Contour does not write version
  history through it" ([02](main39/02-qa103-delete-said-failed.png)),
  while `b.html` is gone from the disk and its comments from the
  database; the Files panel still lists it until the list is read
  again. The same for a folder (`DELETE folders/f`: 400, folder gone),
  checked by API.
- **Cause**: `Store::delete_file` (`od-store/src/store.rs:513`) runs
  `files::delete_file`, then `forget` (drafts, comments, entry), then
  `versions::mark_deleted` (`:528`), whose first step `store_dir` refuses
  the link; `delete_folder` (`:663`, `:676`) the same per file. A file
  that is not a page also goes through `store_dir` (`versions.rs:361`).
  Fix: `versions::refuse_linked_history` before the disk delete (as
  `write_file` since #208), or let `mark_deleted` skip a history it may
  not touch. On the already-gone path (`:533`) the error turns the 404
  the Studio counts as deleted (MV-15) into a 400.
- **Test**: `fileops.qa.mjs` FO-18 (`knownBug('QA-103')`).

## Not run

- Desktop rows (§17 to §19, §22): the screen was locked.
- QA-103's siblings (a draft image's ×, Leave releasing draft images with
  `.file-versions` a link): same function, not automated.
