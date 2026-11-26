# od-ui

Leptos 0.8 CSR app compiled to WASM, served by the daemon. Map: `../../docs/architecture.md`.
Paths below are relative to this folder.

## Owns
- Every screen: Home, Studio (chat, preview, files, comments, Manual edit), Settings, Library.
- Client state: the shared config copy (`Shell.config`), unsent drafts, window turns.
- Talks to the daemon over `/api/*` and SSE only (`src/app/api.rs`), and to Tauri via `src/app/desktop.rs`.

## Entry points
- Shell and config: `src/app/mod.rs:Shell::save_config`, `Shell::reload_config`, `Shell::add_recent_dir`.
- Studio: `src/app/studio.rs:StudioCtx::send`, `StudioCtx::attach`, `StudioCtx::follow_moves`.
- Tasks: `src/app/tasks.rs:Tasks::spawn_then`, `detached`.
- Leave guard: `src/app/leave_guard.rs:use_leave`, `use_open_after_click`, `leave_then`, `guard_leaving`.
- Comment drafts: `src/app/preview_comments.rs:CommentDraft`; draft sessions: `src/app/drafts.rs`.

## Invariants
1. Work the user asked for (send, save, upload, delete) runs to its end after the screen goes.
   Only the UI part is guarded. Raw spawners are banned in `clippy.toml` (`disallowed-methods`).
   - `src/app/tasks.rs::work_runs_to_its_end_after_the_screen_went_and_ui_does_not`
   - `tests/task_kinds.rs` (source scan: a write a `spawn` task reaches only after another `await`;
     a site that may stay is listed in `KEPT` with the tasks that spawn it and why)
   - e2e `e2e/qa/leave.qa.mjs:"LV-08"`, `"LV-19"`, `"LV-20"`
2. A config save sends only the changed leaves: `next.patch_since(base)`, a dropped key as `null`, arrays whole.
   Never send `to_patch()` (`src/settings.rs:AppConfig::patch_since`).
   - `src/settings.rs::patch_since_sends_only_changed_keys`, `src/settings.rs::patch_since_clears_dropped_keys`
   - `src/settings.rs::patch_since_sends_only_the_changed_leaves_of_an_object`; e2e `e2e/qa/settings.qa.mjs:"SET-12"`
3. Build `next` from `shell.config` right before the call and change only your own fields.
   `save_config` puts `next` in the copy before the PUT; a failed PUT reloads from the daemon.
   - e2e `e2e/qa/settings.qa.mjs:"SET-15"`, `e2e/qa/settings.qa.mjs:"SET-18"`
4. Config requests take a turn when called and send in call order; only the last turn writes the copy
   (`src/turns.rs:Turn::is_last`, `src/app/mod.rs:Shell::keep_config`).
   - `src/turns.rs::a_reload_waits_for_the_save_before_it`, `src/turns.rs::a_turn_dropped_before_it_came_is_skipped`
5. Every way out that would drop a draft asks the leave guard: `use_leave` for routes,
   `leave_then` for full loads and menus. "Unsent" counts an image still uploading (`CommentDraft::unsent`).
   - `src/app/preview_comments.rs::an_image_on_its_way_is_work_not_sent`
   - e2e `e2e/qa/drafts.qa.mjs:"DL-14"`, `e2e/qa/drafts.qa.mjs:"DL-31"`
   A handler (click, key, pick) opens a page with `use_open_after_click`, on the next task: leaving inside
   the event drops listeners it still passes through (#332).
   - `tests/click_navigate.rs` (source scan: every `navigate(` call listed in `KEPT` with why); e2e `e2e/routes.e2e.mjs`
   No element listens for an event that an element inside it listens for too: a handler that closes the outer one
   drops its listener while the event still bubbles to it. An overlay closes on a `.modal-backdrop` beside the dialog.
   - `tests/nested_listeners.rs` (source scan of every `view!`, through components; pairs that stay in `KEPT` with why)
   - e2e `e2e/routes.e2e.mjs` (dialog Cancel, ×, backdrop), Stay in `e2e/preview.e2e.mjs`, Leave in `e2e/c2.e2e.mjs`
6. A comment draft is found by `id`, never by path. Sends and upload landings look it up by id or token
   (`src/app/preview_comments.rs:draft_by_id`, `land_upload`).
   - `src/app/preview_comments.rs::a_draft_renamed_while_it_is_sent_goes_when_it_is_saved`
   - `src/app/preview_comments.rs::an_image_lands_in_its_draft_whatever_its_file_is_called_now`
7. A rename or move updates all path-keyed state in one handler (`StudioCtx::follow_moves`):
   drafts and their images, Manual edit `kept`, tabs, slide, attachments. A path being moved is not gone.
   - `src/app/preview_comments.rs::a_draft_follows_its_file_renamed_or_moved`
   - `src/app/drafts.rs::a_path_being_moved_is_not_gone`, `src/workspace.rs::a_rename_keeps_the_tab_in_place_and_active`
   - e2e `e2e/qa/filemoves.qa.mjs` (MV-01 to MV-16)
8. A draft session is released on every page exit (Studio cleanup, `pagehide`, desktop quit),
   and the release waits for sends in flight (`src/app/drafts.rs:Sends`).
   - `src/app/drafts.rs::a_release_waits_for_the_comments_being_sent`; e2e `e2e/qa/drafts.qa.mjs:"DL-38"`
9. A deck draft keeps the slide it was picked on (`src/app/preview_comments.rs:draft_slide_shown`).
   - `src/app/preview_comments.rs::the_draft_keeps_the_slide_it_was_picked_on`
10. A late Manual edit write, read or upload lands on its own file after a switch.
    - `src/app/manual_edit.rs::a_write_that_lands_after_a_switch_cleans_its_own_file_only`
11. Only the composer's Send (and a Retry of the user's own message) takes the draft's files, attached comments
    and @ chips (`StudioCtx::send`). A turn a button writes goes through `StudioCtx::send_prepared`; the store keeps
    its request (`Message.prepared_message`) so a Retry after a reload sends it again, still without the draft.
    - `tests/prepared_turns.rs` (source scan: every `.send(…)`/`send_at(…)` listed in `KEPT` with why)
    - `src/app/chat_view.rs::a_retry_sends_the_request_and_shows_the_name_again`; e2e `e2e/library.e2e.mjs` (Save as skill)

## Traps
- Reading a disposed signal after an `await` panics; `Tasks::spawn` drops all work after its first await,
  so a save behind the config turn never went out (#247).
- A save or install that comes back late (`spawn_then` ui part) may only close a dialog or open its result if the user is still where they started it: check a token taken at the start (e.g. `visit` in `plugins_page.rs` CreateDialog), not the current mode, or a late result closes what is typed since (#320).
- A read that lands while a form is open and sets the signal the form is built from rebuilds it from disk: what is typed goes. Keep the button that opens the form off while a read or a write of that skill runs (`plugins_page.rs` SkillDetailView `reading`, `restoring`), so the form never opens on a copy about to change (#344).
- A form that edits a user's file opens on `api::get_tagged` and saves with `api::put_if_match` (the tag it opened on);
  a 412 (`HttpError::changed_since`) keeps what is typed and shows `lib_ui::ChangedElsewhere` (Reload / Save anyway).
  A GET-then-PUT from the page left a window between the two and wrote anyway when the read failed (#347 review).
- Sending the whole config copy put back another window's theme, agent and locations (#227, #241).
- Two saves in a row from one window: the second diffed against an old copy and sent `{}` (#241).
- Removing a key needs an explicit `null`; leaving it out of a merge patch keeps it (#241, #244).
- An older GET or PUT answer overwrote a newer copy; hence turns (#246).
- Keying a per-file draft by path loses it on the next rename (#189, #198).
- A new exit (menu item, toast link, restart, Open Folder) skips the guard unless it uses `leave_then` (#193).
- Release racing a send deleted the images the comment was saving (#198).
- The file watcher can list a renamed path before the rename answers; `StudioCtx.moving` covers it (#198).
- Deck live reload passes through slide 0; read the restore target, not the live slide (#189).

## Check
- `make check`; e2e: run only the specs you touched (`e2e/`). QA suites: `e2e/qa/run.sh settings`.
