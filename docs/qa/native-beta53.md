# QA: native check of 0.2.0-beta.53

Released `Contour_0.2.0-beta.53_universal.dmg`, macOS 26, Apple silicon,
2026-11-22 15:37–15:46.

**Criteria 1–3 pass.** The deck PDF fills every page (QA-120 fixed), the
editable PowerPoint keeps the slide background and text colours (QA-121
fixed), and Remix copies the files a template loads (#286). The editable
PowerPoint has two new faults: every sprite-based SVG comes out as an
empty picture (QA-124), and text that follows an icon or swatch in a row
starts on top of it (QA-125).

| # | Criterion | Result |
|---|---|---|
| 1 | Deck from a template (Remix), Export PDF through the Save panel: one slide per page, slide fills the page, text and colours as on screen | **Pass** |
| 2 | Same deck, PowerPoint ▸ Editable text: background from the slide (not `#08090d`), text has colour | **Pass** (new QA-124, QA-125) |
| 3 | Deck and dashboard templates with `assets/` open with styles and scripts after Remix | **Pass**: `html-ppt-zhangzara-pin-and-paper` (deck), `trading-analysis-dashboard-template` (dashboard) |
| 4 | Update now | **Not run**, see below |

Severity: 🔴 data loss or main flow blocked · 🟠 wrong result or hard to
get out of · 🟡 poor or confusing message · 🔵 minor.

## New bugs

| ID | Sev | Bug |
|---|---|---|
| QA-124 | 🟠 | PowerPoint ▸ Editable text: every `<svg><use href="#…">` icon is an empty, fully transparent picture (34 of 35 pictures in the test deck) |
| QA-125 | 🔵 | PowerPoint ▸ Editable text: in a flex row "icon + text", the text box starts at the row's left edge, so it sits on the icon or swatch |

### QA-124 🟠 Sprite SVGs export as empty pictures

- Steps: Templates ▸ Slide deck ▸ "Turn a Field-Study Capstone into a
  Defense Deck like a Star Biology Major" ▸ Remix ▸ Export ▸ PowerPoint ▸
  Editable text ▸ Save. Unzip and look at `ppt/media/*.png`.
- Expected: the safety pins and the small "⟵o" mark beside each kicker, as
  in the preview and the PDF ([PDF](native-beta53/05-pdf-slide6-left.png)).
- Actual: 35 pictures; 34 have no pixel with alpha > 0 (pins 228×101,
  1337×689…, marks 64×32). Only the chart, an inline `<svg>` with its own
  `<polyline>`s, is drawn. In the slide the pins and marks are missing
  ([PPTX in LibreOffice](native-beta53/06-pptx-editable-slide6-left.png)).
- Cause: `crates/od-export/src/pptx_layout.js` `toPng` clones the
  `<svg>` alone and loads it as `data:image/svg+xml`. The clone holds only
  `<use href="#pin">`; the `<symbol>` sits in the page's hidden sprite
  (`<svg width="0" height="0"><defs>…`), which is not in the image, so
  nothing draws. `stroke="currentColor"` is also lost outside the page.
- Fix direction: before serialising, copy each referenced symbol (or the
  sprite's `<defs>`) into the clone, and set `color` on the clone from the
  element's computed colour.
- The PDF and PowerPoint ▸ Slides as images are not affected (they
  screenshot the page).

### QA-125 🔵 Text box sits on the icon before it

- Steps: same deck, slide 6, Editable text.
- Expected: legend "— Community garden (least paved)": a 32 px swatch,
  14 px gap, then the text; kicker "⟵o Visitation rate, by site" the same.
- Actual: the text box starts at x = 32 pt, the same x as the swatch
  (`Text 9` and `Shape 10` both at `off x=406400`); in HTML the text
  starts at 55 pt. The swatch is drawn over the first letters
  ([slide 6](native-beta53/04-pptx-editable-slide6.png)). The kicker has
  the same offset, hidden today only because its mark is empty (QA-124).
- Where to look: `pptx_layout.js` takes the text box from the block's
  `getBoundingClientRect()` while skipping `SVG`/`I` children; the box
  should start at the first text node (a `Range` over the text) instead.

## Criterion 1: Deck PDF

| Check | Result |
|---|---|
| Save panel after Export ▸ PDF | 9 s, proposes `index` in Downloads |
| File | 17.6 MB, Skia/PDF, 11 pages, each 960×540 pt |
| Fill | All four corners and an inner point of every page carry the slide colour (yellow `EDE579`… or blue `2B448F`…), no white band |
| Text | `pdftotext` has every line; page 3 at 200 dpi reads correctly |
| Against the screen | Slides 1, 3 and 6 match the preview: fonts, italics, pins, chart ([screen](native-beta53/02-screen-slide6.png), [PDF](native-beta53/03-pdf-page6.png)). The deck's own navigation pill is not printed, as expected |

## Criterion 2: Editable PowerPoint

| Check | Result |
|---|---|
| Save panel after Export ▸ PowerPoint ▸ Editable text | 3 s, proposes `index-editable` |
| Slides | 11 |
| Background (`<p:bg>`) | `EFE56A` on the 9 yellow slides, `1F3A8A` on slides 4 and 6; no `08090D` in any slide |
| Text colour | 231 of 231 `<a:r>` runs have a `solidFill`, ink `1F3A8A` on yellow slides, `EFE56A` on blue slides, `C2342B` red notes on slide 8 |
| Rendered (LibreOffice headless to PDF) | Colours as in the preview ([slide 6](native-beta53/04-pptx-editable-slide6.png)); fonts fall back to a serif because the deck's web fonts are not on the machine |

The chart's axis labels and title are yellow on the yellow chart card in
the PPTX; the template does the same on screen (`.yhead`, `.yticks` take
`--paper`), so this is not an export fault.

## Criterion 3: Remix brings its files

| Template | Kind | Files in the project after Remix | Opens with |
|---|---|---|---|
| `html-ppt-zhangzara-pin-and-paper` ("Turn a Field-Study Capstone into a Defense Deck…") | Deck | `index.html`, `assets/styles.css`, `assets/deck-stage.js`, all byte-identical to the template | Styled deck, 11 slides, slide rail, Present ([01](native-beta53/01-remix-pin-and-paper.png)) |
| `trading-analysis-dashboard-template` ("Trading terminal dashboard") | Dashboard | `index.html`, `assets/template.html`, byte-identical | Styled dark dashboard with panels and option chain ([07](native-beta53/07-remix-trading.png)) |

## Criterion 4: Update now, not run

Update now replaces the running bundle with the release bundle, whose
bundle id is `dev.contour.desktop`, the owner's. Restarting it would run
with the owner's WebKit storage, preferences and TCC rows, which this
test may not touch (same limit as in the beta.51 run). Not counted as a
failure.

## Timeline

| Time | Step |
|---|---|
| 15:37 | Start; owner idle 31 min; owner's Contour not running |
| 15:38 | DMG downloaded, SHA256 OK; copy made, signed, opened |
| 15:39 | Onboarding: Skip for now |
| 15:40 | Remix pin-and-paper |
| 15:41 | Export PDF, saved |
| 15:42 | Export PowerPoint ▸ Editable text, saved |
| 15:45 | Remix Trading terminal dashboard |
| 15:45 | Quit Contour (menu item); app and its Chrome gone |
| 15:46 | Cleanup done |

## How it was run

- **Owner's app untouched.** `dev.contour.desktop` was not running and was
  not opened. After cleanup, `find ~/Library/{Caches,WebKit,Preferences,Application Support} -maxdepth 1 -name 'dev.contour.desktop*' -newermt '2026-11-22 15:37'`
  printed nothing.
- **QA copy.** `ditto` from the DMG to `~/Applications/ContourQA53.app`,
  `CFBundleIdentifier` `dev.contour.desktop.qa53`, `CFBundleName`
  `ContourQA53`, `codesign --force --deep --sign -` (valid, adhoc,
  5594 files), `open -n --env OD_DATA_DIR=<scratch>/data`.
- **Driving.** agent-desktop accessibility presses on the copy's window;
  file names set in the Save panel with `set-value`; no text typed into
  the WKWebView. `HIDIdleTime` logged before each input, never under
  60 s. Window captures with `screencapture -l` of the copy only.
- **Checks.** `pdfinfo`, `pdftotext`, `pdftoppm`; PPTX unzipped and its
  XML read; pictures' alpha counted; LibreOffice headless (own profile
  folder) to render the PPTX.
- **Cleanup**, in this order: `tccutil reset All dev.contour.desktop.qa53`,
  `lsregister -u`, copy removed, caches, WebKit storage and preferences of
  the QA id removed, data folder, exports and DMG removed.

## Not checked

- Keynote and PowerPoint rendering of the PPTX (LibreOffice and the XML only).
- The lint badge "1" shown on the remixed deck's toolbar.
- Other exports (Slides as images, PNG, MP4) and other `html-ppt-zhangzara-*` decks.
