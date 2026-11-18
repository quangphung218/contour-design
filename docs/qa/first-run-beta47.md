# QA: a new user's first design on 0.2.0-beta.47

A first-run pass of the released beta.47 as a new user would make it: a
developer who already has an agent CLI. Download and install as the
README says, open on an empty data folder, connect the real Claude Code
CLI, make a page from one sentence, change it once, and export PNG, PDF
and HTML. macOS 26, Apple silicon, 2026-11-18. Edge cases (two windows,
emoji, races) were out of scope.

**A new user gets a first design out without help.** From opening the app
to three exported files took about 2 min 20 s, 98 s of it the agent
writing the page, and 10 clicks. Nothing had to be guessed or looked up
after the install step. One bug changes what the user gets: the web font
the agent picks is not loaded in the preview, the PDF or the PNG, while
the exported HTML loads it, and nothing says so (QA-110). One minor
(QA-111).

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## How it was run

- **Release under test:** `Contour_0.2.0-beta.47_universal.dmg` from
  GitHub Releases, checked against `SHA256SUMS` (OK), copied out of the
  DMG into a scratch folder (not `/Applications`), and
  `xattr -dr com.apple.quarantine` run on it as the README says.
- **Window part:** the owner was at the Mac (`HIDIdleTime` 5 s), so the
  native steps were not run: dragging to Applications, Gatekeeper's
  dialog, the desktop window, and the Save dialog of the desktop app. The
  owner's running Contour (beta.47, `dev.contour.desktop`) was not
  quit, opened or touched.
- **Web part:** the released binary's own daemon with an empty data
  folder, as [getting-started.md](../getting-started.md#run-the-installed-apps-daemon-without-its-window)
  describes for QA, and the release's UI in Playwright Chromium
  (1440×900):

  ```sh
  OD_DATA_DIR=<empty folder> <scratch>/Contour.app/Contents/MacOS/od-desktop daemon start --port 27461
  ```

- **Agent:** the real `claude` (Claude Code 2.1.327) already signed in on
  the machine; two chat turns (the brief and one change) plus the
  onboarding check.
- Each step was driven one at a time and timed from the UI (Playwright)
  and from the run records in `app.sqlite`.

The automated version of this flow, with the mock agent, is
[`firstrun.qa.mjs`](../../crates/od-ui/e2e/qa/firstrun.qa.mjs) (FR-01,
test-plan §30): `make e2e-qa` or `crates/od-ui/e2e/qa/run.sh firstrun`.

## Time per step

| # | Step (what the user does) | Time | Clicks | Had to guess or read | Screenshot |
|---|---|---|---|---|---|
| 1 | Download the DMG (59 MB) and check `SHA256SUMS` | 94 s download, 1 s check | browser | README ▸ Install (one `xattr` line) | |
| 1b | Drag to Applications, first open past Gatekeeper | not run (owner at the Mac) | | README gives `xattr` or Open Anyway | |
| 2 | First open: onboarding with Claude Code already picked and tested ("replied in 4036 ms: 'ok'") | page in < 6 s, test 4.0 s | 0 | nothing | [01](first-run-beta47/01-onboarding.png) |
| 3 | Continue (brand step skipped) → Home | 1.6 s | 1 | nothing | [02](first-run-beta47/02-home.png) |
| 4 | Type "Landing page for a tea shop, warm colours, menu and opening hours", Enter → Studio, page written | Studio at once; first file at 75 s; turn done at 98 s | 1 | nothing | [03](first-run-beta47/03-first-design.png) |
| 5 | One change in the chat ("Tea, taken slowly." + an "Order online" button) | 19 s; the preview reloaded by itself | 1 | nothing | [04](first-run-beta47/04-after-edit-pdf-exported.png) |
| 6 | Export ▸ HTML (single file) | 0.1 s | 2 | nothing | [05](first-run-beta47/05-export-menu.png) |
| 7 | Export ▸ PDF (4 A4 pages, text correct) | 1.2 s | 2 | nothing | |
| 8 | Export ▸ Image ▸ PNG (1920×2623, whole page) | 2.5 s | 3 | nothing | [06](first-run-beta47/06-png-export.png) |
| | **From first open to three files** | **≈ 2 min 20 s** | **10** | | |

The three files open: the HTML in a browser shows the changed page and its
menu tabs work; the PDF has the changed headline and all four sections;
the PNG is the whole page at 2×.

What went well, for whoever changes these screens: the agent is picked
and tested before the user does anything; Enter on Home goes straight to
the Studio; "Your first design is ready" appears once the page is written;
the export toasts say what is happening ("Exporting png…", "Exported
tea-shop-landing.pdf").

## Top 5 by effect on a new user's first design

| # | Finding | Step | New user meets it | Effect |
|---|---|---|---|---|
| 1 | QA-110 🟠 The web font the agent links is blocked in the preview, the PDF and the PNG; the exported HTML loads it; nothing tells the user | 4–8 | Often: the real agent did it in this run, and 84 bundled skills, templates and design systems link Google Fonts | Works, but the PDF and PNG are not the design the agent described, and differ from the HTML |
| 2 | QA-111 🔵 Home's design-system and template pickers are cut to "No design sys…" and "Default templ…" at 1440 px | 4 | Always, at 1440 px and narrower | Only annoying: the user cannot read what the two pickers are |
| 3 | The agent answers in the language of the user's own `~/.claude/CLAUDE.md` (here Vietnamese) to an English brief in an English UI | 4–5 | Every user with personal Claude Code instructions | Expected: Contour runs the user's own CLI with its settings. Not a bug; noted so nobody files it |
| 4 | The first turn takes 98 s with "The agent is working" and no file for 75 s | 4 | Always with a real agent | Only waiting; the chat shows "Thinking" and a timer, so the user knows it runs |
| 5 | Desktop-only steps (Gatekeeper, the window, the Save dialog) not run | 1b, 6–8 | — | Unknown for this pass; covered by earlier desktop passes ([bugs-desktop-beta39.md](bugs-desktop-beta39.md)) |

## QA-110 🟠 Web fonts are blocked in the preview, PDF and PNG but not in the exported HTML, and the user is not told

**Steps (real agent):** new project from the brief above. The agent wrote
`tea-shop-landing.html` with
`<link href="https://fonts.googleapis.com/css2?family=Instrument+Serif…" rel="stylesheet">`
and said in the chat that headlines use Instrument Serif. Export ▸ PDF
and ▸ PNG.

**Expected:** what the user sees and exports is the page the agent wrote,
or the user is told why it is not. The original app's preview is a
`srcdoc` iframe with no CSP on fonts (`apps/web/src/runtime/srcdoc.ts`),
so its preview loads the font and matches the HTML.

**Actual:**

- The preview's CSP (`od-preview/src/lib.rs:27`, `style-src 'self'
  'unsafe-inline'`) blocks the stylesheet; the console shows the CSP
  error, the Studio shows nothing.
- PDF and PNG: `od_render_cdp` blocks the request ("render blocked a
  request" in the daemon log); `pdffonts` shows Iowan Old Style (the
  system serif) instead of Instrument Serif.
- The exported HTML keeps the link and lists the font in
  `od-standalone-manifest` `externalDependencies`; opened in a browser
  with network, it shows Instrument Serif. So HTML and PDF/PNG of the
  same page differ.
- The agent is not told that web fonts will not load. 84 bundled files
  (skills, templates, design systems) use Google Fonts, so agents are
  steered toward them. The gallery strips them on purpose
  (`without_google_fonts`, `routes/content.rs:132`), but a user's own
  project is not stripped.

**Evidence:** [qa110-fonts.txt](first-run-beta47/qa110-fonts.txt)
(`pdffonts`, the HTML's links, the daemon log, the console error).

**Needs a decision (no red test yet: the expected result depends on it).**
Blocking is on purpose (nothing leaves the machine; test-plan §15). The
fix is one of:

1. Tell the agent: a line in the system prompt that the preview and
   exports load no web fonts, so it uses a local font stack (or embeds
   the font as a file in the project). Cheapest, keeps the privacy rule.
2. Show it: a notice in the Studio and on export ("1 web font not loaded:
   Instrument Serif; PDF and PNG use a system font"), from the same list
   the export manifest already builds.
3. Allow `fonts.googleapis.com` and `fonts.gstatic.com` in the preview and
   the renderer, behind a setting. Matches the original app's preview,
   but sends requests off the machine.

Recommended: 1 and 2. Same class: any `<script src>`, image or CSS from a
CDN in the agent's page has the same preview/HTML mismatch.

## QA-111 🔵 Home's design-system and template pickers are cut off

**Steps:** first open, Continue, Home at 1440×900.

**Expected:** the two pickers say what they are, e.g. "No design system"
and "Default template".

**Actual:** "No design sys…" and "Default templ…"
([02](first-run-beta47/02-home.png)); a new user has to open them to
learn what they are. Found by eye, no test.
