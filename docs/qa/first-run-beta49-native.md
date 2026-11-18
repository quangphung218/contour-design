# QA: a new user's first design on 0.2.0-beta.49, the native part

The macOS steps that [first-run-beta47.md](first-run-beta47.md) could not
run because the owner was at the Mac: install from the DMG, Gatekeeper
and the README's `xattr` step, the desktop window's first run, the agent
screen, one design, Export through the native Save panel (HTML, PDF, PNG),
and closing or quitting. Released `Contour_0.2.0-beta.49_universal.dmg`,
macOS 26, Apple silicon, 2026-11-18 23:50 to 2026-11-19 00:12.

**The first design does not get past the agent screen as shipped.** The
agent test fails four times in a row: each run of Claude Code makes macOS
ask for the Documents folder again, and "Allow" is not kept, because the
app in the DMG has no sealed signature (QA-112). With the same app
ad-hoc signed, macOS asks once and the rest of the flow works: first
design in 90 s, then HTML, PDF and PNG through the Save panel, each saved
in about a second. Two more bugs: the PDF and PNG are taken before the
page's entrance animation ends, so the PDF has no headline (QA-113), and
quitting during a run is reported as a lost connection (QA-114).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## How it was run

- **Owner's app untouched.** The owner's Contour (`dev.contour.desktop`,
  pid 65499, started 16:18:32) ran the whole time and had the same pid and
  start time at the end. It was not quit, focused or read.
- **QA copy.** The `.app` from the DMG was copied to a scratch folder and
  its `Info.plist` changed: `CFBundleIdentifier`
  `dev.contour.desktop.qa`, name `ContourQA`. The release's executable is
  only linker-signed and its `Info.plist` is not bound, so this changes
  nothing else (`codesign -v` says "not signed at all" before and after).
  The new identifier gives the copy its own WebKit storage, preferences
  and TCC entries. It was opened with its own data folder, which also
  turns the single-instance plugin off (`apps/desktop/src/data_lock.rs:38`):

  ```sh
  open -n --env OD_DATA_DIR=<scratch>/data ContourQA.app
  ```

  `HOME` was the real one, so the signed-in `claude` (2.1.327) could be
  used. Nothing in onboarding writes to `~/.claude*`.
- **Quarantine as a browser sets it.** `gh` sets no quarantine, so the DMG
  got `com.apple.quarantine` = `0083;…;Safari;…`. Finder could not be
  scripted (no Apple Events permission), so the drag was a `ditto` copy
  with the same quarantine set on every file, as Finder does.
- **Driving.** agent-desktop, on the copy's process only. Text went in by
  clipboard paste (the owner's clipboard was restored), because Telex
  rewrites typed keys. `HIDIdleTime` was read before each step. It went
  under 60 s only after the tool's own input.
- The exported files, the scratch folder and the copy were removed. Two
  TCC rows for `dev.contour.desktop.qa` stay (Documents allowed, App
  Management denied from an earlier run): `tccutil` does not find a copy
  under `/private/tmp`.

## Time per step

| # | Step (what the user does) | Time | Clicks | Had to guess or read |
|---|---|---|---|---|
| 1 | Download the DMG (59 MB) and check `SHA256SUMS` | 13 s, 1 s | browser | |
| 2 | Open the DMG, drag Contour out | under 1 s, 3 s copy | 2 | An older Contour DMG still mounted: this one mounts as "Contour 1" |
| 3 | Open it: "“Contour” Not Opened (here “ContourQA”). Apple could not verify “Contour” is free of malware…", buttons **Move to Trash** and **Done** | dialog in under 3 s | 2 | README ▸ Install, as documented |
| 4 | Run `xattr -dr com.apple.quarantine …`, open again | 3 s to the agent screen | 1 + Terminal | |
| 5 | Agent screen: Claude Code picked and tested by itself → "Test timed out after 45002 ms. Claude Code did not pass its test. Update it, or install one of these instead." Two "would like to access files in your Documents folder" dialogs on top | 45 s per try, 4 tries, 4 Allows | 8 | Yes: the advice points at the agent, the cause is the dialog (QA-112) |
| 5b | Same, app ad-hoc signed: one Documents dialog, Allow, test passes; Check again passes in 5 s with no dialog | 23 s from open | 1 | |
| 6 | Continue → Home | 2 s | 1 | |
| 7 | "Landing page for a tea shop, warm colours, menu and opening hours", Enter → Studio, page written, "Your first design is ready" | 90 s | 1 | |
| 8 | Export ▸ HTML (single file) → Save panel as a sheet (Where: Downloads, Save As: `tea-shop-landing`) → Save | panel at once, file under 1 s | 3 | |
| 9 | Export ▸ PDF → Save | panel within 5 s, file 1 s, 4 pages | 3 | PDF has no headline (QA-113) |
| 10 | Export ▸ Image ▸ PNG → Save | panel within 4 s, file under 1 s, 1920×2553 | 4 | Headline faded (QA-113) |
| | **From the first open to three files, signed copy** | **≈ 4 min** | **≈ 16** | |

Other native paths checked:

- Cancel in the Save panel: no file, no message, the Studio stays as it
  was.
- Quit with nothing running: no question, the app is gone within 2 s.
- The window's close button during a run: the window closes, the run goes
  on and finishes (13 s later), and opening the app again shows the same
  window, one process.
- ⌘Q (Quit Contour in the menu) during a run: the app quits at once with
  no question (QA-114).

## Top issues, by how often a new user meets them

| # | Finding | Step | New user meets it | Effect |
|---|---|---|---|---|
| 1 | QA-112 🔴 macOS asks again for Documents on every agent run; Allow is not kept | 5 | Whenever the agent touches `~/Documents`, `~/Desktop` or `~/Downloads`: a project or Claude Code settings there, File ▸ Open Folder on a repo in `~/Documents` | The agent test never passes; the advice says to update the agent |
| 2 | QA-113 🟠 PDF and PNG are taken during the page's entrance animation | 9, 10 | Often: the agent added one unasked; 47 bundled content files use `animation: … both` | The PDF has no headline, lede or buttons; the PNG shows them faded |
| 3 | QA-114 🟡 ⌘Q during a run cuts it with no question, then calls it a lost connection | – | Sometimes: quitting while the agent works | The user is told to retry a connection; the change had landed |
| 4 | QA-115 🔵 Troubleshooting is titled with old Gatekeeper wording | 3 | Users who search the docs for the dialog's words | The section is not found by its title |

### QA-112 🔴 The DMG's app is not sealed, so macOS does not keep "Allow" for Documents and the agent test fails

**Steps.** Install beta.49 from the DMG as the README says. Have Claude
Code read anything under `~/Documents` (here `~/.claude/settings.json`
loads a plugin from `~/Documents/GitHub/…`; a project folder there does
the same). Open Contour.

**Expected.** macOS asks once; after Allow, the test passes.

**Actual.** Each test run gets one or two "“Contour” would like to access
files in your Documents folder" dialogs. Allow is recorded in TCC
(`kTCCServiceSystemPolicyDocumentsFolder`, auth 2), yet the next run asks
again. The test waits for the dialog and times out after 45 s, four times
in a row. The screen then says "Claude Code did not pass its test. Update
it, or install one of these instead.", which points at the wrong cause.

**Cause.** The release's `Contour.app` has only the linker's ad-hoc
signature on the executable: `codesign -dv` shows `Sealed Resources=none`
and `Info.plist=not bound`, and `codesign -v` reports "code object is not
signed at all". macOS cannot match the recorded grant to the app. After
`codesign --force --deep -s - Contour.app`, the same copy asks once, and
Check again passes in 5 s with no dialog.

**Fix.**

- `.github/workflows/release.yml` (Tauri bundle step, around line 288):
  without `APPLE_SIGNING_IDENTITY`, sign ad-hoc. Either set
  `APPLE_SIGNING_IDENTITY=-` for `tauri bundle`, or run `codesign --force
  --deep -s -` on the `.app` before the DMG is made. Each version then
  gets a new identity and macOS asks once per update. App Management has
  the same cause (troubleshooting.md, "macOS keeps asking…").
- The connection test: when it times out, name the macOS dialog as a
  likely cause ("macOS may be asking to allow Contour: answer the
  dialog, then Check again") instead of "Update it".

Not checked: a design run (not only the test) on the unsealed copy. The
same cause applies.

### QA-113 🟠 PDF and PNG export catch the page mid-animation: the PDF has no headline

**Steps.** A page with an entrance animation (here the agent's own,
[03-entrance-animation-css.txt](first-run-beta49-native/03-entrance-animation-css.txt):
`animation: rise .8s … both` from `opacity: 0`). Export ▸ PDF and Export
▸ Image ▸ PNG.

**Expected.** The page as the preview shows it once loaded.

**Actual.** PDF page 1 has the nav, the eyebrow and the photo, but no
headline, lede or buttons
([01](first-run-beta49-native/01-pdf-page1-hero-missing.png)). The PNG
has them part-way faded
([02](first-run-beta49-native/02-png-hero-faded.png)). The exported HTML
and the preview are right.

**Cause.** `settle()` in `crates/od-render-cdp/src/script.rs:21` waits
for fonts, images and two animation frames, not for CSS animations, so
the capture runs about 0.1 s into a 0.8 s animation.

**Fix.** In `settle()`, after fonts and images, finish the finite
animations (`document.getAnimations().forEach(a => { try { a.finish() }
catch {} })`; infinite ones throw and are left), or emulate
`prefers-reduced-motion: reduce` before the page loads. Same path, same
bug: JPEG and WebP, and the per-slide capture (`show_slide`). MP4 sets
the animation time itself (`script.rs:179`) and is not affected.

### QA-114 🟡 ⌘Q during a run: no question, and the turn reads "Lost the connection to Contour"

**Steps.** Send a change in the chat. While the agent works, ⌘Q (or
Quit Contour in the menu). Open Contour again and the project.

**Expected.** Either a question before quitting, as CHANGELOG says for
other ways out "while work would be lost", or at least a turn that says
it was stopped by quitting.

**Actual.** The app quits at once. The turn shows **Failed**: "Lost the
connection to Contour, so this turn was cut off. Retry once it is back."
The file already had the change (new headline, "Order online" button).

**Where.** The text is `RunAppClosedHint` (`crates/od-ui/src/i18n.rs:1558`,
both locales): the code knows the app closed, the words say the
connection was lost. `docs/features.md:358` says quitting stops running
agents, so whether ⌘Q asks first is the owner's call. The text should
say "Contour was quit while this turn ran" and that the file may already
hold part of the change.

### QA-115 🔵 Troubleshooting's Gatekeeper section is titled with old wording

`docs/troubleshooting.md:284` is titled "Contour is damaged and can't be
opened" / "unidentified developer". On macOS 15 and later the dialog says
"“Contour” Not Opened" and "Apple could not verify…". getting-started.md
already quotes the new wording; add it to the title.

## Backlog (rare, or not Contour's)

- With an older Contour DMG still mounted, the new one mounts as
  "Contour 1" (macOS behaviour).
- After the first connection test the log has two `state.json write
  failed … No such file or directory` warnings for the test's run. The
  user does not see them.
- The first test raised two Documents dialogs at once, stacked.
- On macOS 26, `open` of a quarantined copy starts the process
  (translocated) and holds it at `_dyld_start` until the dialog is
  answered. Done ends it cleanly.
