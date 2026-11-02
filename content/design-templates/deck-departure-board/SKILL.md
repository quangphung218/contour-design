---
name: deck-departure-board
en_name: "Departure-board ops review"
en_description: "A freight terminal's quarterly operations review shown as split-flap departure boards: on-time, dwell and delays."
description: |
  An operations review deck in the look of a split-flap departure
  board: black panels, amber characters in flaps, status columns and
  platform numbers. For logistics, transport and any operations team
  reporting on-time performance.
triggers:
  - "operations review"
  - "quarterly review"
  - "logistics deck"
  - "departure board"
  - "split flap"
  - "on-time performance"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: operations
  category: "ops-review"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a quarterly operations review deck for a freight terminal styled as a split-flap departure board."
---

# Departure-board ops review

An invented inland freight terminal, Easthaven Rail Hub, reviews its quarter: trains on time, container dwell, the worst delays and what changes next quarter, all on departure-board panels.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Board black `#111214`, flap charcoal `#1d1f23` with a hairline split, amber characters `#ffb000`, status green `#3ddc84` and red `#ff4d4d` used as data only. Everything in the system monospace, uppercase on the boards; sentences in Helvetica Neue.

## Slide vocabulary

- `cover`: a board header with the review title in flaps
- `board`: departure board rows: service, destination, time, status
- `kpi`: three flap counters for the quarter's numbers
- `delays`: top delay causes as a board with minutes lost
- `changes`: next quarter's changes as platform announcements
- `closing`: the ask in one board line

## Writing it

- Status colours are data: green on time, amber late under 15 min, red cancelled or over 15.
- Flap text in caps, max 18 characters a cell; longer text goes in the sentence line under the board.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
