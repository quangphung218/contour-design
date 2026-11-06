---
name: motion-product-launch
en_name: "Product launch"
en_description: "A twelve-second launch film for made-up earbuds: a glow opens, the case rises with the buds lifting out, three spec chips slide in and the name and price land."
zh_name: "产品发布"
zh_description: "12 秒产品发布短片：光晕亮起，耳机盒升起、耳机弹出，三条参数卡滑入，最后落下产品名与价格。"
description: |
  A 12 s, 16:9 launch film for a physical product: "Introducing", the
  product rises out of a soft glow, its parts lift into place, three spec
  chips slide in beside it, then the name, launch date and price land on
  an end card. Swap the inline SVG product, the specs and the end line.
triggers:
  - "product launch video"
  - "launch film"
  - "introducing new product"
  - "product reveal"
  - "new product announcement"
  - "产品发布视频"
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
    12-second launch film for new wireless earbuds on near-black: a mint glow,
    the case rises and the buds lift out, three spec chips (battery, mics,
    water resistance), then the name, launch date and price.
---

# Product launch

A Contour motion template: a 12 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Kestrel One is a made-up product.

## What is in the film

| Time | Beat |
|---|---|
| 0.5–2.4 s | "INTRODUCING" fades up and away while a glow opens. |
| 2.2–4.5 s | The case scales and turns into place; the two buds lift out. |
| 5.0–8.8 s | The product slides left and three spec chips slide in from the right. |
| 8.4–9.2 s | The chips leave; the product shrinks up to the top. |
| 9.0–11.8 s | Name, a drawn rule, launch date and price; everything fades for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 12s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 8.3333%. To move a beat, change its
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
| `background` | #0d0f14 |
| `ink` | #f4f1ea |
| `accent` | #7cf2c9 |
| `chip` | #1a1e27 |
| `muted` | #8b93a3 |

## What to change for a new brief

- Product name `#name`, date and price `#date`.
- The three chips `#s1`–`#s3`: one number in `<b>`, one short line in `<span>`.
- The product drawing inside `#product`: keep `#bud1`/`#bud2` for parts that lift, or drop their tracks.
- Accent `#7cf2c9` for another brand colour.

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
