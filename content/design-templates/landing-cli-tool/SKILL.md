---
name: landing-cli-tool
en_name: "Terminal-first CLI tool landing"
en_description: "A developer CLI's page that leads with a working terminal transcript, install commands and benchmark bars."
description: |
  A developer-tool landing that leads with the terminal: a styled
  session transcript as the hero, copy-ready install commands per
  platform, benchmark bars and a feature grid in monospace. For CLIs,
  SDKs and developer infrastructure.
triggers:
  - "cli landing"
  - "developer tool"
  - "terminal"
  - "devtools landing"
  - "open source tool"
  - "install page"
  - "landing page"
od:
  mode: prototype
  origin: contour
  platform: desktop
  scenario: engineering
  category: "developer-tool"
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  craft:
    requires: [typography, color]
  example_prompt: "Build a landing page for a command-line tool that leads with a terminal transcript, install commands, benchmarks and features."
---

# Terminal-first CLI tool landing

Sift (invented) is a fast log-search CLI. The page shows a real-looking session first, then install, a benchmark against two generic baselines, features and docs links.

## What ships

- `example.html`: one responsive page, no script needed. It reads at 1440 px and folds to a single column under 760 px.
- No network request: system font stacks, CSS shapes and inline SVG only. No web fonts, no CDN, no remote images.

## Visual system

Terminal night `#0c0f14`, panel `#141922`, text `#d7dde8`, prompt green `#7ee787`, match highlight `#ffd866`, links `#79c0ff`. Everything in the system monospace (SF Mono, Menlo, Consolas) except the headline, set in Helvetica Neue bold.

## Sections

- `hero`: headline beside a terminal window with a session transcript
- `install`: tabs-free install list: one command per platform
- `bench`: benchmark bars with the method stated
- `features`: six features as a monospace grid
- `cta`: docs and source links

## Adapting it

- Transcripts must be real output of the tool once it exists; until then, label them as illustrative.
- State the benchmark method (machine, data size, runs) next to the bars.
- Keep one primary call to action and repeat it at the end; secondary links stay quieter.

## Self-check

- [ ] Above the fold at 1440×900: what it is, who it is for, and the main action.
- [ ] At 390 px wide nothing scrolls sideways and tap targets are at least 44 px.
- [ ] Text contrast at least 4.5:1 on every background used.
