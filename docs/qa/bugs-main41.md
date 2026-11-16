# QA bugs: main after beta.40 (87ddea73, #215–#222)

QA pass on `main` at 87ddea73 (beta.40, then #222): what #217–#222
changed, from the user's side. Several files sent together, one of which
fails, in the Files panel, a comment, the chat composer, the Media
library, Add from library, Chat to design and a saved template; a design
system's asset upload and accepting or rejecting a revision; a comment's
draft image on Leave after the user's own program rewrote it. Also the
suites §24, §10/§23, §26, §13, §4 and §20 again. Web only, automated.
macOS 26, Apple silicon, 2026-11-16. Desktop not run: #217–#222 changed
`od-store` and `od-server` only, which the desktop app runs unchanged.

**Two bugs.** What #218–#222 set out to do holds: a batch with one bad
file leaves nothing behind, Try again lands at the same name, Chat to
design and a template leave no half project, a revision whose decision
cannot be saved stays pending with the package unchanged, and a draft
image rewritten in place is kept on Leave. But a file whose name is too
long for the disk fails with a bare "Try again", which fails the same way
every time (QA-106), and a design system's assets named in Vietnamese are
folded to one ASCII name, so the second is written over the first while
the page says "Uploaded." (QA-107).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-107 | 🟠 | Two design system assets `màu.png` and `mẫu.png` sent together: "Uploaded.", only one `m-u.png` kept, the other written over | DS-02 | `safe_asset_name` (`routes/library/design_systems.rs:751`) maps every non-ASCII letter to `-`, and `upload_assets` writes with `tokio::fs::write` (overwrites) | Create design system from attached files: `source_name` (`routes/w3/design_systems.rs:126`), same folding, `std::fs::write` over the first |
| QA-106 | 🟡 | A file whose name is too long for the disk: "Upload failed. Try again." (or "Contour could not finish this. Try again."); Try again fails the same way, and the message names neither the file nor the reason | UL-01, UL-03, UL-05, UL-06 | `ENAMETOOLONG` becomes `StoreError::Io` (`od-store/src/error.rs:107`), so the API answers 500 `INTERNAL_ERROR` and the UI shows its "try again" sentence (QA-081 rule for internal failures) | An `io::Error` the user can act on flattened to 500: every upload site (Files panel, comment, composer, Media, manual edit image), Use template (UL-09 "Could not create the project. Try again."), and design system writes, which wrap every `io::Error` in `ApiErr::internal` (DS-03 "Permission denied" shows "Could not update the design system. Try again.") |

## How it was run

```sh
crates/od-ui/e2e/qa/run.sh uploads                    # builds the UI and od
QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh drafts     # then filemoves, dbfail,
                                                      # templates, studio, failures
```

One file at a time (`run.sh` clears `test-results/qa` on each call).
`uploads` and `dbfail` need the `sqlite3` shell (macOS has it).
`QA_SHOTS=<dir>` saves the evidence screenshots.

## Results

| Suite | Result |
|---|---|
| uploads (§27, new) | 10 of 15 pass, 5 known bugs (QA-106 ×4, QA-107) |
| drafts (§10, §23) | 51 of 51 pass |
| filemoves (§24) | 16 of 16 pass |
| dbfail (§26) | 13 of 13 pass (QA-105 stays fixed) |
| templates (§13) | 21 of 21 pass |
| studio (§4) | 21 of 21 pass |
| failures (§20) | 21 of 21 pass |

### §27 Uploads all or nothing

| ID | Check | Result |
|---|---|---|
| UL-01 | Files panel: `ok.png` + a 310-letter name | **Fail: QA-106**. All or nothing holds (nothing on disk, not listed, not in the ZIP; `ok.png` alone lands as `ok.png`); the toast says "Upload failed. Try again." |
| UL-02 | Files panel: image + page, `.file-versions` a link | **Pass**: refused, names the link, `a.png` not left |
| UL-03 | Comment: `shot.png` + long name | **Fail: QA-106** ([01](main41/01-qa106-long-name-try-again.png)). No chip, `comments/` empty, `shot.png` again lands as `comments/shot.png` |
| UL-04 | Comment: the 2nd image's draft row refused | **Pass**: nothing on disk; after, both land at their own names |
| UL-05 | Chat composer: `ok.png` + long name | **Fail: QA-106**. One by one by design: `ok.png` attached, the other's card says "Upload failed. Try again." and its retry fails again |
| UL-06 | Media library: `up06.png` + long name | **Fail: QA-106**. Nothing kept; "Contour could not finish this. Try again." (no word of uploading) |
| UL-07 | Add from library, one asset deleted meanwhile | **Pass**: "Could not add from library (Not found: file not found)", no copy |
| UL-08 | Chat to design, `pending_prompt` refused | **Pass**: "Could not create the project. Try again.", no project, no folder |
| UL-09 | Use a saved template with a file that cannot be written | **Pass** (twice): no project, no folder. The message is QA-106's |
| DS-01 | DS assets: `logo.png` + `notes.txt` | **Pass**: "notes.txt: only images (PNG, JPG, SVG…) and fonts", nothing added |
| DS-02 | DS assets: `màu.png` + `mẫu.png` | **Fail: QA-107** ([02](main41/02-qa107-vietnamese-assets-one-kept.png)) |
| DS-03 | Accept, `revision.json` read-only | **Pass**: error, `tokens.css` unchanged, pending; Reject later leaves it unchanged |
| DS-04 | Accept a token rebuild (control) | **Pass** |
| DR-01 | Draft image rewritten in place, same size and mtime, then Leave | **Pass**: kept with the user's bytes |
| DR-02 | Draft image untouched, Leave (control) | **Pass**: deleted |

## QA-107 🟠 Design system assets with Vietnamese names overwrite each other, and the page says "Uploaded."

- **Steps**: Design systems ▸ a system of your own ▸ Kit ▸ upload
  `màu.png` and `mẫu.png` together (two different pictures).
- **Expected**: both kept (as the Files panel keeps `màu.png` and
  `mẫu.png`), or one refused with a reason. Never one lost unseen.
- **Actual**: "Uploaded." ([02](main41/02-qa107-vietnamese-assets-one-kept.png));
  the API answers both as `m-u.png`; `assets/` holds one `m-u.png`, the
  second picture. The same happens across uploads: `mẫu.png` today
  replaces the `màu.png` of last week, and `logo chính.png` replaces
  `logo-ch-nh.png`.
- **Cause**: `safe_asset_name` (`od-server/src/routes/library/design_systems.rs:751`)
  keeps only ASCII letters, digits and `.-_`, so every Vietnamese letter
  becomes `-`; `upload_assets` then writes each part with
  `tokio::fs::write`, which overwrites. The comment there accepts
  overwriting "assets of the same name", but these were not the same
  name. Fix: keep Unicode letters as `paths::sanitize_name` does, and
  refuse (or suffix) a name taken twice in one batch.
- **Same class**: Create design system ▸ attach files
  (`routes/w3/design_systems.rs:126` `source_name`): `Hướng dẫn màu.pdf`
  and `Hướng dẫn mẫu.pdf` both become `H--ng-d-n-m-u.pdf`, the second
  written over the first in `source/`, and the brief lists the name
  twice. Found by reading the code, not automated.
- **Test**: `uploads.qa.mjs` DS-02.

## QA-106 🟡 A name too long for the disk says only "Try again", and Try again fails the same way

- **Steps**: Files panel ▸ Upload, or a comment's Attach image, or the
  composer, or Media ▸ Upload: a file whose name is over 255 bytes (a
  "Save page as" or a messaging app can make one; about 85 Vietnamese
  letters are enough).
- **Expected**: the message names the file and says its name is too long
  (and what to do: rename it), or the name is shortened to fit, keeping
  its extension.
- **Actual**: the API answers 500 `INTERNAL_ERROR` "io error: File name
  too long (os error 63)". The user reads "Upload failed. Try again."
  (Files panel, composer card), "Could not attach the image. Try again."
  ([01](main41/01-qa106-long-name-try-again.png)), or "Contour could not
  finish this. Try again." (Media). Try again sends the same file and
  fails the same way. In a batch the user cannot tell which file is the
  bad one. Since #218 nothing of the batch is left, which is right.
- **Cause**: `From<io::Error> for StoreError` (`od-store/src/error.rs:107`)
  maps only permission and full disk; `ErrorKind::InvalidFilename`
  (ENAMETOOLONG) falls to `StoreError::Io`, answered as 500
  `INTERNAL_ERROR`, and the UI hides internal causes behind "Try again"
  (QA-081). `paths::sanitize_name` has no length limit. Fix: shorten in
  `sanitize_name` (keep the extension, cut on a char boundary under 255
  bytes, leaving room for `-N`), or map `InvalidFilename` to a 400 that
  names the file.
- **Same class** (an `io::Error` the user can act on answered as 500):
  Use template (UL-09: "Could not create the project. Try again."), the
  manual edit's image upload (same route), and design system writes,
  which wrap every `io::Error` in `ApiErr::internal`
  (`routes/library/design_systems.rs`: DS-03's "Permission denied" reads
  "Could not update the design system. Try again.", where the store
  would answer `NO_WRITE_PERMISSION`); a design system asset with a long
  name too (`safe_asset_name` has no limit either).
- **Test**: `uploads.qa.mjs` UL-01, UL-03, UL-05, UL-06.

## Tests corrected during the pass (test faults, not product)

- **UL-03, UL-04** first waited for a `role="alert"`; the comment box's
  failure line is plain text, so they read it by its words (as DB-08).
- **UL-05** first read the long file's name as its message; it now reads
  the failed card's tooltip.
- **UL-09** once read the first press's alert during the second press,
  and counted the projects while the second, half made, still existed.
  Each press now waits for its own answer (3 of 3 runs pass).

## Noted, not a bug

- **UL-03, UL-04**: a comment image batch that failed leaves an empty
  `comments/` folder (the Files panel shows `comments 0`). The upload
  makes the folder before the files; an empty folder holds nothing.
- **Comment box failure line** (`preview_comments.rs:831`): "Could not
  attach the image. Try again." is a plain `<p class="error-text">`, not
  `role="alert"`, so a screen reader does not announce it. The Studio's
  toasts and the Library's notices are alerts.
