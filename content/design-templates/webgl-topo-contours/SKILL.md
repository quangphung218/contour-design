---
name: webgl-topo-contours
en_name: "Live topographic contours"
en_description: "Ink contour lines shift over a paper map as the terrain slowly breathes, for a hiking app launch."
description: |
  A topographic contour field drawn in a fragment shader: a moving
  fbm height map turned into evenly weighted iso-lines, with every fifth
  line heavier, on a paper ground. For outdoor, mapping and geo brands.
triggers:
  - "topographic"
  - "contour lines"
  - "topo map"
  - "terrain lines"
  - "isolines"
  - "webgl"
  - "shader"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: product
  category: "webgl"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [animation-discipline]
  example_prompt: "Make a hero with animated topographic contour lines on a paper background for a hiking app."
---

# Live topographic contours

Ridgeline, a fictional hiking app, opens on a paper map whose contour lines drift as if the hills were breathing. The pointer raises a small summit under the cursor.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

One fragment shader. Height `h` is a slow fbm plus a Gaussian bump at the pointer. Lines come from `abs(fract(h·N) − 0.5)` divided by the screen-space gradient, which is estimated with two extra height samples one pixel away (so it needs no `OES_standard_derivatives`). Every fifth level is an index line, drawn heavier in rust.

## Palette and type

Paper `#f1ebdf`, ink `#33443a`, index lines rust `#c0582c`. Headline in a system serif (Charter, Iowan Old Style, Georgia), labels in monospace like map annotations.

## Adapting it

- `LEVELS` sets line density (24 is legible on a laptop; 40 for a 4K wall).
- Freeze the terrain (set the time factor to 0) when the page carries a lot of copy.
- Map labels in the overlay are plain HTML; position them in `%` so they track the viewport.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] Line weight looks even on steep and flat ground (the gradient divide is working).
