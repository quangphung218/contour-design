---
name: webgl-liquid-metal
en_name: "Liquid chrome blobs"
en_description: "Mercury-like blobs merge and split in a chrome studio for a late-night electronic music series."
description: |
  Raymarched metaballs: five spheres blended with a smooth minimum,
  shaded as polished chrome against a procedural studio environment. A
  loud, tactile hero for music, fashion and event launches.
triggers:
  - "metaballs"
  - "liquid metal"
  - "chrome"
  - "blob"
  - "raymarch"
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
  example_prompt: "Make an event teaser hero with raymarched liquid chrome blobs that merge and split behind bold type."
---

# Liquid chrome blobs

Mercury Sessions, an invented night of live electronic sets, teases its line-up with blobs of liquid chrome that drift, merge and split behind heavy condensed type.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

Five spheres on Lissajous orbits, merged with a polynomial smooth-min (`k = 0.45`). Shading reflects the view ray into a procedural environment: a vertical gradient with two bright softbox stripes and a magenta floor bounce, so the chrome has something to mirror. Fresnel brightens the edges. The pointer pulls one blob.

## Palette and type

Black studio `#08070b`, chrome greys, a magenta bounce `#ff3d8b` and an acid accent `#d7ff3d` for the date. Type: Impact / Haettenschweiler / Arial Narrow bold, the most condensed faces systems ship.

## Adapting it

- Blob count and orbits live in `map()`; keep `k` between 0.3 and 0.6 or they stop merging.
- Recolour the softbox stripes in `env()` to tint the chrome (gold: warm stripes, warm floor).
- Type can sit over the blobs; keep it white with a 1px dark outline for contrast.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] Blobs stay inside the frame on 21:9 and 4:3 screens.
