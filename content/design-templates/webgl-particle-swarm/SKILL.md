---
name: webgl-particle-swarm
en_name: "Particle murmuration"
en_description: "Sixty thousand glowing points flock like starlings at dusk behind a bird-count campaign."
description: |
  A full-screen particle field: tens of thousands of points moved
  entirely in the vertex shader along looping flock paths, drawn with
  additive blending. Suits campaigns and events that want a living,
  organic hero.
triggers:
  - "particles"
  - "particle system"
  - "flocking"
  - "murmuration"
  - "point cloud"
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
  example_prompt: "Make a full-screen hero where thousands of particles flock like starlings behind a campaign headline."
---

# Particle murmuration

A bird-conservation charity calls volunteers to its winter count. Sixty thousand points swirl like a starling murmuration over a dusk sky.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

One `gl.POINTS` draw. Each point carries a random `aSeed` (three floats); the vertex shader turns seed and time into a looping Lissajous path, so the CPU uploads the buffer once and never touches it again. Additive blending (`ONE, ONE`) builds the dense core. The pointer drags the nearer points.

## Palette and type

Dusk navy `#0d1530` into ember `#e9774a`; points in warm cream `#ffd9b0`. Headline in Futura / Avenir Next condensed weight, body in the system sans.

## Adapting it

- Change `COUNT` for density (20k on low-end laptops still reads as a flock).
- Change the path in the vertex shader, not on the CPU.
- Keep the headline low and left: the flock owns the upper right.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] The flock reads at 20k points as well as at 60k.
