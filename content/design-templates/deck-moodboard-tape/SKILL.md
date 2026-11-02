---
name: deck-moodboard-tape
en_name: "Taped moodboard brand pitch"
en_description: "A small studio pitches a bakery rebrand as a taped-up moodboard: swatches, type samples, sketches and the plan."
description: |
  A creative pitch deck that looks like a studio wall: kraft-paper
  background, swatch cards and type samples held with masking tape,
  pencil arrows, and rough sketches in SVG. For brand refresh pitches,
  creative direction and design proposals.
triggers:
  - "moodboard"
  - "brand pitch"
  - "rebrand proposal"
  - "creative direction"
  - "design pitch"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: design
  category: "brand-pitch"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a brand refresh pitch deck for a bakery styled as a taped-up moodboard with swatches, type samples and sketches."
---

# Taped moodboard brand pitch

Studio Pell (invented) pitches a refresh to Crumb & Co., a family bakery with four shops: what customers say, the direction, palette and type, the new mark sketched, and the rollout.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Kraft `#d8c3a0` background, paper white cards, masking tape `#efe3b8` at 70% opacity, pencil `#4a4238`, and the proposed brand colours: rye `#8a5a2b`, crust `#d9822b`, flour `#fbf6ec`, oven black `#262019`. Headlines in Cooper / Rockwell / Georgia bold; notes in Marker Felt / Segoe Print.

## Slide vocabulary

- `cover`: pinned title card, slightly rotated, with tape
- `voices`: customer quotes on sticky notes
- `direction`: three words, each on its own card
- `palette`: swatch cards with names and hex
- `mark`: logo sketches in SVG, the chosen one circled
- `plan`: rollout as cards on a string

## Writing it

- Rotate cards between −3° and 3°; never more, or it reads as a mess.
- The pencil arrows carry one reason each; keep them under eight words.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
