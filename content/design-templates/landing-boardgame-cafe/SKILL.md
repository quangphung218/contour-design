---
name: landing-boardgame-cafe
en_name: "Board-game café landing"
en_description: "A board-game café's page built from game pieces: dice, meeples and a token track for booking a table."
description: |
  A playful hospitality landing built from game-board pieces: bold
  primary tokens, dice drawn in CSS, a winding track for the steps, and
  chunky rounded type. For cafés, family venues, games and events.
triggers:
  - "board game cafe"
  - "playful landing"
  - "cafe website"
  - "family venue"
  - "games night"
  - "booking page"
  - "landing page"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: personal
  category: "hospitality"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [typography, color]
  example_prompt: "Build a playful landing page for a board-game café with a game-board look: how it works, the game library, prices and booking."
---

# Board-game café landing

Roll & Brew (invented), a board-game café, shows how a visit works, the library's most-played games, prices per table and the weekly events, with a booking call to action.

## What ships

- `example.html`: one responsive page, no script needed. It reads at 1440 px and folds to a single column under 760 px.
- No network request: system font stacks, CSS shapes and inline SVG only. No web fonts, no CDN, no remote images.

## Visual system

Table felt `#1f6f5c`, card cream `#fff7e6`, and four player colours used as tokens: red `#e5484d`, yellow `#f5b700`, blue `#3b82f6`, orange `#f97316`. Type: Arial Rounded MT Bold / Nunito-like system rounded stacks, with Avenir Next for body.

## Sections

- `hero`: headline on felt with CSS dice and meeples
- `track`: how a visit works, as a board-game track
- `library`: game cards with players, time and difficulty pips
- `prices`: table cover prices as player tokens
- `events`: weekly events and the booking button

## Adapting it

- Player colours mark categories (players, time, difficulty); keep them consistent across cards.
- Dice and meeples are CSS/SVG; keep them decorative (`aria-hidden`).
- Keep one primary call to action and repeat it at the end; secondary links stay quieter.

## Self-check

- [ ] Above the fold at 1440×900: what it is, who it is for, and the main action.
- [ ] At 390 px wide nothing scrolls sideways and tap targets are at least 44 px.
- [ ] Text contrast at least 4.5:1 on every background used.
