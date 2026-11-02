---
name: deck-field-notebook
en_name: "Field-notebook annual report"
en_description: "A rooftop beekeeping co-op's year told like a naturalist's notebook: hive counts, honey yield and next season's asks."
description: |
  A warm annual-report deck styled as a naturalist's field notebook:
  cream ruled paper, olive ink, hand-drawn hexagon marks and pressed-leaf
  labels. For community groups, co-ops and nonprofits reporting a year
  to members.
triggers:
  - "annual report"
  - "co-op report"
  - "nonprofit annual report"
  - "field notebook"
  - "botanical deck"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: operations
  category: "annual-report"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create an annual report deck for a community co-op in a field-notebook style: the year in numbers, what worked, and the asks for next season."
---

# Field-notebook annual report

Hivewise, an invented rooftop beekeeping co-op, reports its year to members: hives, honey, swarms caught, the treasurer's line, and three asks for next season.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Notebook cream `#f6f0df` ruled every 48px in pale blue; olive ink `#3f4a24`; honey `#d99a1e` for the one number that matters on each slide; pencil grey `#7b7466` for annotations. Headlines in the system serif (Iowan Old Style, Palatino, Georgia) italic-free; notes in Bradley Hand / Segoe Print with a cursive fallback, used sparingly.

## Slide vocabulary

- `cover`: title on the ruled page with a hexagon cluster drawn in SVG
- `tally`: three big numbers as notebook entries, each with a pencil note
- `chart`: honey by month as bars drawn on the ruled lines
- `specimen`: a labelled specimen plate: one hive, its parts and what changed
- `asks`: numbered asks with the member action under each
- `closing`: thank-you note with a signature line

## Writing it

- Keep honey `#d99a1e` for one figure per slide; everything else stays in olive ink.
- The handwriting face is for marginal notes only, never for headlines or numbers.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
