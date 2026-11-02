---
name: landing-farm-box
en_name: "Kraft-paper farm box landing"
en_description: "A weekly veg-box scheme's page on kraft paper with rubber-stamp labels, this week's box and delivery days."
description: |
  A local-food landing on kraft paper: rubber-stamp labels, hand-
  tied tags, a box contents list that reads like a packing slip, and an
  earthy palette. For farm shops, veg boxes, markets and local producers.
triggers:
  - "veg box"
  - "farm shop"
  - "local food"
  - "kraft paper"
  - "market landing"
  - "csa"
  - "landing page"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: sales
  category: "local-food"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [typography, color]
  example_prompt: "Build a landing page for a weekly vegetable box scheme on kraft paper with stamps, this week's box, sizes and delivery days."
---

# Kraft-paper farm box landing

Two Fields Farm (invented) sells a weekly veg box. The page shows this week's box as a packing slip, the three sizes, which villages get delivery on which day, and how to start.

## What ships

- `example.html`: one responsive page, no script needed. It reads at 1440 px and folds to a single column under 760 px.
- No network request: system font stacks, CSS shapes and inline SVG only. No web fonts, no CDN, no remote images.

## Visual system

Kraft `#c9a77c` with a fibre texture from layered gradients, paper `#f3ead9` for slips, stamp red `#b23a2a`, field green `#3f6b33`, ink `#2b2118`. Type: American Typewriter / Courier for slips and stamps, Rockwell / Georgia bold for headlines.

## Sections

- `hero`: headline on kraft with a rubber stamp and a tag
- `box`: this week's box as a packing slip with weights
- `sizes`: three sizes as hanging tags with prices
- `delivery`: delivery days by village in a stamped table
- `start`: three steps and the sign-up button

## Adapting it

- Stamps rotate a few degrees and use the stamp red only.
- Update the packing slip weekly; it is the page's reason to come back.
- Keep one primary call to action and repeat it at the end; secondary links stay quieter.

## Self-check

- [ ] Above the fold at 1440×900: what it is, who it is for, and the main action.
- [ ] At 390 px wide nothing scrolls sideways and tap targets are at least 44 px.
- [ ] Text contrast at least 4.5:1 on every background used.
