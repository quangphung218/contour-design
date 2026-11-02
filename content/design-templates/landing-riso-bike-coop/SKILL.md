---
name: landing-riso-bike-coop
en_name: "Risograph bike co-op landing"
en_description: "A community bike-repair workshop's page printed in two overlapping riso inks, with opening hours and a fix-it menu."
description: |
  A community landing page in a two-colour risograph print style:
  fluorescent pink and blue inks that overprint, slight misregistration,
  halftone dots and chunky grotesk type. For community spaces, zines,
  workshops and grassroots projects.
triggers:
  - "risograph"
  - "riso"
  - "community landing"
  - "zine style"
  - "bike shop"
  - "workshop page"
  - "landing page"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: personal
  category: "community"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [typography, color]
  example_prompt: "Build a landing page for a community bike repair workshop in a two-colour risograph print style with hours, services and how to volunteer."
---

# Risograph bike co-op landing

Spoke & Pedal (invented), a volunteer bike workshop, explains itself on one page: fix your own bike with our tools, the price list, open hours, and how to volunteer.

## What ships

- `example.html`: one responsive page, no script needed. It reads at 1440 px and folds to a single column under 760 px.
- No network request: system font stacks, CSS shapes and inline SVG only. No web fonts, no CDN, no remote images.

## Visual system

Paper `#f4f0e6`, riso fluorescent pink `#ff48b0` and medium blue `#0078bf`, overprinting with `mix-blend-mode: multiply` to make a deep violet where they cross. Type: Arial Black / Helvetica Neue heavy for headlines, Courier-like monospace for details, like a photocopied flyer.

## Sections

- `hero`: big overprinted headline, a bike drawn in two inks, halftone dots
- `how`: three steps as stamped boxes
- `menu`: fix-it price list in a dotted-leader table
- `hours`: opening hours as a riso calendar strip
- `volunteer`: call to action on a pink block

## Adapting it

- Only two inks plus paper. Darker tones come from overprinting, not from a third colour.
- Misregister one layer by 3–4 px for the print feel; never more.
- Keep one primary call to action and repeat it at the end; secondary links stay quieter.

## Self-check

- [ ] Above the fold at 1440×900: what it is, who it is for, and the main action.
- [ ] At 390 px wide nothing scrolls sideways and tap targets are at least 44 px.
- [ ] Text contrast at least 4.5:1 on every background used.
