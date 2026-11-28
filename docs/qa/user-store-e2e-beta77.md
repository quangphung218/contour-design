# QA: a user's files after the user_store PRs (main after beta.77)

Headless pass on `main` at `e831fab9` (after 0.2.0-beta.77, which holds
#353 #356 #359 #361 #362 #364 #367 #369), macOS 26, 2026-11-28.
A CLI daemon (`od daemon start`) on a data folder and `HOME` of its own,
the mock `claude`, and Playwright Chromium headless. No `Contour.app`,
no real data folder.

**Scenarios 1, 2, 3, 5 and 6 pass: 38 of 38 tests, no new bug.**
Scenario 4 (export into a folder that already holds the name) cannot run
headless: the question is asked by the desktop app's native Save panel.
Reading the code behind it found one weak spot, QA-126 (🟡, not reproduced).

| # | Scenario | Result | Tests |
|---|---|---|---|
| 1 | Skill: new, Save as skill, edit and Save, Replace from source, Restore, delete; two tabs on one skill | **Pass** | US-01 to US-09, US-26, US-28 |
| 2 | Design system: from a pasted DESIGN.md, a bundled brand (Copy), the agent (brief + file); edit DESIGN.md; same-name assets; delete | **Pass** | US-10 to US-16, US-29 |
| 3 | Plugin: install, reinstall, uninstall; `kill -9` during install, reinstall and uninstall, then start again | **Pass** | US-20 to US-25, US-27 |
| 4 | Export HTML/ZIP into a folder that already holds the name | **Not run** (desktop Save panel); QA-126 from the code | none |
| 5 | Data folder that is the user's link; links planted inside an entry | **Pass** | US-30 to US-36 |
| 6 | Broken memory `config.json` and `MEMORY.md` | **Pass** | US-40 to US-44 |

After every test the data folder is walked (links not followed) for
`.removing-*`, `.putting-*`, `.aside-*`, `.dropping-*`, `.journal-*`,
`.swap-*`, `.old-*` and a `Guard::write` temp `.<file>.<tag>.tmp`: none
was left in any test, including after each `kill -9`.

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## Run it

```sh
crates/od-ui/e2e/qa/run.sh userstore                          # builds the UI and od first
QA_SKIP_BUILD=1 QA_ONLY='US-2' crates/od-ui/e2e/qa/run.sh userstore   # the plugin and kill -9 tests only
```

Each test that needs a restart or a `kill -9` gets a daemon of its own
(`opts.daemon`); the others share one. Data and `HOME` folders come from
`os.tmpdir()`: set `TMPDIR` to keep them in a scratch folder. The whole
file takes about 75 s.

## 1. Skills

| ID | What the user does | Checked on disk | Result |
|---|---|---|---|
| US-01 | New skill "Giọng thương hiệu", body with ☕ | `SKILL.md` holds the description and body | Pass |
| US-02 | Edit, Save with the tag the form opened on | file written once | Pass |
| US-03 | Two tabs open one skill; A saves, then B with its old tag; a Save with no `If-Match` | B is 412, no tag is 428, A's text stays | Pass |
| US-04 | The same in two browser tabs (Library ▸ skill ▸ Edit) | tab B shows Reload / Save anyway (`skill-save-changed`), keeps what B typed; disk holds A | Pass |
| US-05 | Install a folder (Save as skill), install again (409 without `replace`, then with it), Restore, Delete | `.previous/<id>` holds the copy before; Restore swaps back with side files; Delete removes the skill and `.previous/<id>`; the source folder untouched | Pass |
| US-06 | Replace from source (`~/.claude/skills/tea` edited) after editing in Contour, then Restore | the agent's copy in, Contour's in `.previous`; Restore swaps back; agent folder never written | Pass |
| US-07 | New skill "Cà phê" NFC, then NFD | one folder, the first not overwritten | Pass |
| US-08 | Replace, delete, new skill of the same id, Restore | 404: the copy before the delete is not offered | Pass |
| US-09 | Delete a skill with a FIFO in it | deleted at once | Pass |
| US-26 | Replace and Restore of a 300-file skill, `kill -9` at 10 moments from 0 to 1.2 × one replace (≈270 ms) | after each restart: the skill whole (all files of one copy), `.previous` whole, listed | Pass |
| US-28 | Save and Delete of one skill sent together, 10 times | never a folder without `SKILL.md` | Pass |

## 2. Design systems

| ID | What the user does | Checked on disk | Result |
|---|---|---|---|
| US-10 | Import a pasted DESIGN.md "Phở Brand"; Copy a bundled brand | both folders hold `DESIGN.md` | Pass |
| US-11 | Two tabs edit DESIGN.md; A saves, B with its old tag | B is 412, A's text on disk | Pass |
| US-12 | Upload `logo.svg` twice; a batch with `dup.svg` and `DUP.svg` | one `logo.svg`, the second content; the batch is 400, none of it lands | Pass |
| US-14 | Generate with the agent from a brief and `brand.md` | job succeeds, `DESIGN.md` there | Pass |
| US-15 | Upload `logo.svg`, then `Logo.svg` | one file | Pass |
| US-16 | Four uploads of `mark.svg` at once | all 200, one file | Pass |
| US-13 | Delete | folder gone | Pass |
| US-29 | Delete a 600-file skill and a design system together, `kill -9` at 0–105 ms, 8 times | each whole or gone, listed only when on disk; one more restart brings nothing back | Pass (deletes finished within 15 ms, so only the 0 ms kill landed before them; US-23 catches a delete on its way) |

## 3. Plugins

| ID | What the user does | Checked on disk | Result |
|---|---|---|---|
| US-20 | Install (3 assets), install again (1 asset), uninstall | the second copy alone (no asset of the first), then nothing | Pass |
| US-21 | Reinstall 1 500 files over 2, `kill -9` once `.putting-*` appears | the old or the new copy, whole | Pass (caught while the tree was built) |
| US-22 | Reinstall 800 over 800, `kill -9` once `.aside-`, `.dropping-` or `.journal-` appears | one whole copy | Pass |
| US-23 | Uninstall 1 500 files, `kill -9` once `.removing-` or `.journal-` appears | not listed, folder not put back | Pass (caught on its way) |
| US-24 | Uninstall, normal restart | not listed | Pass |
| US-25 | Reinstall A/B in turn, `kill -9` at 12 moments from 0 to 1.2 × one install (≈250 ms) | one whole copy, listed, every time; the copy after the kill was B up to 107 ms, then alternated with what was sent | Pass |
| US-27 | Four installs of one plugin at once (A, B, A, B) | one whole copy | Pass |

## 5. Links

| ID | Setup | Checked | Result |
|---|---|---|---|
| US-30 | `OD_DATA_DIR` is a link to the real folder | skill create/save/delete, design system import/delete, plugin install/uninstall all land in the real folder; the link stays | Pass |
| US-31 | `skills/mine` is the user's link to a folder outside | Save writes in that folder; Delete removes the link only | Pass |
| US-32 | `skills/<id>/scripts` planted as a link to an outside folder; `SKILL.md` planted as a link to an outside file | replace, restore, delete and Save leave the outside folder and file as they were | Pass |
| US-33 | A design system's `assets` is a link to an outside folder | the upload is refused, nothing written outside | Pass |
| US-34 | A plugin folder to install holds a link to a private folder | nothing from behind the link is copied | Pass |
| US-35 | `plugins/ownp` is the user's link | uninstall removes the link only | Pass |
| US-36 | `<data>/skills` itself is the user's link | create, save, replace, delete work in the target; the link stays | Pass |

## 6. Memory files

| ID | Setup | Checked | Result |
|---|---|---|---|
| US-40 | `config.json` is cut JSON | `GET /api/memory/config`: `unreadable`, memory off; Settings ▸ Memory shows "config.json cannot be read…"; after Settings and a restart the file is byte for byte the same | Pass |
| US-41 | `{"enabled":"yes"}` | `unreadable`, file unchanged | Pass |
| US-42 | Hand-set `chatExtractionEnabled: true`, `verifyEnabled: false`, then one switch turned | the other switches stay as set | Pass |
| US-43 | `MEMORY.md` not UTF-8 | Settings shows "MEMORY.md cannot be read…"; unchanged after a restart | Pass |
| US-44 | Cut `config.json`, the user turns Memory on, as the notice says | nothing written before the click; after it a whole config, the notice goes | Pass |

A switch writes `config.json` from the five switches only: a key Contour
does not know is not kept. The original does the same on purpose
(`writeMemoryConfig` in open-design `apps/daemon/src/memory.ts`), so it is
not filed.

## 4. Export into a folder holding the name (not run)

In the web UI an export is a browser download; the browser names the file
itself (`page (1).html`) and never writes over one. In the app,
`apps/desktop/src/main.rs:save_file` opens the native Save panel, which asks
"Replace?" for a name that is there, and only then writes. Driving that
panel needs the GUI, so this scenario needs a manual pass on `Contour.app`.

### QA-126 🟡 An export that fails half way has already emptied the file it replaces (from the code, not reproduced)

`save_file` writes with `tokio::fs::write(&path, bytes)`: it opens the file
the user agreed to replace with `O_TRUNC` and writes into it. When the write
stops (disk full, a USB drive pulled, a network folder gone), the old file
is already empty or cut, and the new one is not complete. A link at that
path is also written through (the file it points at changes).

- Expected: the old file stays whole until the new one is complete, as
  every other user file write since #356 (temp file in the same folder,
  fsync, rename).
- Fix: write `bytes` to a temp file beside `path` (`O_CREAT|O_EXCL`), sync,
  rename over `path`; remove the temp file on error.
- Same pattern elsewhere: `copy_skill_to_claude` already uses
  `Guard::make_tree`; no other Tauri command writes a user-picked path.

## Not covered

- The native Save panel (scenario 4) and Copy to Claude Code: desktop GUI.
- A `kill -9` inside a delete: deletes of 600 files finished before the
  first kill moment that came after them; US-23 (uninstall of 1 500 files,
  kill on the first staging name) is the one that landed inside.
- Windows (junctions, `remove_staged`): `od-fs` unit tests only.
