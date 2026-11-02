---
name: webgl-star-warp
en_name: "Starfield warp tunnel"
en_description: "Stars stream past in a hyperspace tunnel behind a launch countdown for a small-satellite startup."
description: |
  A starfield fly-through in one fragment shader: layered star cells
  in polar space, stretched into streaks by speed, with a soft nebula
  glow. For launches, countdowns and anything about speed or space.
triggers:
  - "starfield"
  - "warp"
  - "hyperspace"
  - "space background"
  - "tunnel"
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
  example_prompt: "Build a launch countdown page over a full-screen starfield warp tunnel."
---

# Starfield warp tunnel

Perigee Labs, a made-up small-satellite company, counts down to its first launch. Stars stream out from the centre; the countdown sits in the middle of the tunnel.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

Polar coordinates around the centre. Four depth layers of hashed star cells; each layer moves outward at its own speed and wraps with `fract`, so the loop never ends. Streak length grows with radius and speed. A faint two-colour nebula (fbm) sits behind. The pointer shifts the vanishing point a little.

## Palette and type

Space `#03040a`, star white with an ice-blue tint `#bcd7ff`, nebula violet `#5b3cc4` and teal `#1fa3a3`. Countdown digits in the system monospace with tabular figures.

## Adapting it

- `SPEED` controls the rush; 0.25 reads calm, 0.8 reads urgent.
- Set the countdown target in `TARGET` (ISO date). The digits update once a second; the shader does not depend on them.
- Keep the centre of the tunnel clear of anything but the countdown.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] Countdown digits do not jitter in width (tabular figures).
