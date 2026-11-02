---
name: webgl-product-spin
en_name: "Rotating 3D product"
en_description: "A raymarched fabric speaker turns slowly on a studio sweep, with a spec rail beside it."
description: |
  A product hero with a real-time 3D object: a raymarched speaker
  (rounded cylinder, fabric stripes, metal top) turning on a studio
  backdrop with soft shadow. For hardware launches and product pages that
  want a 3D render without a model file.
triggers:
  - "3d product"
  - "product render"
  - "rotating product"
  - "raymarch"
  - "product hero"
  - "3d viewer"
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
  example_prompt: "Create a product launch hero with a 3D rotating product rendered live in WebGL next to its key specs."
---

# Rotating 3D product

Orrin One, a fictional home speaker, launches with a single screen: the product turning on a warm studio sweep, specs on a rail beside it.

## What ships

- `example.html`: one self-contained file. Inline GLSL, system fonts, no network request of any kind.
- A static poster (`.fallback`, `data-webgl-fallback`) drawn in CSS/SVG. The gallery preview runs no script, so the poster is what the card shows. It also stays up, with a short note, when the browser has no WebGL or loses the context.
- A small harness (`GLX`): WebGL2 with a WebGL1 fallback, shader compile logs, DPR clamped to 2, pause when the tab is hidden, one still frame under `prefers-reduced-motion`.

## Technique

Signed distance fields, raymarched in one fragment shader. The body is a rounded cylinder with a groove cut near the top; material is chosen by height (fabric below the groove, brushed metal above). Fabric gets angular stripes so the rotation reads. Lighting: key light with soft shadow, rim light, ambient occlusion from five samples, and a contact shadow on the floor plane. Pointer X adds to the turn angle.

## Palette and type

Studio sand `#efe6da` to `#d8cab8`, fabric sage `#7f9183`, metal warm grey. Type: Avenir Next for the name, monospace for specs.

## Adapting it

- Change the body by editing `body()`; any SDF works (rounded box for a router, capsule for a bottle).
- Tune `FABRIC` and `METAL` colours; keep the backdrop two steps lighter than the product.
- Lower `STEPS` from 96 to 64 if the target hardware is weak.
- Keep the poster in step with the shader: same palette, same composition. Someone without WebGL sees only the poster.
- Keep the file free of `http://` and `https://` URLs: no CDN, no web fonts, no remote images.

## Self-check

- [ ] The poster alone (scripts off) reads as a finished hero.
- [ ] No console errors; the first frame clears `gl.getError()`.
- [ ] Headline contrast holds over the brightest frame.
- [ ] The product silhouette stays inside the right 55% of the frame at 16:9 and 4:3.
