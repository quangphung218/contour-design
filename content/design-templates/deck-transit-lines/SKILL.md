---
name: deck-transit-lines
en_name: "Transit-map network proposal"
en_description: "A city bus-network redesign pitched in transit-diagram style: coloured lines, station dots and before/after trip times."
description: |
  A public-sector proposal deck drawn in the language of a transit
  diagram: thick coloured lines, white station dots, 45° bends and a
  route-number badge on every slide. For transport, infrastructure and
  network-change proposals.
triggers:
  - "transit map"
  - "bus network"
  - "public transport proposal"
  - "metro map style"
  - "network redesign"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: policy
  category: "public-sector"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a proposal deck for redesigning a city bus network in a transit-map visual style, with before/after travel times."
---

# Transit-map network proposal

The (invented) city of Port Aldor proposes a simpler bus network: fewer, straighter, more frequent lines. The deck walks a council committee from today's tangle to the new map.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Paper white `#fbfbf7`, near-black text `#141414`, and five line colours used as data: red `#e2312b`, blue `#1f63c6`, green `#1d9a5b`, yellow `#f2b705`, purple `#7b3fa0`. Type: Helvetica Neue / Arial bold for everything, tight and municipal; route badges are circles with the line number.

## Slide vocabulary

- `cover`: the new map drawn as SVG lines with a stop list
- `problem`: today's tangled routes, greyed, with the one number that hurts
- `map`: the proposed network: five coloured lines, interchanges as white rings
- `trips`: before/after travel times as paired bars per trip
- `timeline`: the phase-in plan as a line with station dots
- `decision`: what the committee is asked to approve

## Writing it

- Line colours are data: one colour per route, never decorative.
- Draw routes with 45° and 90° bends only; stations are white circles with a 6px black ring.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
