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
   Server code writes app config only through `patch_config`, never the file directly.
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
   Library assets (`src/routes/library/assets.rs`), handoff (`src/routes/handoff.rs`). `GET files` lists them on purpose.
   - `tests/drafts.rs::a_draft_image_ships_only_once_a_comment_takes_it`, `tests/drafts.rs::the_handoff_prompt_leaves_draft_images_out`
7. A Manual write claims its draft image in the same step and gives it back if the page was not written
   (`src/routes/preview_tools.rs:source_edits`).
   - `tests/drafts.rs::a_manual_write_takes_its_draft_image_in_the_same_step`
   - `tests/drafts.rs::a_write_that_fails_after_the_page_is_written_keeps_its_image`
8. Opening a project sweeps draft images older than a day (`src/routes/projects.rs`).
   - `tests/drafts.rs::opening_a_project_sweeps_draft_images_a_day_old`
9. A comment follows its file renamed or moved (bookkeeping is in `od-store`).
   - `tests/drafts.rs::a_comment_sent_follows_its_file_renamed_or_moved`

## Traps
- A key owned by a dedicated route must be stripped from the generic PUT (#227).
- A shallow replace of `agentModels`, `byok` or `notifications` lost another window's entries (#244).
- A patch holding only `byok.model` is not a route change; validate the merged result (#244).
- Without `savedRoute` the server cannot tell "this window moved the host" from "this window is stale" (#246).
- A list setting under RFC 7396 replaces whole; give it its own locked route (#246).
- `PUT /api/media/config` still replaces its whole map: separate store, left as is in #246.
- A new listing or export that ships files must filter draft images; handoff was missed once (#200).

## Check
- `make check`; single test: `cargo test -p od-server --test drafts`.
