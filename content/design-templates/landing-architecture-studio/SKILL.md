---
name: landing-architecture-studio
en_name: "Swiss-grid architecture studio"
en_description: "An architecture practice's homepage on a strict Swiss grid: numbered projects, line drawings and a single red rule."
description: |
  A portfolio landing on a strict 12-column Swiss grid: black and
  white, one red accent, numbered project rows, plan and section line
  drawings in SVG, and asymmetric type. For architects, studios,
  engineers and portfolio sites that want restraint.
triggers:
  - "architecture portfolio"
  - "studio website"
  - "swiss grid"
  - "portfolio landing"
  - "minimal portfolio"
  - "landing page"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: design
  category: "portfolio"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [typography, color]
  example_prompt: "Build a portfolio homepage for an architecture studio on a strict Swiss grid with numbered projects and line drawings."
---

# Swiss-grid architecture studio

Vos Okafor Architects (invented) present six projects on a strict grid: a statement, numbered project rows with SVG line drawings, the practice's principles, and contact.

## What ships

- `example.html`: one responsive page, no script needed. It reads at 1440 px and folds to a single column under 760 px.
- No network request: system font stacks, CSS shapes and inline SVG only. No web fonts, no CDN, no remote images.

## Visual system

White `#ffffff`, black `#0a0a0a`, grey `#8a8a8a` for metadata, one signal red `#e10600` for the rule and active states. Type: Helvetica Neue / Arial only, two weights (400, 700), sizes on a 1.25 scale.

## Sections

- `hero`: statement set over 8 columns, red rule, index
- `projects`: numbered rows: number, name, place, year, type, drawing
- `principles`: three short principles in columns
- `practice`: facts: people, years, awards
- `contact`: address grid and one email

## Adapting it

- Everything snaps to the 12-column grid; check with the `.grid` overlay class.
- Line drawings use one stroke width; red appears once per screen.
- Keep one primary call to action and repeat it at the end; secondary links stay quieter.

## Self-check

- [ ] Above the fold at 1440×900: what it is, who it is for, and the main action.
- [ ] At 390 px wide nothing scrolls sideways and tap targets are at least 44 px.
- [ ] Text contrast at least 4.5:1 on every background used.
