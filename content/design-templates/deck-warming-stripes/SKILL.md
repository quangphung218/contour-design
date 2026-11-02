---
name: deck-warming-stripes
en_name: "Climate-risk council briefing"
en_description: "A climate-risk briefing for a town council, with warming-stripe bands, heat-day counts and a costed adaptation plan."
description: |
  A climate briefing deck built on warming-stripe bands: a year-by-
  year stripe of blues to reds frames every slide, with plain charts and
  costed actions. For councils, planning teams and climate adaptation
  proposals.
triggers:
  - "climate briefing"
  - "climate risk"
  - "warming stripes"
  - "adaptation plan"
  - "council briefing"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: policy
  category: "climate"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a climate-risk briefing deck for a town council with warming-stripe visuals, local heat and flood data and a costed adaptation plan."
---

# Climate-risk council briefing

An invented market town, Ellesby, briefs its councillors on local climate risk: how much warmer it got, more hot days and flash floods, who is most exposed, and five costed actions.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Stripe ramp from deep blue `#08306b` through white `#f7f7f7` to deep red `#67000d` (data only: one stripe per year, invented for this town). Page off-white `#fbfaf7`, ink `#1a1a1a`. Type: Avenir Next / Helvetica Neue for everything, figures in bold.

## Slide vocabulary

- `cover`: full-bleed stripes with the title on a white band
- `stripe`: the stripes again with a plain-language reading
- `days`: hot days per decade as columns
- `exposure`: who is exposed: three groups with counts
- `actions`: five actions with cost and timing
- `decision`: the vote and the first-year budget

## Writing it

- The stripes are data: one per year, coloured by anomaly. Replace with real local series before use.
- Never put text directly on the stripes; use the white band.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
