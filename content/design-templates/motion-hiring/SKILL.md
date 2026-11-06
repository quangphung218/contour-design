---
name: motion-hiring
en_name: "We're hiring"
en_description: "A ten-second recruiting post: \"We're hiring.\" rises out of a mask, three role cards slide in and nudge one by one, and an apply-by line closes it."
zh_name: "招聘公告"
zh_description: "10 秒招聘短片：“We're hiring.” 从遮罩中升起，三张职位卡依次滑入并逐一轻推，最后给出截止日期。"
description: |
  A 10 s, 16:9 recruiting film for job boards, careers pages and team
  socials: a big "We're hiring." rises out of a mask with a one-line pitch,
  three role cards with location tags slide in and each gets a nudge in
  turn, then an apply-by line. Change the roles, tags and deadline.
triggers:
  - "we're hiring video"
  - "job post video"
  - "recruiting animation"
  - "open roles"
  - "careers announcement"
  - "招聘视频"
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
    10-second "We're hiring." film on cobalt blue: the headline rises line by
    line, three white role cards (designer, Rust engineer, customer success)
    with location tags slide in and nudge in turn, then "Apply by 30 November".
---

# We're hiring

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Cobalt & Fern is a made-up studio.

## What is in the film

| Time | Beat |
|---|---|
| 0.3–1.2 s | "We're" and "hiring." rise out of their masks. |
| 1.3–2.2 s | The pitch line and studio name fade up. |
| 2.4–3.7 s | Three role cards slide in from the right. |
| 4.6–6.6 s | Each card nudges in turn. |
| 7.0–9.6 s | The apply-by line lands; everything clears for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 10s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 10%. To move a beat, change its
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
| `background` | #1446ff |
| `ink` | #ffffff |
| `fern` | #b8f5a0 |
| `card ink` | #0d1440 |
| `muted` | #c9d4ff |

## What to change for a new brief

- Role cards `#c1`–`#c3`: title in `<strong>`, tags in `.tags`. Two or four cards fit; shift the nudge times.
- Pitch `#lead`, studio `#studio`, deadline `#cta`.
- Background `#1446ff` and accent `#b8f5a0`.

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
