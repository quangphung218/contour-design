# Design decisions

Settled 2026-10-22 (Atlat, with full authority delegated by the owner). Answers the 4 open questions in `spec.md` §5.6, plus general constraints.

| # | Question | Decision | Why |
|---|---|---|---|
| 1 | Agent permissions | Default matches the original app: the agent self-approves (`bypassPermissions` for Claude, the equivalent flag for other CLIs), cwd = project folder. Settings has a switch `agent_permission_mode = auto | ask` (ask: Claude uses `acceptEdits`, other CLIs drop the self-approve flag). | Local single-user app; running without answering every command is the core experience. The switch lets users tighten it when needed. |
| 2 | Chrome for export | Use only the Chrome/Edge/Chromium already installed (found via standard paths + `OD_CHROME_PATH`). Never auto-download Chromium. Without one, PDF falls back to webview printing and screenshots fail with a clear "Chrome required" error. | No hidden outbound network; the owner's machine already has Chrome. |
| 3 | Brand-named design systems (revised 2026-10-25, owner) | Keep all 152 bundled design systems with their names, attribution and nested LICENSE files. The 90 named after a real company or product state it as `brand` in `manifest.json`; the app shows "Inspired by <Brand>'s public design language. Not affiliated with or endorsed by <Brand>. Trademarks belong to their owners." on their page in every UI language, and `NOTICE` and the README say the same. | The 2026-10-22 decision (ship as is) rested on "local app, not publicly released"; that no longer holds, since Contour ships as a public macOS beta. The brand names are what make the presets useful, so the owner chose a clear non-affiliation note over renaming or removing them. |
| 4 | Prompt set | Legacy set only (`apps/daemon/src/prompts/` of the original app), rewritten in Rust. No rollout/A-B. | Clear structure, all in one place. |
| 5 | Product name (2026-10-24, owner) | The app is **Contour** (was "Nét"): UI in every locale, window and menus, CLI help, bundle `Contour.app`, identifier `dev.contour.desktop`, data folder `contour` under the platform data dir. The first run moves an existing `open-design-rs` data folder to `contour` and leaves a symlink at the old path (Unix). Crates and binaries keep their `od-*` / `od` names, and env vars keep the `OD_` prefix. | A global name that reads the same in every language. Renaming 20 crates, the `od` binary and the `OD_*` variables would touch every import, script, MCP config and agent prompt for no user-visible gain, and would break installs that already call `od`. |

## General constraints
- **No login, no cloud, no telemetry, no outbound network** except calls to the agent/provider the user configures. E2e tests block every request outside loopback.
- **Stack**: Cargo workspace, edition 2024, tokio, axum, rusqlite (bundled), Leptos CSR (UI), Tauri 2 (shell). Crate layout per `spec.md` §5.2.
- **Performance**: follow the target table in `spec.md` §5.5; each milestone has benches for the parts it touches.
- **Rust code** follows the `rust-skills` skill (`.claude/skills/rust-skills`).
- **16 GB dev machine**: `.cargo/config.toml` caps `jobs = 4`; heavy tests run serially.
- **Content** (skills, design-templates, design-systems, craft…) is copied from the original app per the table in `spec.md` §3.7, keeping every nested LICENSE; prompt templates with unclear licenses and unlicensed fonts are excluded.
- **Repository**: `quangphung218/contour-design` on GitHub (private). Work merges into `main` after Atlat's review gate; Atlat creates the remote and pushes.
