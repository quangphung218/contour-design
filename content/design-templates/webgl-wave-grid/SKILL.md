---
name: webgl-wave-grid
en_name: "Ocean point grid"
en_description: "A perspective grid of points rolls like a swell under live buoy readings for a coastal sensor network."
description: |
  A 3D field of points on a plane, displaced by a sum of travelling
  waves and seen in perspective, drawn with `gl.POINTS` and a real
  projection matrix. For data, climate and ocean stories that want depth
  without a 3D library.
triggers:
  - "wave grid"
  - "point grid"
  - "ocean waves"
  - "3d grid"
  - "terrain points"
  - "data landscape"
  - "webgl"
  - "shader"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: research
  category: "webgl"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [animation-discipline]
  example_prompt: "Make a hero with a 3D grid of points rolling like ocean swell under a few live sensor readings."
---

# Ocean point grid

Tideline, an invented coastal sensor network, shows the sea as a grid of light points rolling toward the viewer, with three buoy readings pinned on top.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

A 220×140 grid uploaded once. The vertex shader adds three directional waves (different wavelength, speed, direction) to `y`, then applies a perspective × look-at matrix built on the CPU (`GLX.perspective`, `GLX.lookAt`). Point size and brightness fall off with depth; crest height tints points from deep blue to foam white.

## Palette and type

Night sea `#04121c`, deep points `#1b6f9a`, crests `#d8f3ff`, reading accent `#ffcf5a`. Readings in the system monospace; headline in Avenir Next.

## Adapting it

- Feed real wave height into the amplitudes (`A1`–`A3`) to make the grid a live chart.
- Change `COLS`/`ROWS` together; keep the total under 40k points.
- Move the camera in `lookAt` for a lower, more dramatic horizon.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] The horizon fades smoothly; no hard edge at the far row.
