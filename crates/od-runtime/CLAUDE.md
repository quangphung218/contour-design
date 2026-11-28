# od-runtime

Runs an agent CLI (or a BYOK provider) for one turn and turns its output into `AgentEvent`s.
Map: `../../docs/architecture.md`. Paths below are relative to this folder.

## Owns
- Agent definitions as data (`src/def.rs:RuntimeDef`) and detection on `PATH` (`src/detect.rs:detect_all_in`),
  plus the login-shell `PATH` (`src/login_path.rs:login_shell_path`).
- The run engine (`src/engine.rs:RunEngine`): spawn, parse, steer, cancel, idle timeout, retry, resume fallback.
- Process groups and tree kill (`src/process.rs`), the agent env (`src/env.rs`), skill staging (`src/stage.rs`),
  the tool-loop guard (`src/guard.rs:ToolLoopGuard`), BYOK (`src/byok.rs`).
- Stream parsers, one file per format under `src/parse/`.

## Entry points
- `src/engine.rs:RunEngine::launch` → `drive` → `attempt` (one process lifetime) → `src/process.rs:spawn`;
  `attempt_byok` for API-key runs. Cancel: `RunEngine::cancel`, `cancel_all`. Steer: `RunEngine::steer`.
- Config: `EngineConfig::from_env` + `with_login_path`. Caller: `../od-server/src/routes/runs.rs`.
- Parsers: `src/parse/mod.rs:parser_for` returns a `StreamParser`. A new format needs a parser file, a `parser_for` arm and a golden recording.

## Invariants
1. The agent runs in its own process group (Windows: new process group + a Job Object per child);
   cancel and shutdown end the whole group (`src/process.rs:terminate_group`, grace `KILL_GRACE`).
   - `tests/engine.rs::processes_left_in_the_group_die_with_the_run`
2. A group left by a crashed daemon is killed only if its leader still runs the recorded binary
   (`src/process.rs:reap_orphan`).
   - `tests/engine.rs::daemon_restart_mid_run_ends_it_failed_and_reaps_the_orphan`
   - `tests/engine.rs::reap_orphan_spares_a_group_running_something_else`
3. The agent env is the daemon's env minus `PATH` and every `OD_*`; then `PATH` and the run's own
   values are set (`src/env.rs:agent_env`). `OD_API_TOKEN` never reaches the agent.
   `src/process.rs:spawn` sets `PWD` to the run's cwd: OpenCode 2 works in `$PWD`, not the process cwd (QA-116).
   - `src/env.rs::api_token_never_reaches_the_agent`
   - `tests/engine.rs::the_agent_sees_its_cwd_in_pwd`
4. Protocol agents get their cancel message before any signal.
   - `tests/engine_m6.rs::acp_cancel_sends_session_cancel_before_any_signal`
   - `tests/engine_m6.rs::dsh_cancel_is_sent_on_the_open_stdin_before_any_signal`
5. A staging folder (`.od-skills`) that is a link is replaced by a real folder, never written through
   (`src/stage.rs:stage_skill`).
   - `src/stage.rs::a_staging_folder_that_is_a_link_is_replaced_not_written_through`
6. Each parser matches its recorded golden event stream.
   - `tests/golden.rs::claude_recordings_match_golden_event_streams`, `tests/golden.rs::codex_exec_json_matches_golden_event_stream`
   - more in `tests/golden_m6.rs`, `tests/golden_c3.rs`; fixtures in `tests/fixtures/`
7. A run's task runs in a `run{run_id, agent, project}` span under the span of whoever launched it
   (`src/engine.rs:RunEngine::launch`), so its lines carry the request id too. Every run logs
   `run started` and either `run failed` or `run ended` there, whatever the agent (QA-117).
   - `../od-server/tests/request_id.rs::a_runs_lines_carry_its_run_id_and_the_request_that_started_it`
   - `../od-server/tests/request_id.rs::every_run_logs_its_start_and_end_in_its_span`
8. A file-writing tool call is named while its arguments stream: `src/parse/tool_path.rs:ToolPathScanner`
   reads the path from the first bytes (only a complete one, top-level key, write tools only), the Claude
   parser sends `tool_in_flight` then once per KiB, and the engine lets at most four a second through per
   call (`src/in_flight.rs:InFlightGate`). Live only: the server does not store it (`persisted`).
   - `tests/in_flight.rs::a_10_kb_write_names_its_file_within_the_first_kilobyte`, `src/parse/tool_path.rs::tests`
   - `tests/engine.rs::tool_in_flight_goes_out_at_most_four_times_a_second`
   - `../od-server/tests/runs.rs::recorded_claude_stream_reaches_sse_as_the_golden_events`

## Traps
- An agent's shell can turn any project path into a link; writing through it changes files outside the project (#206).
- An ACP agent that says `loadSession: false` (Gemini CLI 0.24) answers `session/load` with "Method not found";
  `AcpSession` opens `session/new` with the full transcript instead (#280,
  `tests/engine_m6.rs::acp_agent_without_load_session_resumes_with_the_transcript`).

## Check
- `cargo test -p od-runtime`. After an intended parser change: `OD_UPDATE_GOLDEN=1 cargo test -p od-runtime --test golden` rewrites the `.events.jsonl` files.
