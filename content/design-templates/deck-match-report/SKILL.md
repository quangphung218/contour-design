---
name: deck-match-report
en_name: "Floodlit season review"
en_description: "A youth football club's season review for parents, with a floodlit scoreboard, chalk pitch diagrams and next year's plan."
description: |
  A sports-club deck under floodlights: night-pitch green, a
  stadium scoreboard for the numbers, chalk-white pitch diagrams drawn in
  SVG, and a bright kit colour. For club season reviews, sports
  programmes and team briefings.
triggers:
  - "season review"
  - "sports club"
  - "football deck"
  - "team review"
  - "scoreboard"
  - "youth sport"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: education
  category: "sport"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a season review deck for a youth football club for parents: results on a scoreboard, a pitch diagram, development stats and next season's plan."
---

# Floodlit season review

Riverside Rovers (invented), a youth football club, reviews the season with parents: results, how the under-12s learned to build from the back, minutes played by every child, and what changes next year.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Night pitch `#0f3d22` with mowing stripes, chalk `#f5f5ef`, kit yellow `#ffd400`, scoreboard black `#0b0b0b` with LED amber `#ffb300`. Headlines in Futura / Avenir Next heavy, uppercase; numbers on the scoreboard in the system monospace.

## Slide vocabulary

- `cover`: floodlit pitch stripes with the club crest in SVG
- `scoreboard`: season results on an LED scoreboard
- `pitch`: a chalk pitch diagram with passing arrows
- `minutes`: minutes played per player as a dot strip
- `next`: three changes for next season
- `closing`: dates and volunteer asks

## Writing it

- The kit colour marks the club; keep opponents and neutral data in chalk.
- For youth teams, report team and development numbers, not individual rankings.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
