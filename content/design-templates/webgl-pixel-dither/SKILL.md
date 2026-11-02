---
name: webgl-pixel-dither
en_name: "Dithered pixel sunset"
en_description: "A two-colour ordered-dither sunset scrolls over pixel hills for an independent radio station."
description: |
  A low-resolution scene rendered in a fragment shader and quantised
  with a 4×4 Bayer ordered dither into two colours, for a print-like
  1-bit look. For music, games and retro-tinged brands.
triggers:
  - "dither"
  - "1-bit"
  - "pixel art"
  - "bayer"
  - "retro shader"
  - "lo-fi"
  - "webgl"
  - "shader"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: personal
  category: "webgl"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [animation-discipline]
  example_prompt: "Make a landing hero for a radio station with a two-colour ordered-dither pixel sunset that slowly moves."
---

# Dithered pixel sunset

Dusk FM, a made-up listener-funded radio station, opens on a two-colour pixel sunset: a banded sun sinking behind two scrolling hill lines, dithered like an old thermal print.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

The shader snaps `gl_FragCoord` to a cell of `PIXEL` screen pixels, evaluates a brightness scene at the cell centre (sky ramp, banded sun, two hill silhouettes with parallax), then compares it with a 4×4 Bayer threshold. The Bayer index is built from bit operations written as float math so it runs in GLSL ES 1.0. The poster is the same scene dithered in Python into one SVG path.

## Palette and type

Two inks only: night plum `#2b1b3d` and warm sodium `#ffb35c`. Type: monospace for the player, condensed system sans for the call letters.

## Adapting it

- Change `INK` and `PAPER`; any strong two-colour pair works (blue and cream reads as cyanotype).
- `PIXEL` sets the cell size in screen pixels; 4–8 reads as pixel art.
- Draw a different scene in `scene()`; keep it in brightness only, the dither does the colour.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] Pixel cells stay square on every aspect ratio.
