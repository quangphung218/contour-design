---
name: motion-testimonial
en_name: "Customer testimonial (1:1)"
en_description: "A nine-second square review card: five stars pop in, a customer quote rises line by line, the key phrase gets a hand-drawn underline and the customer's avatar and name land."
zh_name: "客户评价（方形）"
zh_description: "9 秒方形客户评价卡：五颗星依次弹出，评价逐行升起，关键句画上手绘下划线，最后出现头像与姓名。"
description: |
  A 9 s, 1:1 testimonial for feeds and ads: a card rises, five stars pop in
  one by one, a rating line, a three-line customer quote that rises line by
  line with its key phrase underlined by hand, then an initials avatar,
  name and role. Swap the quote, person and rating.
triggers:
  - "testimonial video"
  - "customer review animation"
  - "social proof video"
  - "review card"
  - "customer quote video"
  - "客户评价视频"
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
    9-second square testimonial on warm cream: five gold stars pop in, a
    three-line quote from a bakery owner rises, "a full day a week" gets an
    orange hand-drawn underline, then avatar, name and role.
---

# Customer testimonial (1:1)

A Contour motion template: a 9 s, 1080×1080 (1:1)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Crumbline, Brightwell Bakery and Ana Moreau are made up.

## What is in the film

| Time | Beat |
|---|---|
| 0.1–0.8 s | The card rises in with a large quote mark. |
| 0.74–1.5 s | Five stars pop in, one every 0.14 s; the rating slides in. |
| 1.9–3.2 s | The quote rises line by line. |
| 3.4–4.1 s | The key phrase is underlined. |
| 4.0–8.8 s | Avatar and name land, hold, then the card fades for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 9s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 11.1111%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 10.8px on the 1080px grid), so the stage
  scales to the preview and renders at exactly 1080×1080.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `background` | #fff7ef |
| `card` | #ffffff |
| `ink` | #2a1b12 |
| `accent` | #e8743b |
| `stars` | #f5b301 |
| `muted` | #8a7465 |

## What to change for a new brief

- Quote lines `#q1`–`#q3`; the underlined phrase sits in `.hl`.
- Rating `#rating`; drop a star by removing its `<svg>` and its track.
- Avatar initials `#avatar`, name and role in `#person`.
- Accent `#e8743b`.

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
