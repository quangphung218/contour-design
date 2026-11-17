# QA bugs: main after beta.45 (cd76b867, #223–#238)

QA pass on `main` at cd76b867 (beta.45): what #223–#238 changed, from
the user's side. Exports (PNG, PDF, PPTX, MP4) one after the other, two at
once, and cancelled midway (tab closed, page reloaded, `od export` stopped
with Ctrl+C, Contour quit), checking with `ps` that no Chrome keeps
running; Media ▸ MP4 and its list of motion projects; Settings saved from
two windows; upload names (long, Vietnamese, decomposed, emoji, case) in
the Files panel and a design system. Also the suites §5, §8, §10/§23 and
§27 again, and `manage.e2e.mjs`. Web and CLI only, automated. macOS 26,
Apple silicon, Chrome 157, 2026-11-17. Desktop not run: #223–#238 changed
`od-server`, `od-store`, `od-render-cdp` and the UI bundle, which the
desktop app runs unchanged; the LaunchServices start is the same code
path for `od` and the app.

**Two bugs.** What #228–#238 set out to do holds: every export gave its
file, and in all six ways of ending one (finished, two at once, tab
closed, reload, Ctrl+C, quit) no Chrome was left running 30 s later;
Media ▸ MP4 lists only motion projects and answers a page with "not a
motion video". QA-106 and QA-107 stay fixed. But a Settings save still
sends the whole copy the window read when it opened, so a second window
puts back the theme, or the local agent over the API provider, that the
first one set (QA-108); and a design system asset whose name the app
rewrites (`🎨.png`, `logo chính.png`) is written over by a later upload
of another name that rewrites the same way, while the page says
"Uploaded." (QA-109).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Summary

| ID | Sev | Bug | Found by | Root cause | Same class |
|---|---|---|---|---|---|
| QA-108 | 🟠 | A Settings save from a second window (opened before a change in the first) puts back the old theme, permission mode, language, or agent; after the first window chose an API provider, the next run goes to the local agent | SET-12, SET-14 | `Shell::save_config` (`od-ui/src/app/mod.rs:125`) sends `AppConfig::to_patch`, every key of the copy read when the window opened; `PUT /api/app-config` merges it, so keys the form did not touch overwrite newer values. #227 left out only `projectLocations` and `defaultProjectLocationId` | Every caller of `save_config` sends the whole copy: Settings agent card and `save_prefs` (`settings_page.rs:194`, `:205`), API provider (`byok_form.rs:227`), notifications (`notify.rs:203`), the palette's theme (`palette.rs:258`), composer model (`composer.rs:1313`), Home (`home.rs:997`), default design system (`ds_page.rs:742`), quick menu, onboarding. SET-13 passes only because a window that never chose a language sends no `locale` |
| QA-109 | 🟠 | Design system ▸ Kit: `🎨.png` uploaded, later `🖼.png`: "Uploaded.", one `_.png` kept, the first picture gone. The same for `logo chính.png` then `logo-chính.png` | UL-14, UL-15 | `upload_assets` (`routes/library/design_systems.rs:876`) writes a name that was not shortened with `tokio::fs::write`, over an existing asset of the cleaned name; it compares cleaned names, not the names the user sent. `sanitize_name` turns every emoji into `_` and spaces into `-` | Within one batch `refuse_same_name` catches it (UL-13, UL-16 pass). The Files panel is not affected: `upload_file` never overwrites (`_.png`, `_-1.png`) |

## How it was run

```sh
crates/od-ui/e2e/qa/run.sh chrome                  # builds the UI and od
QA_SKIP_BUILD=1 crates/od-ui/e2e/qa/run.sh settings  # then uploads, drafts, export
OD_E2E_SKIP_BUILD=1 crates/od-ui/e2e/run.sh manage.e2e.mjs
```

One file at a time (`run.sh` clears `test-results/qa` on each call).
`chrome` needs Chrome and ffmpeg; it makes `/tmp/odqa-ch-*` folders (one
`TMPDIR` per daemon) and kills any Chrome a failed test left.
`QA_SHOTS=<dir>` saves the evidence screenshots.

## Results

| Suite | Result |
|---|---|
| chrome (§28, new) | 10 of 10 pass (second run; see "Noted") |
| settings (§8, §29 new) | 12 of 14 pass, 2 known bugs (QA-108 ×2) |
| uploads (§27, §27b new) | 20 of 22 pass, 2 known bugs (QA-109 ×2) |
| drafts (§10, §23) | 51 of 51 pass |
| export (§5) | 14 of 14 pass |
| `manage.e2e.mjs` (#227's spec) | pass |

### §28 Export leaves no Chrome running

| ID | Check | Result |
|---|---|---|
| CH-01 | Deck PDF, PNG, PPTX, editable PPTX, then film MP4 | **Pass**: each file valid; no Chrome after each; profile folders removed |
| CH-02 | Two PDFs at once | **Pass** |
| CH-03 | MP4, tab closed midway | **Pass**: Chrome ends; the next MP4 works |
| CH-04 | Media ▸ MP4, reload midway | **Pass**: Chrome ends, no MP4 saved, a PDF after works |
| CH-05 | `od export --format mp4`, Ctrl+C | **Pass**: non-zero exit, Chrome ends, no half file |
| CH-06 | Quit (SIGTERM) mid-MP4 | **Pass**: no Chrome 30 s after the quit |
| MP-01…04 | Media ▸ MP4 list, empty hint, page rewritten, CLI on a page | **Pass** |

### §29 Settings from two windows, §27b names

| ID | Check | Result |
|---|---|---|
| SET-11 | Default location (#227) | **Pass** |
| SET-12 | Dark in A, Ask first in B | **Fail: QA-108**. Theme back to `system` |
| SET-13 | vi in A, sound off in B | **Pass** (B sends no `locale`) |
| SET-14 | API provider in A, Dark in B | **Fail: QA-108**. `byok` kept, `agentId` back to `claude` |
| UL-10…13, UL-16 | Long, NFD, case, batch clashes | **Pass**. UL-13 is refused, naming both long names |
| UL-14 | `🎨.png` then `🖼.png` | **Fail: QA-109** ([01](main46/01-qa109-emoji-asset-written-over.png)) |
| UL-15 | `logo chính.png` then `logo-chính.png` | **Fail: QA-109** |

## QA-108 🟠 A second Settings window puts back what the first one changed

- **Steps**: open Settings in two tabs (or two windows). In tab A pick
  Dark. In tab B pick Permissions ▸ Ask first. Or: in tab A switch to
  API, choose Ollama, Save; in tab B pick Dark.
- **Expected**: each save changes what its own form shows; the theme
  stays Dark, the API provider stays the agent.
- **Actual**: after B's save `GET /api/app-config` holds `theme:
  "system"` (SET-12), or `agentId: "claude"` with the provider still
  stored (SET-14): the next run goes to the local Claude, not the API
  provider, and neither window says anything. Tab A still shows Dark
  until it is reloaded.
- **Cause**: `Shell::save_config` (`od-ui/src/app/mod.rs:125`) PUTs
  `AppConfig::to_patch()` of the window's copy, every known key plus
  `extra`. The daemon merges it whole. #227 drops only the two location
  keys on the server. Fix: each form sends only the keys it changed (a
  patch built from the fields it owns), or the daemon refuses a patch
  built on an older copy (a revision number in the config).
- **Same class**: every `save_config` caller (table above). A single
  window is safe: it updates its copy after each save.
- **Test**: `settings.qa.mjs` SET-12, SET-14.

## QA-109 🟠 A design system asset is written over by a later upload of another name

- **Steps**: Design systems ▸ one of your own ▸ Kit ▸ upload `🎨.png`.
  Later, upload `🖼.png` (another picture).
- **Expected**: both kept (as the Files panel keeps `_.png` and
  `_-1.png`), or the second refused with a reason. The first picture is
  never lost without a word.
- **Actual**: "Uploaded." ([01](main46/01-qa109-emoji-asset-written-over.png));
  `assets/` holds one `_.png`, the second picture. `logo chính.png` then
  `logo-chính.png` does the same with `logo-chính.png`.
- **Cause**: `upload_assets` (`od-server/src/routes/library/design_systems.rs:876`)
  writes a name that was not shortened with `tokio::fs::write`, over an
  existing file. The comment there allows "a name as sent replaces the
  asset of that name", but the check is on the cleaned name, and
  `sanitize_name` maps every emoji to `_` and whitespace to `-`. Fix:
  replace only when the cleaned name equals the name sent (NFC, case
  folded); otherwise take `stem-N.ext` as a shortened name does, or keep
  emoji in `sanitize_name`.
- **Same class**: none found. The Files panel, comments, composer and
  Media never overwrite (`upload_file`).
- **Test**: `uploads.qa.mjs` UL-14, UL-15.

## Tests corrected during the pass (test faults, not product)

- **SET-11** first sent a location with `label` instead of `name`; the
  daemon answered 400. Fixed; it passes.

## Noted, not a bug

- **MP4 render did not finish, once.** In the first `chrome` run
  (19:17–20:00, while this machine's network was down and the load
  average was 8–10) every MP4 that had to finish (CH-01, CH-03, MP-01, a
  direct API call) was still rendering after 120–240 s; the cancel tests
  passed. After the network came back, the same API call took 31 s and
  the whole suite passed. The template loads nothing from the network.
  Not reproduced; if it shows again, the render has no overall time limit
  to say so.
- **UL-13**: a design system batch of two long names that differ only at
  the end is refused ("… would both be saved as … Rename one"), where the
  Files panel keeps both. A reason is given, so not a bug, but the two
  screens differ.
- **Theme change**: a screenshot taken right after picking Dark shows
  light fields with light text for a moment; once the page settles the
  colours are right (checked with computed styles).
