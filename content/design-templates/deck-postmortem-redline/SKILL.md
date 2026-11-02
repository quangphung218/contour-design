---
name: deck-postmortem-redline
en_name: "Redline incident postmortem"
en_description: "A blameless postmortem printed like a marked-up report: timeline in the margin, red-pen notes and owned actions."
description: |
  An engineering postmortem deck that looks like a printed report
  marked up in red pen: a timestamped timeline in the margin, struck-out
  assumptions, circled numbers and an action table with owners. For
  incident reviews, outage write-ups and reliability retros.
triggers:
  - "postmortem"
  - "incident review"
  - "outage report"
  - "blameless retro"
  - "sre deck"
  - "reliability review"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: engineering
  category: "incident-review"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Create a blameless incident postmortem deck styled as a red-pen marked-up report, with timeline, root cause and owned actions."
---

# Redline incident postmortem

A made-up payments platform, Tallyrun, reviews a 47-minute checkout outage: impact, a minute-by-minute timeline, why it happened, what went well and the actions with owners and dates.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Report paper `#fdfcf8`, ink `#1e1e1e`, red pen `#d22b2b` for annotations only, highlighter `#fff27a` for one phrase per slide. Body in Charter / Iowan Old Style / Georgia like a printed doc; timestamps and code in the system monospace; red-pen notes in Bradley Hand / Segoe Print.

## Slide vocabulary

- `cover`: report title page with incident ID and severity stamp
- `impact`: three numbers, the worst one circled in red
- `timeline`: margin timeline: UTC time, event, red notes
- `cause`: the assumption, struck out, and what was true
- `went-well`: two columns: helped / hurt
- `actions`: action table with owner, due date, ticket

## Writing it

- Blameless: name systems and decisions, never people, in the cause and timeline.
- Red pen is for the reviewer's voice only; facts stay in black ink.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
