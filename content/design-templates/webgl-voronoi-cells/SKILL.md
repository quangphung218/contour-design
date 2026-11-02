---
name: webgl-voronoi-cells
en_name: "Living cell field"
en_description: "Soft Voronoi cells drift and divide like tissue under a microscope, behind a biotech hero."
description: |
  An animated Worley/Voronoi field: moving seed points form soft
  cells with glowing nuclei and thin membranes, like tissue under a
  fluorescence microscope. For biotech, health and science brands.
triggers:
  - "voronoi"
  - "cells"
  - "worley noise"
  - "biotech background"
  - "organic pattern"
  - "microscope"
  - "webgl"
  - "shader"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: healthcare
  category: "webgl"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [animation-discipline]
  example_prompt: "Create a biotech hero with an animated Voronoi cell field that looks like fluorescent tissue."
---

# Living cell field

Cellwise, an invented live-cell imaging startup, shows tissue drifting under a fluorescence microscope behind a calm, clinical headline.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

Classic Worley noise in a 3×3 neighbourhood: each grid cell owns a seed that orbits with time. The shader keeps the nearest and second-nearest distances; `F2 − F1` draws the membranes, `F1` draws the nuclei. Two hues (cyan membranes, magenta nuclei) imitate two fluorescence channels. A vignette mimics the microscope's round field.

## Palette and type

Dark field `#050b10`, membrane cyan `#3fe0d0`, nucleus magenta `#e05bb8`. Type in Avenir Next with a monospace caption, like a microscope readout.

## Adapting it

- `SCALE` sets cell size; fewer, larger cells read better behind text.
- Swap the two channel colours for a brand pair; keep them on opposite sides of the hue wheel.
- Drop the round vignette (`vig`) for a full-bleed pattern.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] Membranes stay thin at 4K (they scale with the cell, not the pixel).
