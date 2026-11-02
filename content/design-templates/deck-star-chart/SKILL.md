---
name: deck-star-chart
en_name: "Star-chart public lecture"
en_description: "An astronomy society's evening talk on finding planets from a city balcony, with star charts drawn in SVG."
description: |
  A public-lecture deck on deep navy with star charts drawn in SVG:
  constellation lines, magnitude-sized stars, a thin gold accent and
  generous dark space. For science talks, planetarium nights and
  education evenings.
triggers:
  - "astronomy"
  - "public lecture"
  - "star chart"
  - "science talk"
  - "night sky deck"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: education
  category: "public-lecture"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Make a public-lecture deck about stargazing from a city, with star charts drawn in SVG on a deep navy background."
---

# Star-chart public lecture

The (fictional) Kellmore Astronomical Society gives a free evening talk: how to find five planets from a city balcony with nothing but your eyes and binoculars.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Deep navy `#0a1230` to `#060a1c`, starlight `#f4f1e6`, gold `#e9c46a` for the one thing to look at, faint chart lines `#5d6a9a`. Headlines in the system serif (Baskerville, Iowan Old Style, Georgia); labels in small caps sans with wide tracking.

## Slide vocabulary

- `cover`: title over a scattered starfield
- `chart`: a constellation chart with the target star in gold
- `scale`: a magnitude scale: dots sized by brightness
- `timeline`: a month of evenings: which planet when
- `checklist`: kit and habits in two columns
- `closing`: next meeting and where to stand

## Writing it

- Star size encodes brightness; never resize stars for decoration.
- Gold marks the one object the audience should find on that slide.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
