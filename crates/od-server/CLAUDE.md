# od-server

The daemon: axum HTTP API, SSE, static UI. Every client (UI, desktop, CLI, MCP) goes through it.
Map: `../../docs/architecture.md`. Paths below are relative to this folder.

## Owns
- Routes under `src/routes/` (one module per area), the origin and token guards (`src/guard.rs`),
  the daemon lock (`src/lock.rs:DaemonLock`), start and shutdown (`src/lib.rs:serve`, `router`).
- `app-config.json` and the 0600 secret config files. Everything in SQLite goes through `od-store`.

## Entry points
- Start: `src/lib.rs:serve` (CLI daemon), `src/lib.rs:router` (desktop, in-process).
- App config: `src/routes/ui.rs:get_config`, `put_config`, `add_recent_dir_route`; in-process writes `patch_config`.
- Project locations: `src/routes/manage/locations.rs:put`.
- Runs: `src/routes/runs.rs:create` → `prepare_and_launch`; SSE `src/routes/runs.rs:events`.
- Files: `src/routes/files.rs:upload`, `release_drafts`; file ops in `src/routes/file_ops.rs`.
- Export: `src/routes/export/mod.rs:routes` (renderer is `od_render_cdp::CdpRenderer`).

## Invariants
1. Every read-modify-write of `app-config.json` holds `CONFIG_LOCK` (`src/routes/ui.rs`).
   Code outside `src/routes/ui.rs` writes app config only through `patch_config`, never the file directly.
   - `tests/manage.rs::two_recent_folders_added_at_once_both_stay`
2. `PUT /api/app-config` is an RFC 7396 merge patch: objects merge, `null` removes at any depth,
   arrays replace (`src/routes/ui.rs:merge_patch`, `merge_object`).
   - `src/routes/ui.rs::merge_null_removes`, `src/routes/ui.rs::merge_is_recursive_for_objects`
   - `src/routes/ui.rs::merge_replaces_arrays_and_non_objects`, `tests/ui.rs::app_config_round_trip`
3. The generic PUT never changes `projectLocations` or `defaultProjectLocationId`;
   it drops `locations::CONFIG_KEYS`. Only `src/routes/manage/locations.rs:put` writes them.
   - `tests/manage.rs::a_settings_save_with_an_old_copy_keeps_the_locations`
4. BYOK checks (key required, Azure endpoint) run on the merged config, not the patch.
   The stored key and model stay only while the route stays (`same_route`, `byok_moved`);
   `savedRoute` in a patch is compared, never stored.
   - `src/routes/ui.rs::an_azure_byok_needs_its_endpoint_after_the_merge`, `src/routes/ui.rs::byok_on_a_cloud_preset_needs_a_key`
   - `src/routes/ui.rs::merge_keeps_the_byok_key_only_on_its_route`, `src/routes/ui.rs::the_byok_model_follows_its_route`
   - `tests/runtimes_m6.rs::a_byok_patch_is_checked_after_the_merge`
5. Recent folders are added on the server under the lock (`POST /api/app-config/recent-dirs`,
   `src/routes/ui.rs:push_recent_dir`), never sent as a whole array.
   - `src/routes/ui.rs::a_recent_dir_goes_first_once`
6. Draft images never ship until a comment claims them. Every route that ships project files
   filters `unclaimed_draft_uploads`: ZIP (`src/routes/export/mod.rs`), selection ZIP (`src/routes/file_ops.rs`),
   Library assets (`src/routes/library/assets.rs`), handoff (`src/routes/handoff.rs`), the file list (`src/routes/files.rs:list`).
   - `tests/drafts.rs::a_draft_image_ships_only_once_a_comment_takes_it`, `tests/drafts.rs::the_handoff_prompt_leaves_draft_images_out`
7. A Manual write claims its draft image in the same step and gives it back if the page was not written
   (`src/routes/preview_tools.rs:source_edits`).
   - `tests/drafts.rs::a_manual_write_takes_its_draft_image_in_the_same_step`
   - `tests/drafts.rs::a_write_that_fails_after_the_page_is_written_keeps_its_image`
8. Opening a project sweeps draft images older than a day (`src/routes/projects.rs`).
   - `tests/drafts.rs::opening_a_project_sweeps_draft_images_a_day_old`
9. A comment follows its file renamed or moved (bookkeeping is in `od-store`).
   - `tests/drafts.rs::a_comment_sent_follows_its_file_renamed_or_moved`
10. Every request runs in a `req{request_id}` span (`src/request_id.rs:tag`, outermost layer of `router`)
    and answers with `x-od-request-id`. Work it moves to its own task keeps the span: spawn through
    `src/spawn.rs` (`spawn_in_span`, `spawn_blocking_in_span`), never `tokio::spawn`/`spawn_blocking` directly
    outside the start-up list in `tests/spawn_in_span.rs`; logs hold ids, paths and messages, never bodies, prompts or tokens.
    - `tests/spawn_in_span.rs::every_task_spawned_in_src_keeps_the_callers_span`
    - `tests/request_id.rs::a_spawned_tasks_lines_carry_the_request_id`, `tests/request_id.rs::the_spawn_helpers_keep_the_callers_span`
    - `tests/request_id.rs::every_line_of_a_failed_export_carries_its_request_id`
    - `tests/request_id.rs::a_runs_lines_carry_its_run_id_and_the_request_that_started_it`
11. The log file gets its own field formatter (`src/logs.rs:PlainFields`), never the console's.
    - same two tests (ANSI codes break `request_id=…`)
12. `od doctor` and `POST /api/doctor` share `src/doctor.rs`; a check takes its inputs as arguments
    and has a pass and a fail test; only `Status::Fail` fails the report. The route is a POST (it starts
    Chrome and the agents; a GET from an `<img>` carries no `Origin`) and runs one at a time.
    When a daemon answers, `od doctor` takes the checks from it, not from its own `OD_DATA_DIR` (QA-119).
    - `src/doctor.rs::tests`, `../od-cli/tests/cli.rs::doctor_reports_each_check_and_fails_on_any_failure`
    - `../od-cli/tests/cli.rs::doctor_reports_the_data_folder_of_the_daemon_it_asks`
    - `tests/c2.rs::the_doctor_runs_only_on_a_post_from_the_app`
13. A run that fails logs one `run failed` line in its span and becomes the last failure diagnostics lead with
    (`src/routes/runs.rs:persist_run`, `src/request_id.rs:remember_failed`).
    - `tests/request_id.rs::a_runs_lines_carry_its_run_id_and_the_request_that_started_it`
14. A project made from bundled content (Remix) gets the files its page loads, as the one-file export
    walks them (`src/routes/export/mod.rs:page_files`), not the page alone. Copying is not exporting:
    no export limits, a file it cannot follow (hidden, missing) is skipped, never a failed Remix.
    - `tests/library.rs::remix_brings_every_file_its_example_uses`, `tests/library.rs::remix_copies_only_what_the_example_loads`
    - `tests/library.rs::remix_copies_past_the_export_limits`, `tests/library.rs::remix_skips_what_it_cannot_follow`
    - `tests/library.rs::remix_keeps_an_example_that_is_not_utf8`
15. "Copy to Claude Code" writes only through the desktop app's Tauri command (`copy_skill_to_claude` →
    `src/skill_export.rs:copy_to_claude`, decision #9): no route and no `od` command writes into an agent CLI's folders,
    since an agent reaches this daemon on loopback from outside its sandbox. `GET /api/skills/{id}/export?target=claude`
    only reads; it shows the content (`SKILL.md`, every file copied) and its `hash` (`user_content::ExportFiles`).
    The copy takes that `hash` and copies only that content: a skill changed since (an agent can `PUT` it over loopback)
    is refused as `changed` with nothing written, and the bytes and permission bits compared are the ones written. Every
    file and folder is read through `od_content::nofollow` (opened from its parent's descriptor, no link, no FIFO). It takes only a Personal
    skill (no `description` is a warning), into the agents' `HOME/.claude/skills/<id>`,
    never over anything there: an entry of that name in any case or Unicode form is refused by its name on disk, the
    folder is claimed with `create_dir`, nothing hidden is written beside it, `SKILL.md` comes last, and a copy of ours cut
    short (`.contour-copy` mark, no `SKILL.md`) is replaced, whole. The warnings are `ExportFiles::check` (whole SKILL.md
    and `scripts/`, of the same bytes).
    - `tests/library.rs::no_request_copies_a_skill_to_claude_code`, `::a_personal_skill_is_copied_to_claude_code_once`,
      `::copy_to_claude_code_follows_no_link`, `::copy_to_claude_code_checks_the_whole_skill_first`,
      `::a_skill_changed_after_its_preview_is_not_copied`; e2e `../od-ui/e2e/library.e2e.mjs` (`copyToClaudeDesktop`)
    - `../od-fs/src/nofollow.rs::a_link_swapped_in_for_a_file_is_never_followed`, `::a_link_swapped_in_for_a_folder_is_never_followed`,
      `::a_fifo_swapped_in_for_a_file_never_blocks`, `::the_budget_counts_what_is_read`; `user_content.rs::a_copy_keeps_the_permission_bits_it_reviewed`
    - `../od-content/src/user_content.rs::an_export_copies_only_the_content_reviewed`, `::a_skill_with_no_description_is_copied`,
      `::an_export_names_what_is_there_and_replaces_only_its_own_unfinished_copy`,
      `::export_copies_a_user_skill_and_never_writes_over`, `::an_export_never_shows_a_hidden_folder_in_the_agents_skills`,
      `::a_folder_made_right_before_the_copy_is_kept`, `::the_skill_md_lands_after_everything_else`
    - The review marks hidden characters in `SKILL.md` (Unicode `Default_Ignorable_Code_Point` and `Bidi_Control`: tags, zero-width,
      bidi controls and marks, variation selectors, soft hyphen, Hangul fillers…) as `⟨U+…⟩` with a warning
      (`../od-ui/src/app/plugins_page.rs::the_review_marks_every_hidden_character`; the table checked against ICU's data:
      `::hidden_ranges_are_unicodes`).
    - The Library routes (`src/routes/library`), the content routes (`src/routes/content.rs`: a design template's example, a craft section) the content index (`../od-content/src`) and the memory files (`../od-store/src/memory_index.rs`: `MEMORY.md`, `config.json`) read a file only through
      `od_content::nofollow` (`O_NONBLOCK`, regular file checked on the descriptor, bounded); the index still takes an
      entry that is itself a link, but no link below it (`nofollow::read_file_under`: `.claude-plugin`). No `fs::copy` and no `od_store::files::read_file` there either (a Remix copies the bytes
      its walk read: `src/routes/export/mod.rs:read_page_file`). `tests/no_path_reads.rs` (parses the sources with `syn`: paths through `use … as`/`type`, `<File>::open`, `fcntl`/`libc` opens; what stays is in `ALLOWED` with why),
      `../od-content/tests/index_reads.rs`, `src/routes/library/design_systems.rs::package_reads_never_wait_on_a_fifo`,
      `src/routes/library/templates.rs::a_remix_reads_no_fifo_and_no_link`, `::a_remix_never_waits_on_a_side_file_swapped_for_a_fifo`,
      `tests/library.rs::design_system_routes_never_wait_on_a_fifo`, `::content_routes_never_wait_on_a_fifo`, `../od-content/src/plugin_validate.rs::a_manifest_the_index_skips_is_named`
    - `../od-cli/tests/cli.rs::od_cannot_copy_a_skill_to_claude_code`,
      `../../apps/desktop/src/main.rs::the_daemon_ui_may_call_the_app_commands`
    - The other way, "Review and replace" (From agent CLI) writes only `<data>/skills`: `GET /api/skills/{id}/source-preview`
      reads an agent folder `local-sources` lists under that id (any other path is a 400), and
      `POST /api/skills/{id}/replace-from-source` takes the `hash` shown (`ExportFiles::read_copy`: what an install copies) and is
      refused as `changed` (409) with nothing written once the folder changed. The copy in Contour moves to
      `<data>/skills/.previous/<id>` (`restore-previous` swaps it back, one level; a `.previous` that is a link is refused; `DELETE` drops it and every `.swap-<id>-*`/`.old-<id>-*` left; an install over a user skill keeps it there too; a failed replace keeps the copy kept before (set aside as `.old-*`, removed only once both land); what a replace or restore cut short left is put back, never removed, and one entry that cannot move is logged, never stops another skill); the index lists no hidden folder.
      `tests/library.rs::an_imported_skill_is_replaced_from_its_source_and_restored`;
      `../od-content/src/user_content.rs::a_replace_keeps_the_copy_in_contour_for_one_restore`,
      `::a_replace_writes_nothing_for_a_source_changed_or_renamed`, `::a_replace_follows_no_link_and_waits_on_no_fifo`,
      `::only_a_listed_agent_folder_is_taken`; `::a_replace_cut_short_at_any_step_loses_nothing`, `::a_first_replace_or_install_cut_short_at_any_step_loses_nothing`, `::a_restore_cut_short_at_any_step_loses_nothing`, `::a_replace_that_fails_keeps_the_copy_kept_before`, `::a_deleted_skill_never_comes_back_from_what_a_restore_left`, `::one_leftover_that_cannot_move_stops_nothing_else`; `../od-content/src/skills.rs::hidden_folders_are_no_skills`
16. A form that saves a user's file saves only over the copy it opened on (`src/cas.rs`): the `GET` it opens on
    answers an `ETag` (sha256 of the content shown: `GET /api/skills/{id}`, `GET /api/design-systems/{id}/kit`,
    `GET /api/memory/index`), its `PUT` sends it as `If-Match`, and a file changed since is a 412 with nothing written.
    The compare runs under the lock of the folder's `od_fs::user_store::UserRoot` (one per root path: `<data>/skills`
    for a save, replace, restore, install; the folder holding the design system (`<data>/design-systems`) for a Save of `DESIGN.md` and an accepted revision;
    `<data>/memory` for the index editor, `link`/`unlink`), and `Guard::write` compares again with what that one read
    found (`Expect`) right before its rename. A `MEMORY.md` there but unreadable (a link, a FIFO, too large) is not a
    missing one: no entry reaches a prompt and `GET /api/memory/index` says `unreadable` (Settings shows it). No `If-Match` is a 428 with nothing written
    (the extractor refuses it); a write over whatever is there sends `If-Match: *`. The `ETag` is of the bytes the body
    shown came from, never of a second read.
    GET and PUT tag the same file: for a skill, the one a save writes over (`user_content::skill_write_dir`:
    an override on disk, named `id` byte for byte, before the bundled folder; a user skill named like it in
    another case or Unicode form is a 409, never written over); a skill changed on disk since the index read it is read
    again by its `GET` for its own answer (`routes/content.rs:skill_for_form`), never stored in the shared index (a read
    may find it half written in place; a run's prompt reads the index): held still over `SETTLE`, that file (empty, or
    under another `name:`, too); written all along, the index's copy with its own tag (the Save is a 412); not a skill
    (too large, unparsable, a link: tag `unreadable`), the index's copy tagged with that file, so a Save puts a skill there
    again; gone (no `SKILL.md`), the index is read again: the bundled copy of a deleted override, a 404 for a deleted
    skill. No form is ever given the tag of nothing (`GONE_TAG`), and a Save that sends it is a 412; a `PUT` of a skill gone
    from the index is a 412 with a tag, and `*` ("Save anyway") puts it back as a user skill.
    A form's Save of `SKILL.md`, `DESIGN.md` or `MEMORY.md` goes through `Guard::write` (`../od-fs/src/user_store.rs`:
    the root and the entry under it (a skill's or design system's folder, which may be the user's own link: the index
    lists it) are followed, every folder below the entry and the file itself `O_NOFOLLOW`, temp file `O_EXCL` of a random
    name, fsync, `renameat`; one test per question of the six in `user_store_tests.rs`). The skill form's `GET` tags
    `SKILL.md` through the same `Guard::read` (`user_content::skill_tag`), so GET and PUT see one file
    (`tests/linked_entries.rs`). Any other write of one goes only through `user_content::write_replacing` (`od_content::nofollow::replace_file`:
    a new file of a random name made `O_EXCL|O_NOFOLLOW`, bits set on its descriptor, renamed over it: no half-written read,
    no write through a link at the file or at the temp name), every temp file renamed into place is made `create_new`, and a folder under the user root is named by an id only after the
    names on disk are checked (`tests/user_file_writes.rs`, `syn` scan of `od-content`, `od-server` and `../od-store/src/memory_index.rs`, `KEPT` with why). The memory index and config are written with `Guard::write` too (`../od-store/tests/memory_files.rs`).
    Every `PUT` route takes `IfMatch` itself (not `Option<IfMatch>`) or is listed in `KEPT` with why.
    A folder filled from a tree (install, install over a user skill, Replace from source, an override's first copy,
    Copy design system, a generated design system, Copy to Claude Code) is made new (`nofollow::NewDir`: `create_dir`, never
    one already there) and filled through its descriptor (`mkdirat`, `openat` `O_CREAT|O_EXCL|O_NOFOLLOW`, bits set on the
    descriptor; Windows `create_new` and no reparse point on the way): a link an agent plants in it is never written
    through. Staging names (`.installing-`, `.incoming-`, `.old-`, `.swap-`) take `nofollow::random_tag()`, never a pid
    (`user_file_writes.rs` rule `pid`); what a run cut short left is still read back by id with a pid or a tag.
    A Library delete (skill, plugin, design system: `user_content::delete_user_dir`) goes through `Guard::remove` under the
    root's lock: an entry that is the user's own link goes as a link, never the folder it points at; a folder is journaled
    (`.journal-<tag>`), renamed to a hidden `.removing-<tag>`, then removed (no link in it followed). `UserRoot::recover`
    runs once at start-up for each root (`od_content::ContentStore::load`: skills, design systems, plugins;
    `od_store::Store::open`: memory): it finishes a journaled remove, leaves an entry never renamed whole, keeps a journal
    of another kind, and removes a `Guard::write` temp file left (`.<file>.<tag>.tmp`), nothing else.
    - `../od-fs/src/user_store_tests.rs::a_remove_cut_short_at_any_step_loses_nothing_of_another_entry`,
      `::removing_a_linked_entry_removes_the_link_alone`, `::removing_a_folder_follows_no_link_in_it`,
      `::recover_removes_only_what_it_made`; `../od-content/src/user_content.rs::deleting_a_linked_skill_removes_the_link_alone`,
      `../od-content/src/index.rs::loading_finishes_what_a_delete_or_save_cut_short_left`,
      `../od-store/tests/memory_files.rs::opening_the_store_removes_a_temp_file_a_cut_short_save_left`
    "Save anyway" (`*`) on an override deleted on disk while the index still lists it as the user's makes it again from
    the bundled skill (`routes/library/skills.rs:save`), `od:` block and side files kept, never a `SKILL.md` alone.
    - `tests/form_cas.rs::save_anyway_over_a_deleted_override_makes_it_again_from_the_bundled_skill`
    No half-copied folder is ever left where an index lists it: `copy_dir` removes the folder it made when a write
    fails, and an override's first copy and Copy design system are built in a hidden `.copying-<name>-<tag>`
    (`user_content::copy_dir_into_place`), finished there, then renamed into place; something already at the name is a
    409 named by its folder name.
    - `../od-content/tests/planted_links.rs`, `../od-fs/src/nofollow.rs::a_new_dir_writes_through_no_link_planted_in_it`,
      `::a_new_dir_is_new`, `../od-content/src/user_content.rs::leftovers_are_read_by_id_with_a_random_tag_or_a_pid`,
      `::an_install_cut_short_with_a_random_tag_is_put_back`,
      `../od-content/src/design_system_gen.rs::a_link_planted_in_the_staging_folder_is_never_written_through`,
      `src/doctor.rs::the_data_folder_check_never_writes_through_a_planted_link`, `::the_probe_file_is_made_new`,
      `../od-fs/src/nofollow.rs::a_new_dir_swapped_for_a_link_before_it_is_opened_is_refused`,
      `../od-content/src/user_content.rs::a_copy_into_place_is_whole_or_not_there`, `::a_copy_cut_short_removes_its_folder`
    - `tests/put_reads_if_match.rs` (source scan with `syn`), `tests/form_cas.rs` (412 → GET again → 200, and
      24 saves at once from one copy land once, per lock; `a_skill_read_while_written_in_place_leaves_the_index_whole`,
      `a_bundled_skill_saved_beside_one_named_alike_writes_nothing`), `src/cas.rs::tests`
    - `src/routes/library/design_systems.rs::an_accept_waits_for_a_save_of_design_md` (the lock in `decide`),
      `src/routes/content.rs::tests` (`skill_for_form`, `steady_read`), `tests/form_cas.rs::a_skill_md_left_empty_opens_the_form_on_it`,
      `::an_override_named_otherwise_is_shown_and_kept`, `tests/user_file_writes.rs`,
      `../od-content/src/user_content.rs::a_save_never_lands_in_a_skill_named_alike`, `::a_save_replaces_a_link_and_writes_nothing_through_it`
    - e2e `../od-ui/e2e/library.e2e.mjs` (skill form, DESIGN.md editor), `../od-ui/e2e/c2.e2e.mjs` (memory index)

## Traps
- A key owned by a dedicated route must be stripped from the generic PUT (#227).
- A shallow replace of `agentModels`, `byok` or `notifications` lost another window's entries (#244).
- A patch holding only `byok.model` is not a route change; validate the merged result (#244).
- Without `savedRoute` the server cannot tell "this window moved the host" from "this window is stale" (#246).
- A list setting under RFC 7396 replaces whole; give it its own locked route (#246).
- `PUT /api/media/config` still replaces its whole map: separate store, left as is in #246.
- A new listing or export that ships files must filter draft images; handoff was missed once (#200).
- A `tokio::spawn` or `spawn_blocking` drops the request's span: use `crate::spawn`, or its lines lose the id (the static test refuses it).
- Two fmt layers with the same field formatter share a span's formatted fields; the file got the console's colors.
- Remix copied only `example.html`; 10 templates load `assets/…` (#286). A new route that copies a bundled page uses `page_files`.
- A GET that does more than read (starts a process, writes a file) can be set off by an `<img>` with no `Origin`; make it a POST (#271).

## Check
- `make check`; single test: `cargo test -p od-server --test drafts`.
