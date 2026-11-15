# QA bugs: main after #215 and #216 (382bb69a, before beta.40)

QA pass on `main` at 382bb69a (beta.39, then #215 and #216): §24 Files
moved, §25 File operations, §10 and §23 Drafts, §4 Studio, §3 Chat and
§20 Failures again, and a new §26 on what #215 changed: a file operation
the disk has done is reported done when the database cannot follow, and
renaming or moving a file or folder the agent's shell removed says "not
found" even with `.file-versions` a link. Web only, automated. macOS 26,
Apple silicon, 2026-11-16.

**One bug, a sibling #215 left as it was.** While another program holds
the database (a DB browser left open, a backup), attaching an image to a
comment says "Could not attach the image. Try again." although the image
was written: it stays in the project as an ordinary file (Files panel,
ZIP), and Try again adds a second copy (QA-105). Everything #215 set out
to do holds: delete, folder delete, upload, paste, New page and the entry
page all say done when only the database cannot follow, a reload shows
the disk, and the three "gone × link" cases say "Not found".

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-105 | 🟠 | With the database busy, attaching an image to a comment says it failed; the image stays in the project as a plain file and Try again adds `shot-1.png` | DB-08 | `routes/files.rs:286` (`upload`): `upload_file` writes the file, then `record_draft_upload(...)?` (`od-store/src/drafts.rs:221`) fails on the `INSERT` and the `?` answers 500, with the file left on disk and no draft row | "Disk first, DB after, error reported": the pattern #215 fixed for the forget steps. #215's PR body lists it under "same pattern, not changed here" |

## How it was run

```sh
crates/od-ui/e2e/qa/run.sh filemoves                  # builds the UI and od
QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh fileops    # then drafts, studio, chat,
                                                      # failures, dbfail
```

One file at a time, in that order (`run.sh` clears `test-results/qa`
on each call). `dbfail` needs the `sqlite3` shell (macOS has it).

## Results

| Suite | Result |
|---|---|
| filemoves (§24) | 16 of 16 pass |
| fileops (§25) | 22 of 22 pass (QA-102, QA-103 stay fixed) |
| drafts (§10, §23) | 51 of 51 pass |
| studio (§4) | 21 of 21 pass |
| chat (§3) | 12 of 12 pass |
| failures (§20) | 21 of 21 pass |
| dbfail (§26, new) | 12 of 13 pass, 1 known bug (QA-105) |

Against the code before #215 (`store.rs` at abccff73^), `dbfail` has 9
of its 10 #215 cases red ("Could not delete. Try again.", upload 500, "…
holds a link" for a file gone); DB-07, GONE-04 and GONE-05 are controls
and pass on both.

### §26 When the database cannot follow the disk

| ID | Check | Result |
|---|---|---|
| DB-01 | Delete a page with a comment, comments cannot be deleted | **Pass**: dialog closes; after a reload not listed, comment not shown |
| DB-02 | Same, a folder | **Pass** |
| DB-03 | Upload a page where a removed page left a comment | **Pass**: uploaded once, no `old (1).html` |
| DB-04 | Paste text as that page | **Pass** |
| DB-05 | Delete the entry page, the project cannot be updated | **Pass**: no error; opens on index.html (the row still names home.html, see below) |
| DB-06 | Database held by another program: Delete, New page, Upload | **Pass**: each says done after the 5 s wait; a reload lists the disk |
| DB-07 | Same: Rename | **Pass**: "Could not rename. Try again.", the file unchanged |
| DB-08 | Same: attach an image to a comment | **Fail: QA-105** ([01](main40/01-qa105-attach-failed-image-kept.png)) |
| GONE-01 | Rename a file the agent removed, `.file-versions` a link | **Pass**: "Could not rename: Not found …", API 404 |
| GONE-02 | Move a folder the agent removed, same | **Pass**: "Could not move: Not found …", API 404 |
| GONE-03 | Move a file the agent removed, same | **Pass** |
| GONE-04 | Rename a removed file, no link | **Pass**: same wording |
| GONE-05 | Rename, Move a file still there, link | **Pass**: refused, names the link |

## QA-105 🟠 With the database busy, a comment's image "could not attach" but stays, and Try again doubles it

- **Steps**: a project with `index.html`, the Studio open. Another
  program takes the database's write lock and keeps it (here `sqlite3
  app.sqlite` then `BEGIN IMMEDIATE;`; a DB browser or a backup tool
  does the same). Comment mode, pick an element, Attach image
  `shot.png`. Release the lock. Attach `shot.png` again.
- **Expected**: either the image is attached, or the box says it could
  not and the project has no such image; Try again attaches one image.
- **Actual**: after the 5 s busy wait the upload answers 500 and the box
  says "Could not attach the image. Try again."
  ([01](main40/01-qa105-attach-failed-image-kept.png)), while
  `comments/shot.png` is on disk and listed in the Files panel (the
  `comments` folder shows 1). Try again writes `comments/shot-1.png` and
  attaches that one. `shot.png` has no draft row, so Leave and the sweep
  never delete it, and `GET files`, ZIP and the agent see it as a
  project file.
- **Cause**: `upload` (`od-server/src/routes/files.rs:286`) calls
  `s.upload_file(...)` (writes the file) and then
  `s.record_draft_upload(...)?`; the `INSERT` into `draft_uploads`
  (`od-store/src/drafts.rs:221`) fails with the database busy and the
  `?` turns it into a 500, with the file already written. Unlike the
  forget steps #215 changed, this row is what makes the file a draft's,
  so logging it is not enough: undo the write when the record fails
  (delete the file just uploaded, as `record_draft_upload` does for a
  released session), or record the row before the write.
- **Same site, not automated**: the manual edit's image upload
  (`manual_edit.rs:718`, the same route with a draft session). A
  multi-file upload that fails on file 2 (#215's own list) answers an
  error with file 1 written.
- **Test**: `dbfail.qa.mjs` DB-08 (`knownBug('QA-105')`).

## Noted, not a bug

- **DB-05**: after the entry page's delete with the project row
  refusing, `entryFile` still names `home.html`. The Studio opens on
  index.html, so the user sees nothing wrong; a page the agent later
  writes at `home.html` opens first again (FO-05 under a database
  failure). #215 accepts such leftovers ("rows are leftovers about a
  path").
- **DB-07**: a rename with the database busy says "Could not rename. Try
  again." and leaves the file, as `follow_renames` puts it back. Correct;
  the message does not say why (a 500 shows as "Try again").

## Not run

- Desktop rows (§17 to §19, §22): brief is web only.
- A full disk or a database file gone while the daemon runs: the
  trigger and the lock reach the same code paths.
