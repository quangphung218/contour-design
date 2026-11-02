---
name: motion-example-habit-app-story
en_name: "Example: habit app vertical story"
en_description: "A finished ten-second 9:16 story for a made-up habit app: tasks tick off on a phone, a streak counts to 21 days and a download line closes."
description: |
  A finished 10 s vertical (9:16) story for a made-up habit app, Tidy:
  a phone shows the day's list, four habits tick off one by one, a streak
  counter climbs to 21 days with a flame, and a download line closes it.
  Use it as a reference for app promos made for vertical feeds.
triggers:
  - "app promo"
  - "app story"
  - "vertical app video"
  - "habit app"
  - "9:16 promo"
od:
  mode: video
  surface: video
  platform: desktop
  scenario: motion
  category: motion-example
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  example_prompt: |
    10-second 9:16 story for "Tidy — small habits, every day": phone with
    four habits ticking off, streak counter to 21 days, end line "Free to
    download". Lilac and ink.
---

# Example: habit app vertical story

A Contour motion example: a 10 s, 1080×1920 (9:16)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.5 s | App name and tagline; the phone slides up from below. |
| 1.3–1.9 s | The day label, four habits and the streak card build in. |
| 2.0–4.8 s | Each habit ticks off (0.8 s apart); the streak steps 17 → 21; the flame flickers. |
| 6.0 s | "Free to download" rises under the phone. |
| 9.4 s | Fade out. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 10s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 10.000%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 10.8px on the 1080px grid), so the stage
  scales to the preview and renders at exactly 1080×1920.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `lilac` | #e9e3ff |
| `ink` | #1d1838 |
| `violet` | #6c4ef5 |
| `soft violet` | #c7bdf5 |
| `flame` | #ff8a3d / #ffd166 |

## What to change for a new brief

- Tidy is made up. Replace the name `#logo`, tagline `#tagl`, the four habits `#i1`–`#i4` and the end line `#end`.
- Each tick: the box `#bN` fills violet and its check `#cN` draws, 0.8 s apart from 2.0 s.
- Streak counter `#n` (end value in its `--n` keyframes) and flame `#flame`.

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
