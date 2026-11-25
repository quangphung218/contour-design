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
15. "Copy to Claude Code" (`POST /api/skills/{id}/export`) copies only a Personal skill that has a `description` into the
    agents' `HOME/.claude/skills/<id>` (the `HOME` "From agent CLI" reads) and never writes over anything there: an entry
    of that name in any case or Unicode form (`name_key`, folder, file or link) is a 409 naming that folder, and the folder
    is claimed with `create_dir` (fails if another program made it). Nothing hidden is written there (Claude Code loads
    hidden folders as skills); `SKILL.md` is copied last; links are not followed (`od-content` `user_content::export_skill`).
    The warning (`contourTools`) is decided once, on the server, from the whole SKILL.md and `scripts/`
    (`user_content::export_check`); the app reads it from `GET …/export?target=claude`.
    - `tests/library.rs::a_personal_skill_is_copied_to_claude_code_once`, `::copy_to_claude_code_follows_no_link`,
      `::copy_to_claude_code_checks_the_whole_skill_first`
    - `../od-content/src/user_content.rs::export_copies_a_user_skill_and_never_writes_over`, `::export_follows_no_link`,
      `::an_export_never_shows_a_hidden_folder_in_the_agents_skills`, `::a_folder_made_elsewhere_during_an_export_is_not_replaced`,
      `::the_export_check_reads_the_whole_skill`
    - `../od-cli/src/lib.rs::the_contour_tools_warning_knows_every_od_command`

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
