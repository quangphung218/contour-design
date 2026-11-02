---
name: webgl-ink-gradient
en_name: "Noise gradient hero"
en_description: "A slow, grainy gradient of peach, plum and teal that drifts behind a paint studio's launch headline."
description: |
  A full-screen hero whose background is a domain-warped fbm noise
  gradient in four colours, with film grain and a gentle pull toward the
  pointer. Built for brand launches that want colour as the hero.
triggers:
  - "noise gradient"
  - "gradient hero"
  - "mesh gradient"
  - "grainy gradient"
  - "fbm"
  - "webgl"
  - "shader"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: marketing
  category: "webgl"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [animation-discipline]
  example_prompt: "Build a full-screen hero with a drifting grainy noise gradient behind a short launch headline."
---

# Noise gradient hero

A paint studio, Mireille Colour Lab, launches a seasonal palette. The page is one screen: a living gradient in the four launch colours and a quiet editorial headline.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

One fragment shader on a full-screen triangle. Two passes of domain warping (`q`, then `r`) feed an fbm that picks between four colours with `smoothstep` bands. Per-pixel hash noise adds grain. The pointer nudges the warp by a quarter of a unit.

## Palette and type

Cream `#fcf2e0`, peach `#f9cbad`, plum `#8c4273`, teal `#21787f`. Headline in the system serif stack (Iowan Old Style, Palatino, Georgia), labels in the system sans.

## Adapting it

- Swap the four `vec3` colours (`c1`–`c4`) and the matching CSS tokens together.
- Slow it further by lowering the `0.06` time factor; never go above `0.12`, the grain starts to swim.
- Keep the headline on the cream side of the frame (left third).
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] The gradient never flashes or pulses; motion reads as drift.
