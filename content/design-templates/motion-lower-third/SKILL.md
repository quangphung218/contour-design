---
name: motion-lower-third
en_name: "Lower third"
en_description: "A six-second name strap: an accent block wipes in, the name and role slide out of it, hold, then it wipes away."
description: |
  A 6 s lower third (name strap) for interviews, talks and explainers:
  an accent block wipes in, the name and role slide out from behind it, a
  small live dot pulses, then the strap wipes away. The background is a
  soft stand-in for footage; replace it with your clip or keep it plain.
triggers:
  - "lower third"
  - "name strap"
  - "name title"
  - "speaker title"
  - "chyron"
od:
  mode: video
  surface: video
  platform: desktop
  scenario: motion
  category: motion-template
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  example_prompt: |
    6-second lower third over soft footage-like background: name "Mai
    Tran", role "Head of Product, Fieldnote", lime accent block, wipes in,
    holds, wipes out.
---

# Lower third

A Contour motion template: a 6 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0–6 s | The footage stand-in drifts slowly; the live dot blinks. |
| 0.6–2.0 s | The accent block rises, the panel opens from it, name then role slide in. |
| 2.0–4.6 s | Hold for reading. |
| 4.6–5.6 s | Text fades, the panel closes into the block, the block drops away. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 6s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 16.667%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 19.2px on the 1920px grid), so the stage
  scales to the preview and renders at exactly 1920×1080.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `background stand-in` | #23303a |
| `panel` | rgba(12,16,20,.82) |
| `accent` | #c8f169 |
| `live dot` | #ff4d4d |

## What to change for a new brief

- Name `#name` and role `#role`; the panel grows with the text.
- Accent: `#c8f169` (block and role). Tag in the corner: `#live`.
- Over real footage: delete `.bg` and `.vig`, add a muted `<video>` the HyperFrames way, or render on a plain colour and key it in your editor.

## Workflow

1. Scaffold a source folder: `"$OD_NODE_BIN" "$OD_BIN" media scaffold --project "$OD_PROJECT_ID" --composition-dir "motion-source/film"`.
2. Replace `motion-source/film/index.html` with this template's
   `example.html`, then change only what the brief asks: copy, colours,
   timing. Keep the composition root, `data-poster-time` inside the film
   and the master timeline.
3. Render: `"$OD_NODE_BIN" "$OD_BIN" media generate --project "$OD_PROJECT_ID" --surface video --model hyperframes-html --composition-dir "motion-source/film" --output "film.mp4"`.
   If it prints a task id, run `"$OD_NODE_BIN" "$OD_BIN" media wait <id>` until it exits 0.
4. Open the review frames it prints, then link the MP4 and the source HTML
   in the reply.
