---
name: motion-event-tickets
en_name: "Event ticket promo"
en_description: "A ten-second ticket drop for a made-up summer festival: a ticket swings in, dates and venue settle, the early-bird price counts up, a stamp slams and the stub tears off."
zh_name: "活动票务宣传"
zh_description: "10 秒活动售票短片：门票甩入画面，日期与地点依次出现，早鸟价数字上涨，印章盖下，票根撕开。"
description: |
  A 10 s, 16:9 ticket-sale film for festivals, concerts and conferences: a
  ticket swings in over slow rays, the event name, dates and venue settle,
  the early-bird price counts up on the stub, an "On sale now" stamp
  slams, the stub tears off and a call to action lands.
triggers:
  - "ticket sale video"
  - "event promo"
  - "festival tickets"
  - "concert announcement"
  - "tickets on sale"
  - "活动售票视频"
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
    10-second ticket promo for a summer music festival on warm paper: a ticket
    swings in, dates and venue appear, the early-bird price counts to 89 euros,
    an "On sale now" stamp slams and the stub tears off.
---

# Event ticket promo

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Lanternfield and Hollin Bay are made up.

## What is in the film

| Time | Beat |
|---|---|
| 0–1.6 s | Rays turn in; the ticket swings in from the left and settles at −2°. |
| 1.5–3.2 s | Name, tagline, dates and venue, line-up line. |
| 3.0–4.3 s | The early-bird price counts up on the stub. |
| 4.6–5.3 s | "On sale now" stamp slams onto the ticket. |
| 6.6–9.9 s | The stub tears away, the call to action lands, then all fades. |

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
| `paper` | #f6efe3 |
| `ticket` | #fffaf2 |
| `ink` | #1e1b2e |
| `stub` | #ff4d2e |
| `stamp` | #2f6bff |
| `muted` | #6d6577 |

## What to change for a new brief

- Event name `#t1`, tagline `#t2`, the two rows `#t3` and the line-up line `#t4`.
- Price: the `--n: 89` end value in the `#price` keyframes, the currency in `<small>`.
- Stamp words `#stamp`, call to action `#cta`.
- Stub `#ff4d2e` and stamp `#2f6bff` colours.

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
