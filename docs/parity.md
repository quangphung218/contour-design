# Contour parity with OpenDesign — last measured on main `f4c455b`, updated on branch `final-polish`

> **`final-polish` update (2026-10-24): 100%.** The last two rows are done: #119 Export JSX (preview renders `.jsx/.tsx`, plus "Export React component as HTML", compiled in-page with bundled React 18.3.1 + Sucrase, no CDN) is now 100; #177 BYOK presets reach 26/26 with Azure OpenAI (protocol `azure`: resource endpoint, deployment, `api-version`, key in `api-key`). Evidence is in those two rows and in the "Wave C live run results" section. Everything below keeps the figures measured at `f4c455b`, except the summary table, the "Quick read" section and those two rows.

- Rust version: `main` HEAD `f4c455b` (merge `waveC-integrate`, 2026-10-24): wave C comprises C1-preview, C2-chat-home, C3-runtimes, C4-depth.
- Two baselines: **40.7%** (`docs/parity.md` at `53f04bc`, 215 scored rows) and **90.9%** (version `cf5effa` measured at `c4fb1c7` after wave B, 214 scored rows). Same catalogue, same scoring, same row order.
- Original app: `~/Documents/GitHub/open-design` (read-only).
- Scoring: **100** complete, **50** partial (notes what is missing), **0** absent, **N/A** deliberately dropped. A group's % is the mean score, excluding N/A. Every row has equal weight. Available only via API/CLI, not in the UI: 50.
- Catalogue changes: per the settled decisions (Connectors, Orbit, Figma, marketplace, pet, browser tab, Web Clipper, xAI OAuth, auto-update are N/A), **7 rows became N/A**: #18 Pet, #43 Figma import, #108 Browser tab, #142 Registry/marketplace, #191 Web Clipper, #199 Orbit, #209 Pet window. Connectors (#185) and auto-update (#210) were already N/A. Rows #3 (action chips) and #14 (entry tabs) are partly Figma/Connectors: that part is dropped and the row is scored on the rest. Hence "Now" scores **207 rows**, versus 214 in wave B.
- Live run: debug `od` daemon (`~/.cache/odrs-target-open-design-rs/debug/od`, built 08:52 from `f4c455b`, `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2`) + UI bundle built with `trunk build --dist` into the run directory + mock agent, with a temporary data dir that each scenario creates and deletes. Ran the repo's 10 Playwright scenarios (exactly as `crates/od-ui/e2e/run.sh`) from a copy outside the repo, sequentially: studio, design-system, library, manage, chat, files, preview, c2, c3-runtimes, c4-depth. **10/10 PASS**, `externalRequests: 0` in every scenario that counts it. 161 screenshots (light and dark) in `shots/`, logs in `logs/`.
- Evidence marked `e2e <scenario> step N` was actually run in this measurement; the rest is code reading (file:line at `f4c455b`). Line numbers for rows whose score did not change were mapped automatically from `c4fb1c7` to `f4c455b` via `git diff` hunks (114 rows had shifted line numbers); 22 spots were checked by hand.

## Summary

| Group | Scored rows | Complete | Partial | Absent | N/A | 40.7% (`53f04bc`) | 90.9% (wave B) | Now | Gain vs wave B |
|---|---|---|---|---|---|---|---|---|---|
| Projects | 14 | 14 | 0 | 0 | 1 | 32 | 100 | 100 | 0 |
| Design systems | 13 | 13 | 0 | 0 | 1 | 19 | 100 | 100 | 0 |
| Templates/Examples | 6 | 6 | 0 | 0 | 1 | 50 | 100 | 100 | 0 |
| MCP | 7 | 7 | 0 | 0 | 1 | 44 | 100 | 100 | 0 |
| Shortcuts | 17 | 17 | 0 | 0 | 0 | 44 | 100 | 100 | 0 |
| Studio-preview | 26 | 26 | 0 | 0 | 1 | 60 | 94 | 100 | +6 |
| Studio-files/tabs | 15 | 15 | 0 | 0 | 1 | 25 | 94 | 100 | +6 |
| Studio-chat | 31 | 31 | 0 | 0 | 2 | 44 | 88 | 100 | +12 |
| Plugins/Skills | 7 | 7 | 0 | 0 | 2 | 12 | 88 | 100 | +12 |
| Agent runtimes | 8 | 8 | 0 | 0 | 1 | 69 | 88 | 100 | +12 |
| Settings | 15 | 15 | 0 | 0 | 1 | 37 | 87 | 100 | +13 |
| Desktop shell | 8 | 8 | 0 | 0 | 2 | 56 | 83 | 100 | +17 |
| Automations | 7 | 7 | 0 | 0 | 1 | 38 | 81 | 100 | +19 |
| Home | 17 | 17 | 0 | 0 | 3 | 39 | 81 | 100 | +19 |
| Media | 5 | 5 | 0 | 0 | 1 | 17 | 75 | 100 | +25 |
| Export | 11 | 11 | 0 | 0 | 1 | 45 | 91 | 100 | +9 |
| **Total** | **207** | **207** | **0** | **0** | **20** | **40.7** | **90.9** | **100** | **+9.1** |

Quick read:
- **Up from 90.9% to 100%** (207/207 scored rows at 100). 16/16 groups at 100%. At `f4c455b` it was 99.8%; `final-polish` finished #119.
- Like-for-like: dropping the 7 newly N/A rows from the wave B version puts wave B at 93.7% (19,400/207). The real gain from wave C code is **+6.1 points**; the rest comes from settling N/A.
- **No row is below 100 any more.** #119 Export JSX has the full "Export React component as HTML", and preview renders the component as the original app does.
- Row #177 (BYOK presets) is now 26/26: Azure OpenAI was added, so its 100 is no longer generous scoring.
- 5/6 "new rows" outside the catalogue are now at 100; xAI OAuth is N/A.

## Rows still below 100

| # | Row | Score | Missing | Now | Notes |
|---|---|---|---|---|---|
| — | None left | — | — | — | #119 and #177 done on `final-polish` (see those two rows) |

## New rows (in the original app, missing from the old catalogue; not counted in %)

| Group | Feature (original app) | Wave B | Now | Rust version at `f4c455b` |
|---|---|---|---|---|
| Studio-preview | One-time hint on the first artifact (`FirstArtifactHint.tsx`) | 0 | 100 | `crates/od-ui/src/app/first_hint.rs:61-71`; e2e preview step 1; `shots/c1/first-artifact-hint-light.png` |
| Design systems | Missing brand font files banner (`MissingBrandFontsBanner.tsx`) | 0 | 100 | `crates/od-ui/src/app/font_banner.rs:33-42` (Upload, Keep), `crates/od-ui/src/brand_fonts.rs`; e2e c2 step 11; `shots/c2-font-banner-light.png` |
| Studio-chat | Toast when memory is saved (`MemoryToast.tsx`) | 0 | 100 | `crates/od-ui/src/app/memory_settings.rs:391`, `crates/od-ui/src/memory_toast.rs`; e2e c2 step 5; `shots/c2-memory-toast-light.png` |
| Agent runtimes | DeepSeek Harness setup dialog (`DeepSeekHarnessSetupDialog.tsx`) | 50 | 100 | `crates/od-ui/src/app/dsh_setup.rs` `dsh` install command, `open-design` profile, bundled guide, re-detection via `POST /api/agents/:id/detect`; does not bundle `@open-design/dsh-runtime` (deliberate difference). e2e c3 steps 2, 6; `shots/c3/shots/dsh-setup-dialog-light.png` |
| Settings | xAI OAuth sign-in (`XaiOAuthControl.tsx`) | 0 | N/A | xAI OAuth: N/A (settled decision); the key-based xAI preset is still there |
| Studio | Project workspace recovery tip (`ProjectWorkspaceRecoveryTip.tsx`) | 0 | 100 | `crates/od-ui/src/app/recovery_tip.rs:31`; e2e preview step 5; `shots/c1/workspace-recovery-tip-light.png` |

## Wave C live run results

| Wave C item | Scenario, step | Result | Screenshots |
|---|---|---|---|
| Manual edit | preview 3 | PASS: content, link, style, attributes, html, image upload; delete; undo/redo (an undo that would overwrite a newer change is rejected with 409) | `shots/c1/manual-edit-*` |
| Comments tab | preview 4 | PASS: list, images, anchored/stale/lost, resolve/reopen, images attached to the turn | `shots/c1/comments-*` |
| Code highlighting | preview 2 | PASS: HTML+CSS+JS, CSS, JS, Markdown | `shots/c1/code-view-*` |
| Library in the composer | c2 3–4 | PASS: "+" on Home, "@" and "+" in Studio | `shots/c2-library-picker-*` |
| Onboarding brand extraction | c2 1 | PASS | `shots/c2-onboarding-brand-*` |
| Memory (Settings, toast) | c2 5, 9 | PASS: hooks, import, MEMORY.md, master switch; toast | `shots/c2-settings-memory-*`, `shots/c2-memory-toast-*` |
| Jury Interrupt/Replay | c2 7 | PASS | `shots/c2-jury-*` |
| Media progress row | c2 6 | PASS: 2 images, 1 error with Retry | `shots/c2-media-row-*` |
| Help menu / Quick menu | c2 10 | PASS (web build); native Tauri menu by code reading only | `shots/c2-quick-menu-*`, `shots/c2-docs-*` |
| Export JSX | c2 8 | PASS (downloaded `.tsx` file matches the source) | — |
| Export JSX: preview + HTML (`final-polish`) | c2 8 | See the Verification section of the `final-polish` builder report | `shots/c2-jsx-preview-*` |
| BYOK Azure OpenAI (`final-polish`) | manage 1 | See the Verification section of the `final-polish` builder report | — |
| New runtimes | c3 1, 3 | PASS: new agents detected; one Qoder turn yields thinking + text | `shots/c3/shots/agent-needs-setup-*` |
| Media provider | c3 4–5 | PASS: every new provider has a card; research via Tavily (mock) | `shots/c3/shots/settings-media-providers-*` |
| DeepSeek dialog | c3 2, 6 | PASS: dialog; guide opens before onboarding is finished | `shots/c3/shots/dsh-setup-*` |
| 20 languages | c4 1–2 | PASS: picker with 20 entries; 19 languages × 12 screens with no layout overflow; ar/fa RTL; browser pt-PT → pt-BR | `shots/c4/settings-language-de-*`, `shots/c4/home-ar-*`, `shots/c4/automations-ja-*` |
| Crystallize | c4 3 | PASS: crystallize one run, Apply a proposal | `shots/c4/automations-proposals-*` |
| GenUI | c4 4–5 | PASS: 4 surfaces, answers remembered for the next turn | `shots/c4/chat-genui-*` |

The first c4-depth run FAILED at step 1 ("timed out waiting for German Home link") because the copy outside the repo lacked the `../locales/` directory the script reads (`c4-depth.e2e.mjs:58`). Not an app bug: with the locales directory placed next to the copy it PASSES. Failed-run log: `logs/c4-depth.run1-fail.log`, screenshot `shots/c4-run1-failure-missing-locales.png`.

## Full table

| # | Group | Feature (original app) | 40.7% | 90.9% | Now | Evidence (HEAD f4c455b) / what is missing |
|---|---|---|---|---|---|---|
| 1 | Home | Brief box; Enter creates the project and sends the first turn | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:412` (create button), Enter sends the first turn; e2e chat step 6, studio step 2 |
| 2 | Home | Output-type chips (Prototype, Website clone, Wireframe, Mobile app, Slide deck, Document, Image, Video, Audio, Motion, HyperFrames, WebGL, Live artifact) | 50 | 100 | 100 | 13 chips `crates/od-ui/src/intent.rs:13`; `crates/od-ui/src/app/home.rs:522,540`; e2e chat step 3; `(audit run) shots/b3-home-light.png` |
| 3 | Home | Action chips (Create design system, Create plugin, From Figma, From folder, From template) | 50 | 50 | 100 | Create design system, Create plugin (`crates/od-ui/src/app/home.rs:436`), From folder (`home.rs:850`), template; From Figma **N/A** (Figma settled as N/A), so the row is scored on the rest |
| 4 | Home | Pick a design system at creation | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:353` |
| 5 | Home | Pick a template + "Start from a template" gallery | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:382,975`; e2e studio step 2b |
| 6 | Home | Rotating headline, placeholder carousel, prompt examples | 0 | 100 | 100 | `crates/od-ui/src/app/home.rs:308` rotating headline, `intent.rs:307` placeholder script, prompt examples `home.rs:681`; e2e chat step 2 |
| 7 | Home | "+" menu to add context (attach file, plugin, connector, MCP) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:1005` PlusMenu (attach file, plugin, skill, MCP, folder); connector N/A; e2e chat steps 4–6 |
| 8 | Home | Context search with @ (file, plugin, skill, MCP) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:717` @ palette, `crates/od-ui/src/mention.rs`; e2e chat step 5 |
| 9 | Home | Media picker at the composer footer (Ratio, Duration, Resolution, Notes) | 0 | 100 | 100 | `crates/od-ui/src/app/home.rs:609` MediaBar (model, ratio, resolution, duration, notes `home.rs:647`); e2e chat step 3 |
| 10 | Home | Working directory picker (choose, change, recent folders) | 0 | 100 | 100 | `crates/od-ui/src/app/home.rs:456,780-817` choose/change/recent/clear; e2e chat step 4 |
| 11 | Home | "Recommended for you" card | 0 | 0 | 100 | `crates/od-ui/src/app/home_tabs.rs:303-322` Recommended (Start, Next, Dismiss), mounted at `home.rs:471`; e2e c2 step 2; `shots/c2-home-recommended-light.png` |
| 12 | Home | Recent projects strip (+ owner/type filter) | 50 | 100 | 100 | `crates/od-ui/src/app/projects.rs:684-688` recent strip + type filter; owner filter does not apply since there are no teams (N/A); e2e manage step 6; `(audit run) shots/manage/shots/home-recent-light.png` |
| 13 | Home | Left rail (Home, Projects, Automations, Plugins, Design systems, Integrations, collapse) | 50 | 100 | 100 | `crates/od-ui/src/app/mod.rs:460-473`: Home, Projects, Templates, Design systems, Plugins, Media, Automations, Integrations, Inbox, Settings + collapse |
| 14 | Home | Entry tabs (Designs, Templates, Design systems, Connectors, Image/Video templates) | 50 | 50 | 100 | `crates/od-ui/src/app/home_tabs.rs:19-25,103-114` tabs Designs, Templates, Design systems, Image, Video (tab remembered); Connectors N/A; e2e c2 step 2 |
| 15 | Home | Message center (in-app notifications) | 0 | 100 | 100 | `crates/od-ui/src/app/inbox.rs:109` local Message center (run done/failed/cancelled, read, archive); remote platform notifications do not apply; e2e manage step 6 |
| 16 | Home | Onboarding (pick agent, BYOK, connection test, brand extraction) | 50 | 50 | 100 | `crates/od-ui/src/app/onboarding.rs:25-27,79`, `onboarding_brand.rs:91-130` step 2 extracts a design system from the brand's site; e2e c2 step 1; `shots/c2-onboarding-brand-light.png` |
| 17 | Home | Switch theme, switch language | 100 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:363` language, theme in Settings ▸ Appearance |
| 18 | Home | Floating pet | 0 | 0 | N/A | Pet: N/A (settled decision) |
| 19 | Home | GitHub Star, Discord, Get help, feature request | N/A | N/A | N/A | DROPPED (badge, community) |
| 20 | Home | What's new, privacy consent, NPS survey, cloud sign-in, allowance, campaign | N/A | N/A | N/A | DROPPED (update/telemetry/cloud/billing) |
| 21 | Projects | Project grid with cover images | 100 | 100 | 100 | `crates/od-ui/src/app/projects.rs:534,808`; `(audit run) shots/manage/shots/projects-grid-light.png` |
| 22 | Projects | Rename, duplicate, delete with confirmation | 100 | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:130-145` project menu (rename, duplicate), delete with confirmation |
| 23 | Projects | Run status badge (Running, Needs input, Completed…) | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:638` StatusBadge (Running, Needs input, Completed…); e2e manage step 2 |
| 24 | Projects | Project type tags | 50 | 100 | 100 | `crates/od-ui/src/app/manage.rs:114` 6 tags (Live artifact, Web clone, Slide, Media, Prototype, Other), `projects.rs:851` |
| 25 | Projects | Board (kanban) by status | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:988-1024` board by status; `(audit run) shots/manage/shots/projects-board-light.png` |
| 26 | Projects | Multi-select to delete | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:464-495` multi-select, delete with confirmation; e2e manage |
| 27 | Projects | Filter, sort, list view | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:433` type filter, `:444` sort, `:529` list view; `(audit run) shots/manage/shots/projects-list-light.png` |
| 28 | Projects | Search projects and conversations (Cmd/Ctrl+K) | 0 | 100 | 100 | Cmd/Ctrl+K `crates/od-ui/src/app/mod.rs:266` → `palette.rs:1` (project, page, file); also searches conversations/messages `projects.rs:424,576` |
| 29 | Projects | Project creation panel (fidelity, target platform, speaker notes, animation, media options) | 50 | 100 | 100 | `crates/od-ui/src/app/new_project.rs:132-321`: type, platform, fidelity, speaker notes, animation, media; `(audit run) shots/manage/shots/new-project-panel-light.png` |
| 30 | Projects | Open a folder as a project | 100 | 100 | 100 | `crates/od-ui/src/app/projects.rs:366,388-400`; Home `home.rs:850` |
| 31 | Projects | ZIP import (Claude Design) | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:345-351` Claude Design ZIP import; e2e manage |
| 32 | Projects | Project instructions | 50 | 100 | 100 | `crates/od-ui/src/app/projects.rs:892-965` instructions dialog; `(audit run) shots/manage/shots/project-instructions-light.png` |
| 33 | Projects | Live artifact badge on cards | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:649-658` LiveBadges |
| 34 | Projects | Recently viewed | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:414` Recently viewed, recorded by the `mod.rs` ViewTracker |
| 35 | Projects | Team space, invite, presence, device sync | N/A | N/A | N/A | Team/cloud: DROPPED |
| 36 | Studio-chat | Assistant turn streaming: text, thinking, tool row, Markdown | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:568`; e2e studio step 3 |
| 37 | Studio-chat | Stop | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:312` |
| 38 | Studio-chat | Conversation list: create, rename, delete | 50 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:100-167` rename, delete with confirmation; `chat_view.rs:376` New; e2e chat step 15 |
| 39 | Studio-chat | Fork from a message | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:593` |
| 40 | Studio-chat | Session mode (Ask/Plan/Design) | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:151` |
| 41 | Studio-chat | Attach files, upload in the composer | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:778-887`, `:978` attach button, paste, drag and drop; e2e chat step 7 |
| 42 | Studio-chat | Reference another project, link local code | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:470-611` Link local code, Reference project; e2e chat step 17 |
| 43 | Studio-chat | Figma import (.fig, URL) | 0 | 0 | N/A | Figma: N/A (settled decision) |
| 44 | Studio-chat | Import from Library | 0 | 0 | 100 | `crates/od-ui/src/app/library_picker.rs:150-208` (search, type filter, import), Home `home.rs:295`, Studio `chat_view.rs:165`, @ in `mention.rs`; e2e c2 steps 3–4; `shots/c2-library-picker-light.png` |
| 45 | Studio-chat | Switch design system right in the composer | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:638`; e2e chat step 11 |
| 46 | Studio-chat | @ mention palette | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:694-717`; e2e chat step 8 |
| 47 | Studio-chat | Slash commands (/mcp, /search) | 0 | 100 | 100 | `crates/od-ui/src/mention.rs` /mcp, /search; e2e chat step 9 |
| 48 | Studio-chat | Inline model switcher (CLI/BYOK, agent, model) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:1172-1253` agent, model, reasoning; e2e chat step 10 |
| 49 | Studio-chat | Queue: edit, drag and drop, steer | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:435-487` |
| 50 | Studio-chat | Quote to chat | 0 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:242` quote-selection; e2e chat step 13c |
| 51 | Studio-chat | Run status (Working, Thinking) | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:632` |
| 52 | Studio-chat | Todo list + plan pill "step N/M" | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:412` |
| 53 | Studio-chat | Media generation row, audio player | 0 | 50 | 100 | `crates/od-ui/src/app/media_rows.rs:98-185` media generation row (tile, progress, error, Retry), mounted at `chat_view.rs:571`; audio player `chat_ops.rs:325`; e2e c2 step 6; `shots/c2-media-row-light.png` |
| 54 | Studio-chat | "Files from this turn" (Open/Download) | 50 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:285-333` Open, Download, +N; e2e chat step 13 |
| 55 | Studio-chat | Copy message | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:349` |
| 56 | Studio-chat | "Continue remaining tasks" | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:379-381`; e2e chat step 12 |
| 57 | Studio-chat | Question form | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:850-865` |
| 58 | Studio-chat | GenUI surface (Authorize, Accept all…) | 0 | 0 | 100 | `crates/od-ui/src/app/genui_view.rs:88-312` (Continue/Cancel, Authorize/Skip, form, choice, diff Accept all/Reject all/Partial, remembered answers, Revoke), mounted at `chat_view.rs:264`; e2e c4 steps 4–5; `shots/c4/chat-genui-light.png` |
| 59 | Studio-chat | Next-step actions, suggested next steps | 0 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:669` |
| 60 | Studio-chat | Design toolbox follow-up (Match, Icons, Motion, Polish…) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:1036-1050`, `chat_view.rs:713`; e2e chat step 13b |
| 61 | Studio-chat | Design Jury (critique theater) | 50 | 50 | 100 | `crates/od-ui/src/app/agent_w3.rs:199,608-646` rounds/roles/scores; Interrupt `:535`, Replay `:543` + speed `:511`, `crates/od-ui/src/jury_replay.rs:46`; e2e c2 step 7; `shots/c2-jury-replay-light.png` |
| 62 | Studio-chat | Run errors, reconnect, retry | 50 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:405` Reconnect, Retry `chat_view.rs:196`; e2e chat step 14 |
| 63 | Studio-chat | Export log/conversation | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:364-376` |
| 64 | Studio-chat | Project memory (+ "Remember that…") | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:248-319` |
| 65 | Studio-chat | Side chat | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:876-918` |
| 66 | Studio-chat | Collapse/resize the chat pane | 0 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:389` collapse, `chat_ops.rs:214` resize, `studio.rs:1120` Show chat; e2e chat step 16 |
| 67 | Studio-chat | Post-run deliverable warning | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1378` |
| 68 | Studio-chat | Thumbs feedback, share to community, support dialog, balance gate | N/A | N/A | N/A | DROPPED (telemetry/share/community/billing) |
| 69 | Studio-preview | Sandboxed HTML preview + live reload | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1321-1334`; e2e studio steps 4–5 (reload 47 ms) |
| 70 | Studio-preview | Preview / Code toggle | 0 | 50 | 100 | Toggle `crates/od-ui/src/app/studio.rs:1350-1360`; highlighting `studio.rs:1570`, `crates/od-ui/src/highlight.rs:84` (HTML+CSS+JS, CSS, JS, Markdown); e2e preview step 2; `shots/c1/code-view-light.png` |
| 71 | Studio-preview | Device picker Desktop/Tablet/Mobile | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:48-57` Desktop/Tablet/Phone |
| 72 | Studio-preview | Zoom in/out/reset | 0 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1391-1397`, keys Cmd +/−/0 |
| 73 | Studio-preview | Reload | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:295` reload_preview |
| 74 | Studio-preview | Screenshot to clipboard | 0 | 100 | 100 | `crates/od-ui/src/app/capture.rs:140`; e2e files step 10 |
| 75 | Studio-preview | Screenshot to chat | 50 | 100 | 100 | `crates/od-ui/src/app/capture.rs:143`; e2e files step 10 |
| 76 | Studio-preview | Comment mode: pin, attach to the next turn | 100 | 100 | 100 | `crates/od-ui/src/app/preview_comments.rs:109-197` |
| 77 | Studio-preview | Comments tab, board, stale/lost anchor status, attach images to comments | 50 | 50 | 100 | `crates/od-ui/src/app/comments_panel.rs:55-230` Comments tab (count, attach, resolve, delete), anchor stale `:175`/lost `:178`, images `:195,297`, `crates/od-ui/src/comment_board.rs`; e2e preview step 4; `shots/c1/comments-tab-light.png` |
| 78 | Studio-preview | Draw/Mark (pen, box, text, undo/redo) | 50 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1282-1305` pen, box, text, undo, redo, clear; e2e files step 13 |
| 79 | Studio-preview | Manual edit (layers, Content/Style/Attributes/Html, style controls, image upload, delete element) | 50 | 50 | 100 | `crates/od-ui/src/app/manual_edit.rs:498-731` layers, tabs Content/Style/Attributes/Html, image upload `:604`, delete element `:545`, undo/redo `:536,540`; opened from `preview_tools.rs:650`; e2e preview step 3; `shots/c1/manual-edit-attributes-light.png` |
| 80 | Studio-preview | Inspect panel | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:717` |
| 81 | Studio-preview | Tweaks | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:911` |
| 82 | Studio-preview | Palette swap, 5 presets | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:662` |
| 83 | Studio-preview | Anti-slop lint | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:625,942-969` |
| 84 | Studio-preview | Version history: view, restore | 100 | 100 | 100 | `crates/od-ui/src/app/versions.rs:92-149` |
| 85 | Studio-preview | Deck: navigation, thumbnail rail | 100 | 100 | 100 | `crates/od-ui/src/app/deck.rs:51-278` |
| 86 | Studio-preview | Speaker notes: view, edit, save | 50 | 100 | 100 | `crates/od-ui/src/app/deck.rs:357-397` edit and save into the deck; e2e files step 11 |
| 87 | Studio-preview | Present: in tab / fullscreen / new tab / presenter mode | 50 | 100 | 100 | `crates/od-ui/src/app/deck.rs:167-177` in tab, fullscreen, new tab, presenter (`presenter.rs:245-282`, synced); `(audit run) shots/b4/presenter-light.png` |
| 88 | Studio-preview | Markdown viewer/editor (Editor/Split/Preview) | 0 | 100 | 100 | `crates/od-ui/src/app/doc_view.rs:25-271` Editor/Split/Preview, auto-save; `(audit run) shots/b4/markdown-split-light.png` |
| 89 | Studio-preview | Live artifact viewer (Preview/Code/Data/Refresh history) | 0 | 100 | 100 | `crates/od-ui/src/app/live_view.rs:186-232` Preview/Code/Data/Refresh/History; `(audit run) shots/b4/live-artifact-light.png` |
| 90 | Studio-preview | Image viewer | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1304` |
| 91 | Studio-preview | PDF/Office/video/audio viewer | 50 | 100 | 100 | `crates/od-ui/src/app/doc_view.rs:345-406` PDF, video, audio, Office with text extraction, Download/Open |
| 92 | Studio-preview | Finalize design package (DESIGN.md) | 50 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:343,387-418` Finalize/Re-finalize per DESIGN.md; e2e files step 12c |
| 93 | Studio-preview | Hand off: pick editor, framework, copy prompt, Continue in CLI | 50 | 100 | 100 | `crates/od-ui/src/app/export_extra.rs:156-194` framework, target editor, prompt; Cmd+Shift+K `files_panel.rs:1379`; `(audit run) shots/b4/handoff-dialog-light.png` |
| 94 | Studio-preview | Brand ready prompt | 0 | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:182-209` brand progress/ready/use/dismiss; `(audit run) shots/b4/brand-ready-light.png` |
| 95 | Studio-preview | Share link, visibility | N/A | N/A | N/A | Share/cloud: DROPPED |
| 96 | Studio-files/tabs | File panel (tree, breadcrumb) | 50 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:576-682` tree, collapsible folders with counts, breadcrumb |
| 97 | Studio-files/tabs | Upload, paste as text file, drag and drop | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:482-485,545` upload, paste as text, drag and drop; e2e files steps 2, 4 |
| 98 | Studio-files/tabs | New sketch | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:495`, `sketch.rs:168`; `(audit run) shots/b4/sketch-light.png` |
| 99 | Studio-files/tabs | New document (Markdown from a template) | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:489` |
| 100 | Studio-files/tabs | Folders: create, move | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:498`, move `:909` |
| 101 | Studio-files/tabs | Row menu: open in tab, download, copy path, delete | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:903-911` open, new tab, download, copy path, full path, rename, move, delete |
| 102 | Studio-files/tabs | Multi-select: download ZIP, delete | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:804-809` ZIP, move, delete |
| 103 | Studio-files/tabs | Group/filter by kind, pagination | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:528,734-767` by kind, filter, show more; `(audit run) shots/b4/files-by-kind-light.png` |
| 104 | Studio-files/tabs | Create a design system from a project | 0 | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:141`; e2e files step 12d |
| 105 | Studio-files/tabs | File tab bar, close tab | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1145-1168` |
| 106 | Studio-files/tabs | "+" tab launcher (open file, create new) | 50 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1268-1287` open file, document, page, terminal |
| 107 | Studio-files/tabs | Terminal tab | 100 | 100 | 100 | `crates/od-ui/src/app/terminal.rs:294` |
| 108 | Studio-files/tabs | Built-in browser tab | 0 | 0 | N/A | Browser tab: N/A (settled decision) |
| 109 | Studio-files/tabs | Pages list, create page | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:361,492` Pages group, create page |
| 110 | Studio-files/tabs | Focus workspace / Show chat | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1329`, Show chat `studio.rs:1120`; `(audit run) shots/b4/focus-workspace-light.png` |
| 111 | Studio-files/tabs | Quick switcher Cmd/Ctrl+P | 100 | 100 | 100 | `crates/od-ui/src/app/switcher.rs:72` |
| 112 | Export | PDF | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:17-42` Pdf |
| 113 | Export | Image PPTX | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Pptx |
| 114 | Export | Editable PPTX | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:49` export-pptx-editable (via local Chrome) |
| 115 | Export | Project ZIP | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Zip |
| 116 | Export | Standalone HTML | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Html |
| 117 | Export | Export Markdown (file) | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Markdown; e2e files step 12 |
| 118 | Export | Image export with format choice | 50 | 100 | 100 | PNG, JPEG, WebP `crates/od-ui/src/app/export_menu.rs:21-23` |
| 119 | Export | Export JSX / preview as HTML | 0 | 0 | 100 | Export as JSX `crates/od-ui/src/app/export_menu.rs:29`, `crates/od-ui/src/jsx_export.rs:7-17`; "Export React component as HTML" `export_menu.rs:31,53` (testid `export-jsx-html`) downloads the page from `GET /api/projects/:id/react/*path` (`crates/od-server/src/routes/raw.rs:177`, `lib.rs:53`); preview renders `.jsx/.tsx` as a component `crates/od-ui/src/app/studio.rs:1251`. The page is built in `crates/od-preview/src/react_component.rs:32` (`react` imports read the global, exports become declarations, `prepare` at line 135), React 18.3.1 + Sucrase 3.35.0 inlined from `crates/od-preview/vendor/react-runtime/` (LICENSE included), no CDN. Tests: `react_component` 3 unit, `crates/od-server/tests/c1_preview.rs:259`; e2e c2 step 8 (preview `.card` renders, downloaded HTML file runs on its own when opened standalone, `nothing fetched`) |
| 120 | Export | Save as template | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:329`, `export_extra.rs:74-100` |
| 121 | Export | HyperFrames MP4 | 50 | 100 | 100 | MP4 in the Export menu `crates/od-ui/src/app/export_menu.rs:26` + Media page `media_page.rs:922` |
| 122 | Export | Open in new tab, copy path, download file | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:317-325` |
| 123 | Export | Deploy Vercel/Cloudflare, social share, quick share | N/A | N/A | N/A | Deploy/share: DROPPED |
| 124 | Design systems | Design systems page (Your systems, presets, filter) | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:177-315`; e2e library PASS |
| 125 | Design systems | Pick a design system when creating a project | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:353`, `new_project.rs:162` |
| 126 | Design systems | Create a DS from a brand description | 100 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:141-187` |
| 127 | Design systems | Create a DS from URL, GitHub, file, Figma, pasted DESIGN.md, copy of an existing DS | 0 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:519-584` website, GitHub, Figma via agent, file, DESIGN.md/CSS, copy of an existing set; e2e library fromWebsite, fromCss |
| 128 | Design systems | "Start from a brand" picker | 0 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:519,628` brand picker |
| 129 | Design systems | Phased generation progress | 50 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:569` 4 phases |
| 130 | Design systems | Review (Looks good / Needs work), revision accept/reject | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:917-947,1054-1075` review, revision accept/reject |
| 131 | Design systems | Kit view (Showcase, Tokens, DESIGN.md), brand detail, preview modal | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:997-1211` showcase, tokens, DESIGN.md, fullscreen |
| 132 | Design systems | Edit/copy/save DESIGN.md, upload font/logo | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:1009-1022,1156` edit/copy DESIGN.md, upload asset |
| 133 | Design systems | Default for new chats | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:878` |
| 134 | Design systems | Download DS, Edit with agent | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:872,893` |
| 135 | Design systems | Import a DS from GitHub/shadcn in Settings | 0 | 100 | 100 | In the Create flow of the Design systems page (GitHub via agent, pasted shadcn theme); the shadcn registry is not fetched because nothing goes to the network |
| 136 | Design systems | Rebuild token contract, quality score | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:1142-1150` score, rebuild |
| 137 | Design systems | Publish, share/sync to team | N/A | N/A | N/A | Publish/team: DROPPED |
| 138 | Plugins/Skills | Plugin list, plugin detail | 50 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:234-332,552` |
| 139 | Plugins/Skills | Use a plugin (Use, Remix) | 50 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:607-627`; e2e library pluginUse |
| 140 | Plugins/Skills | Plugin input form | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:616` InputsForm |
| 141 | Plugins/Skills | Add a plugin (from prompt, link, folder), install/uninstall | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:930-971` agent, link, folder; uninstall `:595` |
| 142 | Plugins/Skills | Registry sources, trust, marketplace | 0 | 0 | N/A | Marketplace/registry: N/A (settled decision) |
| 143 | Plugins/Skills | Skill list, skill detail | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:251,705` |
| 144 | Plugins/Skills | Edit skills (create, edit, delete, user override) | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:734-836` create, edit (override), delete |
| 145 | Plugins/Skills | Plugin rail in the composer | 0 | 100 | 100 | Plugins reach the composer via the "+" menu and @ (`crates/od-ui/src/app/composer.rs:1091`), matching the original app's behaviour (rail off, `showRail={false}`) |
| 146 | Plugins/Skills | Publish/contribute plugin, share with team | N/A | N/A | N/A | Publish/community/team: DROPPED |
| 147 | Templates/Examples | Gallery with filters (type, surface, industry) | 50 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:222-246` |
| 148 | Templates/Examples | Template cards with live preview | 100 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:259,379`, `lazy_frame.rs` |
| 149 | Templates/Examples | Create from a template | 100 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:407`, Home `home.rs:975` |
| 150 | Templates/Examples | Preview modal (fullscreen, Remix, export) | 0 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:399-407` fullscreen, new tab, download, Remix, Use |
| 151 | Templates/Examples | Image/video prompt templates (Copy prompt) | 50 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:440-488` Copy, Generate |
| 152 | Templates/Examples | User-saved templates | 0 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:541-601` |
| 153 | Templates/Examples | Community gallery, share template | N/A | N/A | N/A | Community/share: DROPPED |
| 154 | Settings | Pick agent + rescan | 100 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:222`; `(audit run) shots/manage/shots/settings-light.png` |
| 155 | Settings | Model, reasoning effort, service tier | 50 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:271,287,298` model, reasoning, service tier |
| 156 | Settings | Advanced: proxy, custom paths | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:428-505` Advanced env (proxy, paths) |
| 157 | Settings | Agent connection test | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:308`; e2e manage step 3 |
| 158 | Settings | BYOK: provider presets, key, base URL, fetch models | 50 | 100 | 100 | `crates/od-ui/src/app/byok_form.rs:216-295` preset, protocol, base URL, key, fetch models, test, save; `(audit run) shots/manage/shots/settings-api-light.png` |
| 159 | Settings | Agent permissions (Automatic / Ask first) | 100 | 100 | 100 | Settings ▸ agent permissions (Automatic/Ask first) |
| 160 | Settings | Language | 50 | 50 | 100 | 20 languages: en, vi + the original app's 18 locales (`crates/od-ui/locales/*.txt`, loaded on demand, en fallback), RTL for ar/fa; picker `settings_page.rs:363`; e2e c4 steps 1–2 (no layout overflow across 12 screens × 19 languages); `shots/c4/home-ar-light.png` |
| 161 | Settings | Theme | 100 | 100 | 100 | Settings ▸ Appearance |
| 162 | Settings | Notifications: sound, desktop notification | 0 | 100 | 100 | `crates/od-ui/src/app/notify.rs:240-268` sound, desktop, test |
| 163 | Settings | Project locations | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:724-767` add, default, scan |
| 164 | Settings | Global rules / instructions | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:544-556` |
| 165 | Settings | Global memory (types, hooks, import, MEMORY.md) | 50 | 50 | 100 | `crates/od-ui/src/app/memory_settings.rs:60-331` on/off switch `:64`, hooks `:78`, import (text/file/scan) `:218-264`, edit MEMORY.md `:310-331`, mounted at `settings_page.rs:406`; e2e c2 step 9; `shots/c2-settings-memory-light.png` |
| 166 | Settings | Media providers (key, base URL) | 50 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:847-888`; `(audit run) shots/manage/shots/settings-media-light.png` |
| 167 | Settings | About (version, local diagnostics export, reset onboarding) | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:928-945` version, export diagnostics, reset onboarding |
| 168 | Settings | Quick settings menu (avatar menu) | 0 | 0 | 100 | `crates/od-ui/src/app/quick_menu.rs:142-234` (theme, language, section, Shortcuts, Docs, Report), mounted at `mod.rs:471`; e2e c2 step 10; `shots/c2-quick-menu-light.png` |
| 169 | Settings | Labs, privacy, account, wallet, update | N/A | N/A | N/A | DROPPED (labs/privacy/account/wallet/update) |
| 170 | Agent runtimes | Claude Code (stream, resume, steer) | 100 | 100 | 100 | `crates/od-runtime`; e2e studio (mock claude) |
| 171 | Agent runtimes | Codex | 100 | 100 | 100 | `crates/od-runtime` |
| 172 | Agent runtimes | Cursor, OpenCode, Copilot, Qwen | 100 | 100 | 100 | `crates/od-runtime` |
| 173 | Agent runtimes | ACP family (Kimi, Hermes, Kiro, Kilo, Vibe, Trae, Reasonix, Devin, DeepSeek Harness) | 100 | 100 | 100 | `crates/od-runtime/src/parse/dsh.rs` and the ACP family |
| 174 | Agent runtimes | Other CLIs (Aider, Amp, Pi, Qoder, MiMo, Codebuddy, AtomCode, Grok Build, Antigravity, DeepSeek TUI) | 0 | 0 | 100 | `crates/od-runtime/src/def.rs:450-548` grok-build, qoder, amp, pi, deepseek (TUI), aider, antigravity, codebuddy, mimo, atomcode; parsers `crates/od-runtime/src/parse/{pi,qoder,antigravity}.rs`; e2e c3 step 1 (detection), step 3 (one chat turn via Qoder) |
| 175 | Agent runtimes | Agent detection + Refresh | 100 | 100 | 100 | `GET /api/agents`, `crates/od-ui/src/app/onboarding.rs:131` Refresh |
| 176 | Agent runtimes | Connection test | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:308`, onboarding test; e2e manage step 1 |
| 177 | Agent runtimes | BYOK presets (26 providers) | 50 | 100 | 100 | `crates/od-ui/src/byok.rs`; 26/26 presets: Azure OpenAI `byok.rs:54` (protocol `azure`), runtime `crates/od-runtime/src/byok.rs:46,101` (URL `/openai/deployments/<deployment>/chat/completions?api-version=…`, default `2024-10-21`, an `/openai/v1` base keeps its path; key in `api-key`), the form has an API version field `crates/od-ui/src/app/byok_form.rs:280`. Loopback mock: `crates/od-runtime/tests/engine_m6.rs:572` (stream), `crates/od-server/tests/manage.rs:406` (Test connection, saves `apiVersion`); e2e manage step 1 |
| 178 | Agent runtimes | AMR cloud | N/A | N/A | N/A | Cloud: DROPPED |
| 179 | MCP | MCP server `od mcp` (stdio) | 100 | 100 | 100 | `crates/od-cli/src/mcp_cmd.rs`, `crates/od-mcp`; e2e studio step 15d |
| 180 | MCP | Install MCP into agents (per-client tabs, copy command) | 50 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:241-302` per-client snippet, deeplink, Install/Uninstall; `(audit run) shots/manage/shots/integrations-agents-light.png` |
| 181 | MCP | External MCP servers (stdio/HTTP, args, test) | 100 | 100 | 100 | `crates/od-ui/src/app/mcp_settings.rs:78-249` |
| 182 | MCP | MCP OAuth | 100 | 100 | 100 | `crates/od-ui/src/app/mcp_settings.rs:246` Connect/Sign in |
| 183 | MCP | Sample MCP catalog | 0 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:181-208` |
| 184 | MCP | Note on which agents it is forwarded to | 0 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:111-115` |
| 185 | MCP | Connectors (Composio) | 0 | N/A | N/A | Composio connectors: N/A (settled decision) |
| 186 | MCP | Use everywhere (CLI + MCP guide) | 0 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:333-350`; `(audit run) shots/manage/shots/integrations-everywhere-light.png` |
| 187 | Media | Media providers | 50 | 50 | 100 | `crates/od-media/src/provider.rs:70-84`: OpenAI, fal, ElevenLabs, Volcengine, Grok, Nano Banana, ImageRouter, OpenRouter, Custom Image, Leonardo, MiniMax, FishAudio, SenseAudio, AIHubMix, Tavily (+ local HyperFrames). Covers every `integrated: true` provider of the original app except `vela` (OpenDesign Cloud, cloud) and `stub`; the original app's 9 `integrated: false` providers are placeholders anyway. e2e c3 steps 4–5; `shots/c3/shots/settings-media-providers-light.png` |
| 188 | Media | Image/video/audio projects in the UI (model, aspect, voice) | 0 | 100 | 100 | `crates/od-ui/src/app/media_page.rs:615-734` |
| 189 | Media | Audio player in chat | 0 | 100 | 100 | Audio in chat `crates/od-ui/src/app/chat_ops.rs:325`, library `media_page.rs:437` |
| 190 | Media | Library (filter, upload, Chat to design) | 0 | 100 | 100 | `crates/od-ui/src/app/media_page.rs:346-404` |
| 191 | Media | Web Clipper | 0 | 0 | N/A | Web Clipper: N/A (settled decision) |
| 192 | Media | HyperFrames → MP4 | 50 | 100 | 100 | `crates/od-ui/src/app/media_page.rs:922-944` |
| 193 | Automations | Automations page | 100 | 100 | 100 | `crates/od-ui/src/app/automations.rs:483` |
| 194 | Automations | Create an automation (name, prompt, schedule, project) | 50 | 100 | 100 | `crates/od-ui/src/app/automations.rs:496-554,753-817` hourly/daily/weekly presets, timezone, new project per run |
| 195 | Automations | Row actions: Run now, enable/disable, delete, open conversation, edit | 50 | 100 | 100 | `crates/od-ui/src/app/automations.rs:920-931` enable/disable, Run now, Edit, delete |
| 196 | Automations | Run history | 100 | 100 | 100 | `crates/od-ui/src/app/automations.rs:948-963` |
| 197 | Automations | Crystallize, evolution proposals | 0 | 0 | 100 | `crates/od-ui/src/app/automations.rs:877-888` Crystallize a run, `:588-657` proposals Apply/Reject; e2e c4 step 3; `shots/c4/automations-proposals-light.png` |
| 198 | Automations | Automation templates | 0 | 100 | 100 | `crates/od-ui/src/app/automations.rs:114-158` the original app's 7 templates |
| 199 | Automations | Orbit (daily connector digest) | 0 | 50 | N/A | Orbit: N/A (settled decision) |
| 200 | Automations | Live artifact refresh | 0 | 100 | 100 | `crates/od-ui/src/app/live_view.rs:199` Refresh + history; live-artifact maintainer template |
| 201 | Desktop shell | Main window, in-process daemon | 100 | 100 | 100 | `apps/desktop/src/main.rs:270` |
| 202 | Desktop shell | App/Edit/View/Window menus | 100 | 100 | 100 | `apps/desktop/src/main.rs:198-206` Menu::default + Project |
| 203 | Desktop shell | Help menu (Documentation, Report Issue) | 0 | 50 | 100 | `apps/desktop/src/main.rs:232-247` Help: Documentation, Report Issue, Show Data Folder, Shortcuts, Command palette; handled at `:287-288` (bundled guide `docs_page.rs:45`, Report copies diagnostics, no network). The Tauri shell has not been run; the web build has the same actions via the Quick menu, e2e c2 step 10; `shots/c2-docs-light.png` |
| 204 | Desktop shell | Native folder picker | 100 | 100 | 100 | `apps/desktop/src/main.rs` pick_folder |
| 205 | Desktop shell | Open folder in Finder | 100 | 100 | 100 | Handoff ▸ Open project folder, Help ▸ Show Data Folder |
| 206 | Desktop shell | PDF/image export with a browser engine | 100 | 100 | 100 | Local Chrome via CDP (decisions.md #2) |
| 207 | Desktop shell | Splash | 0 | 100 | 100 | `apps/desktop/static/index.html:6` splash while the daemon starts |
| 208 | Desktop shell | Native theme sync | 0 | 100 | 100 | `apps/desktop/src/main.rs:123-133` set_theme following Appearance |
| 209 | Desktop shell | Pet window | 0 | 0 | N/A | Pet: N/A (settled decision) |
| 210 | Desktop shell | Auto-update, invite deep link, crash diagnostics, AMR Develop menu | N/A | N/A | N/A | auto-update N/A (decision); cloud/telemetry DROPPED |
| 211 | Shortcuts | Cmd/Ctrl+, opens Settings | 0 | 100 | 100 | `crates/od-ui/src/app/mod.rs:278` |
| 212 | Shortcuts | Cmd/Ctrl+K search projects | 0 | 100 | 100 | `crates/od-ui/src/app/mod.rs:266` (palette) |
| 213 | Shortcuts | Cmd/Ctrl+B collapse rail | 0 | 100 | 100 | `crates/od-ui/src/app/mod.rs:282-287`; in Studio it toggles the file panel `studio.rs:840` |
| 214 | Shortcuts | Cmd/Ctrl+P quick switcher | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:836` |
| 215 | Shortcuts | Cmd/Ctrl+T new tab | 50 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1361` opens the tab launcher |
| 216 | Shortcuts | Cmd/Ctrl+W close tab | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:856` |
| 217 | Shortcuts | Ctrl+Tab / Ctrl+Shift+Tab | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:876` |
| 218 | Shortcuts | Ctrl+PageUp/PageDown, Cmd+Opt+←/→ | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1368-1375` |
| 219 | Shortcuts | Cmd/Ctrl+1…9 | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:864` |
| 220 | Shortcuts | Cmd/Ctrl+Shift+K Continue in CLI | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1379` |
| 221 | Shortcuts | Enter / Shift+Enter send, new line | 100 | 100 | 100 | `crates/od-ui/src/app/composer.rs`, `chat_view.rs` |
| 222 | Shortcuts | @ and / in the composer | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:717`, `mention.rs` |
| 223 | Shortcuts | Cmd/Ctrl+Enter submit comment | 50 | 100 | 100 | `crates/od-ui/src/app/preview_comments.rs:182` |
| 224 | Shortcuts | Cmd/Ctrl+Z in draw | 0 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1211` |
| 225 | Shortcuts | Deck: →, ←, PageUp/Down, Home, End, R | 50 | 100 | 100 | `crates/od-ui/src/app/studio.rs:898-901`, R in presenter `presenter.rs:211` |
| 226 | Shortcuts | Esc closes modal, present, mode | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:884` |
| 227 | Shortcuts | Keys in the DS kit (E/C/U/R/⌫) | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:661-666` (E/C/U/R/⌫) |
