---
name: deck-lab-notebook
en_name: "Lab-notebook results readout"
en_description: "An assay readout on graph paper: hypothesis, plate layout, dose-response curve and the go/no-go call."
description: |
  A scientific results deck on graph paper: monospace labels,
  a plate-map grid, dose-response curves drawn in SVG, and a stamped
  decision box. For research readouts, lab meetings and R&D gates.
triggers:
  - "lab results"
  - "research readout"
  - "assay results"
  - "science deck"
  - "lab meeting"
  - "r&d review"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: research
  category: "research-readout"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a lab results readout deck on graph paper: hypothesis, method, plate map, dose-response curve and the decision."
---

# Lab-notebook results readout

An invented biotech team reads out a cell-viability assay for compound KX-114: what they expected, how the plate was laid out, what the curve shows, and whether it moves to the next stage.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Graph paper `#fbfdf8` with 24px minor and 120px major grid in pale green; ink `#16302b`; data in two colours only, compound teal `#127a6b` and control grey `#9aa5a1`; a stamp red `#c23b22` for the decision. Everything in the system monospace except headlines (Avenir Next demi).

## Slide vocabulary

- `cover`: experiment ID block like a notebook header
- `hypothesis`: hypothesis and the result that would falsify it
- `plate`: 96-well plate map, wells coloured by condition
- `curve`: dose-response curve with IC50 marked
- `table`: replicate table with mean and CV
- `decision`: a stamped GO / NO-GO box with the next experiment

## Writing it

- Every chart states n, units and the statistic used, on the slide.
- Use the stamp red only on the decision slide.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
