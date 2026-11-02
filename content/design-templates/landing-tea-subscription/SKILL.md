---
name: landing-tea-subscription
en_name: "Slow tea subscription landing"
en_description: "A small-batch tea subscription with steam drawn in CSS, tasting notes per harvest and a calm monthly plan."
description: |
  A calm product landing for a subscription: muted botanical greens,
  generous white space, a slow CSS steam animation over an SVG cup,
  harvest cards with tasting notes and one simple plan. For food and
  drink subscriptions, wellness and craft brands.
triggers:
  - "subscription landing"
  - "tea"
  - "subscription box"
  - "calm landing"
  - "craft brand"
  - "ecommerce landing"
  - "landing page"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: marketing
  category: "subscription"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [typography, color]
  example_prompt: "Build a landing page for a small-batch tea subscription with a calm botanical look, harvest cards, plan and FAQ."
---

# Slow tea subscription landing

Second Flush (invented) sends three small-harvest teas a month from named growers. The page sells the habit, not the box: the cup, this month's harvests, the plan, and a short FAQ.

## What ships

- `example.html`: one responsive page, no script needed. It reads at 1440 px and folds to a single column under 760 px.
- No network request: system font stacks, CSS shapes and inline SVG only. No web fonts, no CDN, no remote images.

## Visual system

Rice paper `#f6f4ee`, tea green `#5f7357`, deep leaf `#2f3d2b`, steeped amber `#b77a2f` for prices and the single button. Headlines in Hoefler Text / Iowan Old Style / Georgia; body in Avenir Next / Segoe UI.

## Sections

- `hero`: SVG cup with CSS steam, headline, plan price
- `harvests`: three cards: grower, region, tasting notes
- `ritual`: brewing guide as three timed steps
- `plan`: one plan card, cancel any time
- `faq`: four questions in `<details>`

## Adapting it

- The steam respects `prefers-reduced-motion` and stops.
- One amber element per screen: the price or the button, never both large.
- Keep one primary call to action and repeat it at the end; secondary links stay quieter.

## Self-check

- [ ] Above the fold at 1440×900: what it is, who it is for, and the main action.
- [ ] At 390 px wide nothing scrolls sideways and tap targets are at least 44 px.
- [ ] Text contrast at least 4.5:1 on every background used.
