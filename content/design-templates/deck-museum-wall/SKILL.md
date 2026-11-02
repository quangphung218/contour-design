---
name: deck-museum-wall
en_name: "Gallery-wall exhibition proposal"
en_description: "A city museum pitches an exhibition about forgotten river trades, laid out like wall text and object labels."
description: |
  A curatorial proposal deck styled as a gallery wall: bone-white
  walls, black wall text in a high-contrast serif, small object labels
  with accession numbers, and generous empty space. For exhibition
  proposals, cultural programmes and collection stories.
triggers:
  - "exhibition proposal"
  - "museum deck"
  - "curatorial pitch"
  - "gallery wall text"
  - "cultural programme"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: strategy
  category: "culture"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create an exhibition proposal deck for a museum board in a gallery-wall style, with object labels, room plan, audience and budget."
---

# Gallery-wall exhibition proposal

The (invented) Harwick City Museum proposes a 2027 exhibition, “The River Workers”, to its board: the story, three key objects, the room plan, who it is for and what it costs.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Gallery bone `#efece6`, wall black `#111111`, label grey `#6e6a63`, and one river blue `#2d5d7c` for wayfinding. Wall text in Didot / Bodoni 72 / Georgia; labels in Helvetica Neue at small sizes with accession numbers in monospace.

## Slide vocabulary

- `cover`: exhibition title as wall text, dates in the corner
- `statement`: curatorial statement in two columns
- `objects`: three objects as framed placeholders with full labels
- `plan`: room plan in SVG: four rooms and a route
- `audience`: visitor targets as a short table
- `ask`: budget and the board decision

## Writing it

- Object labels follow one format: title, maker, date, material, accession number.
- Keep at least 40% of each slide empty; the room should breathe.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
