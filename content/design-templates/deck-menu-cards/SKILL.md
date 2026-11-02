---
name: deck-menu-cards
en_name: "Seasonal menu launch deck"
en_description: "A neighbourhood restaurant introduces its autumn menu to staff with dish cards, sourcing notes and service timings."
description: |
  A warm hospitality deck built from menu cards: terracotta and
  cream, deckled card edges, small-caps labels and illustrated plates in
  SVG. For menu launches, supplier pitches and food-and-drink briefings.
triggers:
  - "menu launch"
  - "restaurant deck"
  - "food deck"
  - "hospitality briefing"
  - "seasonal menu"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: marketing
  category: "food-and-drink"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Make a staff briefing deck for a restaurant's new seasonal menu, styled as menu cards with dishes, sourcing and service notes."
---

# Seasonal menu launch deck

Osteria Brava, an invented neighbourhood restaurant, briefs its front and back of house on the autumn menu: the idea, four new plates, where the produce comes from and how service changes.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Terracotta `#b9502e` on oat `#f3e7d3`, deep olive `#3b3a1f` for text, and a tomato `#d9442b` for prices only. Dish names in Didot / Bodoni 72 / Georgia; labels in Gill Sans / Avenir small caps.

## Slide vocabulary

- `cover`: a single menu card centred on terracotta
- `idea`: one line of philosophy and three ingredient tags
- `dishes`: a 2×2 grid of dish cards with price and allergens
- `sourcing`: a map-free list of farms with distance in km
- `service`: the evening as a timeline of courses
- `closing`: tasting session time and who to ask

## Writing it

- Prices only in tomato red; never use it for headings.
- Every dish card lists allergens with the standard letters, so staff can answer at the table.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
