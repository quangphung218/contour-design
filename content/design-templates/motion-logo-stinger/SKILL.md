---
name: motion-logo-stinger
en_name: "Logo stinger"
en_description: "A five-second logo sting: four coloured tiles fly in from the corners and snap into a mark, it turns and slides aside, and the wordmark wipes on with its tagline."
zh_name: "Logo 快闪"
zh_description: "5 秒 Logo 快闪：四块彩色方块从四角飞入拼成标志，标志转动并侧移，字标擦出，标语浮现。"
description: |
  A 5 s, 16:9 logo sting for video intros, outros and social bumpers: four
  coloured tiles fly in from the corners and snap together with a soft
  flash, the mark turns a quarter and slides aside, the wordmark wipes on
  with a tagline before it fades.
triggers:
  - "logo sting"
  - "logo stinger"
  - "logo animation intro"
  - "brand bumper"
  - "short logo intro"
  - "Logo 动画"
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
    5-second logo sting on near-black: four tiles (red, yellow, blue, violet)
    fly in from the corners and snap into a mark, it turns and slides left,
    the wordmark "quarrytone" wipes on with a tagline.
---

# Logo stinger

A Contour motion template: a 5 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Quarrytone is a made-up brand.

## What is in the film

| Time | Beat |
|---|---|
| 0.15–1.0 s | Four tiles fly in from the corners, turning as they land. |
| 1.0–1.6 s | They snap together with a soft flash. |
| 1.3–2.6 s | The mark turns a quarter and slides left. |
| 2.3–3.8 s | The wordmark wipes on, the tagline rises. |
| 4.3–4.8 s | The lockup fades for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 5s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 20%. To move a beat, change its
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
| `background` | #101014 |
| `ink` | #f7f4ee |
| `tile 1` | #ff5a36 |
| `tile 2` | #ffc93c |
| `tile 3` | #36c5f0 |
| `tile 4` | #7b61ff |
| `muted` | #8d8a94 |

## What to change for a new brief

- Wordmark text in `#wi` and tagline `#tag`; a longer name needs a smaller `font-size` on `.word`.
- The four tiles `.q1`–`.q4`: colour and corner radius make the mark.
- Keep the wordmark a solid colour. Gradient text (`background-clip: text`)
  shows in the browser but renders transparent in the MP4. The wipe moves
  `#word` and `#wi` in opposite directions; keep their keyframes in step.
- How far the mark slides: `translateX(-17.7083cqw)` in the `#mark` keyframes.

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
