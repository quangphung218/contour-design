---
name: webgl-data-globe
en_name: "Data globe with routes"
en_description: "A dotted globe turns while freight routes arc between hubs, beside a live shipments counter."
description: |
  A data globe: a Fibonacci-lattice dot sphere with a land mask,
  animated arcs between hubs, and back-face dimming, all in one WebGL
  program. For logistics, network and global-reach stories.
triggers:
  - "data globe"
  - "3d globe"
  - "globe"
  - "routes map"
  - "network map"
  - "world map 3d"
  - "webgl"
  - "shader"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: operations
  category: "webgl"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [animation-discipline]
  example_prompt: "Build a hero with a rotating dotted globe and animated arcs between hubs, next to a few live operations numbers."
---

# Data globe with routes

Kestrel Freight, a made-up carrier, shows its network: a slowly turning dotted planet with routes that light up hub to hub, and three numbers on the left.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

One program, two draw modes. Land dots: a Fibonacci lattice of 9,000 points filtered by a smooth mask (sum of sines, so the continents are invented, not geographic). Routes: great-circle paths between hub pairs, lifted by `sin(πt)`, drawn as `LINE_STRIP`s whose alpha runs along `aT` to show direction. The vertex shader rotates the sphere and dims anything facing away.

## Palette and type

Deep ink `#07111f`, dots in sea glass `#6fd3c1`, routes in signal amber `#ffb547`. Numbers in the system monospace; headline in Avenir Next.

## Adapting it

- Replace `HUBS` (unit-sphere positions) and `ROUTES` (index pairs) with real data; convert lat/long with `[cos(lat)cos(lon), sin(lat), cos(lat)sin(lon)]`.
- Real coastlines: swap the `land()` mask for a lookup into an equirectangular mask embedded as a `data:` PNG.
- Keep the globe on the right; the numbers column needs 30% of the width.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] Back-side dots are dimmer than front-side dots, so the sphere reads as solid.
