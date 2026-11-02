---
name: deck-type-specimen
en_name: "Type-specimen release deck"
en_description: "An independent type foundry launches a new serif family, presented as a specimen book: glyphs, weights and pairings."
description: |
  A typeface launch deck styled as a printed specimen book: oversized
  glyphs, weight ladders, a character grid, waterfall sizes and pairing
  examples, in black, paper and one ink colour. For type releases, brand
  type systems and typography talks.
triggers:
  - "type specimen"
  - "typeface launch"
  - "font release"
  - "typography deck"
  - "type foundry"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: design
  category: "typography"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a launch deck for a new typeface styled as a type-specimen book: hero glyphs, weights, character set, waterfall and pairings."
---

# Type-specimen release deck

Fernhill Type (invented) releases Corran, a text serif. Since the deck cannot ship a font file, it presents the family with the system's own serif as the stand-in and labels it as a placeholder.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Specimen paper `#f2efe8`, black `#0e0e0e`, one ink colour, vermilion `#e8401c`, for annotations and metrics lines. All type in the system serif stack (Iowan Old Style, Palatino, Georgia) as a placeholder for the real family; captions in Helvetica Neue.

## Slide vocabulary

- `cover`: one giant glyph with metrics lines
- `weights`: the weight ladder, one line per weight
- `glyphs`: a character grid with one cell highlighted
- `waterfall`: a sentence at falling sizes
- `pairing`: headline and text set together, with notes
- `release`: licences, prices and the release date

## Writing it

- Swap the placeholder serif for the real family with a `@font-face` pointing at a local file in the project, never a web font URL.
- Vermilion marks measurements and notes only, never the specimen text itself.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
