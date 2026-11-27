# od-store

SQLite (`app.sqlite`), project folders on disk, file version history, drafts ledger.
Map: `../../docs/architecture.md`. Paths below are relative to this folder.

## Owns
- `src/store.rs:Store`: the API od-server calls. File ops that can touch a history hold `Store::versions_guard`.
- `src/files.rs`: raw disk ops inside one project folder (`ProjectRoot`).
- `src/versions.rs`: per-page history, `.file-versions/<hash>/manifest.json` plus snapshots.
  `enum History` has the states `Linked`, `Absent`, `Open`, `Closed`.
- `src/paths.rs`: path validation and link checks. `src/drafts.rs`: draft image ledger. `src/comments.rs`: comments.
- Schema: `src/db.rs:migrate`; some modules create their own tables (`drafts.rs`, `comments.rs`).

## Entry points
- `Store::write_file`, `create_file_exact`, `upload_files`, `rename_file`, `delete_file`, `move_folder`, `delete_folder`.
- History: `Store::list_versions`, `read_version`, `restore_version_if`; `src/edit.rs:edit_html_tracked`.
- Drafts: `src/drafts.rs:record_draft_upload`, `release_draft_session`, `take_draft_uploads`, `sweep_draft_uploads`.

## Invariants
1. Never write, delete or move through a link (an agent's shell can make any path a link).
   Reads may follow. A source through a link is `FileNotFound`; a target is `BadRequest`.
   (`src/paths.rs:through_link`, `src/files.rs:refuse_link`)
   - `src/files.rs::deleting_through_a_link_spares_what_it_points_to`, `src/files.rs::writing_through_a_link_is_refused`
   - `src/files.rs::moving_a_folder_through_a_link_is_refused`
2. App files are written as temp file + rename (`src/files.rs:replace_atomic`), which replaces a link.
   - `src/versions.rs::a_manifest_that_is_a_link_is_replaced_not_written_through`
   The memory files (`src/memory_index.rs`: `<data>/memory/MEMORY.md`, `config.json`) go through `od_fs::nofollow`:
   written with `replace_file` (random temp name, `O_EXCL|O_NOFOLLOW`, fsync, rename), read with `read_text`/`read_file`
   (no link followed, no FIFO waited on, bounded; either reads as missing).
   - `tests/memory_files.rs` (one test per question of the six); `../od-server/tests/user_file_writes.rs`, `no_path_reads.rs` scan it
3. A linked history is refused before the page is written (`src/versions.rs:refuse_linked_history`).
   An upload learns its name only after writing, so it is deleted again.
   - `src/comments.rs::a_history_folder_that_is_a_link_is_not_written_through`
4. Every history function goes through one `resolve()` and handles every `History` state;
   only `save()` writes closing marks. A closed history is hidden (decision #8).
   - `src/history_tests.rs::every_operation_in_every_history_state` (ops × states matrix)
   - `src/history_tests.rs::a_closed_history_lists_only_the_page_now`
5. Order: refuse first, change disk, then update history and DB. A failure after the disk changed
   is logged, not returned (`src/store.rs:log_unforgotten`).
   - `src/history_tests.rs::a_record_that_cannot_update_does_not_fail_what_is_done`
   - `src/history_tests.rs::a_history_that_cannot_update_does_not_fail_what_is_done`
6. Exception: restore and edit need the new version. If it cannot be recorded, put the file back,
   then return the error (`src/store.rs:record_or_put_back`).
   - `src/history_tests.rs::a_restore_or_edit_whose_version_cannot_be_recorded_leaves_the_page`
7. Rename and move are all or nothing: DB in a transaction, then histories, then commit;
   any error undoes the histories and puts the files back (`src/files.rs:put_back`).
   - `src/comments.rs::a_rename_or_move_whose_records_cannot_follow_puts_the_files_back`
   - `src/store.rs::histories_moved_before_one_that_cannot_follow_go_back`
8. Comments, comment images, the entry file and draft rows follow a rename and go with a delete
   (`src/store.rs:follow_renames`, `src/store.rs:forget_entry`).
   - `src/comments.rs::a_rename_takes_the_comments_pins_and_images_along`
   - `src/comments.rs::a_deleted_files_comments_do_not_come_back_on_the_next_file_at_its_name`
   - `src/drafts.rs::the_record_follows_a_rename_and_a_delete`
9. Draft claim and release run under one DB lock. An upload landing after its release is deleted at once;
   a comment on a released image is refused.
   - `src/drafts.rs::an_upload_landing_after_its_session_was_released_goes_at_once`
   - `src/drafts.rs::a_comment_on_an_image_released_meanwhile_is_refused`
10. A draft row stays while the disk refuses the delete (else the file ships in a ZIP).
    - `src/drafts.rs::a_draft_image_that_cannot_be_deleted_stays_a_drafts`
11. Opening the store sweeps day-old draft images.
    - `src/drafts.rs::opening_the_store_sweeps_day_old_draft_images`
12. A user's file names compare by `paths::name_key` (NFC, case ignored) on every disk, as on macOS:
    every target of a write, upload, rename, move or new folder, and every source of a rename, move or delete, is
    spelled as on disk first (`paths::as_on_disk`): `B.html` sent for `b.html` takes the history of `b.html` along.
    So is every name the store is sent for a page's history (list, read, restore, version), an edit or replace,
    a comment and its images, and a draft image claimed, once at the door: no second history under the name sent.
    A step that decides on a file (what it holds, its history) and the write or delete after it use that one name.
    An upload takes `-1`, a new file or rename onto a name alike is `FileExists`, an overwrite writes the file there,
    a rename to its own name in another case is allowed.
    - `src/name_key_tests.rs` (all ops; run with `TMPDIR` on a case-sensitive volume to see Linux)
    - `tests/name_key.rs` (source scan by statement: no name check by hand in `crates/*/src`; in `src/files.rs` no write
      before `as_on_disk`/`renamed_as`; in `files.rs`, `store.rs`, `edit.rs`, `comments.rs`, `drafts.rs` no name from
      `validate_project_path`/`sanitize_path` used before it is spelled (`UnspelledName`), and decide and act on one
      spelled name, `fate` only through `fate_spelled` (`DecidedApart`); exceptions in `KEPT`)

## Traps
- Each per-op fix patched one op × state cell and broke a neighbour; add a matrix cell
  for every new op or state (#211).
- A history renamed away and back took another page's versions; compare the content digest (#205).
- `delete_folder` on a link ran `remove_dir_all` on the target (#206).
- A `0444` manifest does not fail a temp + rename write; make the folder `0555` in tests (#206).
- Writing first and refusing after left the page changed while the agent saw an error (#208).
- Uploads learn their final name (`a-1.html`) after writing; delete it when its history is refused (#209).
- Restoring a closed history's entry overwrote a new page with no snapshot (#211, #212).
- Returning an error after the disk changed made a retry upload twice, conflict or 404 (#215).
- Assert an error unconditionally, not inside `if res.is_err()` (#208).
- Linux kept `Logo.png` and `logo.png` as two files; a Mac then shows one (#225, #242). Fold to the disk name, never compare by `==`.
- Still open: `Store::update_project` and `Store::delete_project` report an error after the DB changed (#215).

## Check
- `cargo test -p od-store`; the matrix is Unix-only (`src/history_tests.rs`). QA: `../od-ui/e2e/qa/fileops.qa.mjs`.
