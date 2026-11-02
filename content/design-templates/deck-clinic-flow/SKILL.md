---
name: deck-clinic-flow
en_name: "Clinic patient-flow improvement"
en_description: "A community clinic's plan to cut waiting times, mapped as a patient journey with wait bars and a calm clinical palette."
description: |
  A healthcare improvement deck in a calm clinical style: soft
  teal, rounded cards, a patient-journey map with wait times on each
  step, and plain-language findings. For clinics, hospital teams and
  service-improvement proposals.
triggers:
  - "patient flow"
  - "clinic improvement"
  - "healthcare deck"
  - "waiting times"
  - "service improvement"
  - "patient journey"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: healthcare
  category: "service-improvement"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a service-improvement deck for a community clinic: the patient journey with wait times, causes, a pilot and results."
---

# Clinic patient-flow improvement

Larchfield Health Centre (invented) shows how a walk-in morning went from a 74-minute average wait to 41: the journey mapped step by step, where time was lost, the pilot, the results and what they ask for next.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Clinical white `#f7fbfa`, soft teal `#2a8c82`, deep teal ink `#123b38`, mint `#d6efe9` for cards, and a warm coral `#e0705a` for the steps where patients wait longest. Type: Avenir Next / Segoe UI / Helvetica Neue throughout; large, high-contrast, plain words.

## Slide vocabulary

- `cover`: headline with before/after wait as two big numbers
- `journey`: patient journey: six steps as rounded cards with minutes
- `causes`: three causes, each with the evidence
- `pilot`: what changed in the pilot, before/after columns
- `results`: wait time by week as a line chart in SVG
- `ask`: what the practice board is asked for

## Writing it

- Coral marks only the longest waits; everything else stays teal.
- Write for patients and staff alike: no acronyms on slides without the words beside them.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
