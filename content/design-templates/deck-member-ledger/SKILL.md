---
name: deck-member-ledger
en_name: "Ledger-green members' meeting"
en_description: "A credit union's annual members' meeting in banker's green and ledger lines: savings, loans, dividend and the vote."
description: |
  A finance deck in the look of an old bound ledger: banker's
  green, cream ruled columns, figures right-aligned in tabular numerals,
  and a brass accent. For credit unions, cooperatives and member-owned
  finance reporting to members.
triggers:
  - "annual general meeting"
  - "agm deck"
  - "credit union"
  - "members meeting"
  - "ledger style"
  - "dividend"
  - "html deck"
  - "slides"
od:
  mode: deck
  origin: contour
  platform: desktop
  scenario: finance
  category: "member-report"
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  speaker_notes: false
  animations: false
  example_prompt: "Make an annual members' meeting deck for a credit union in a ledger style: balance sheet, loans, dividend and resolutions."
---

# Ledger-green members' meeting

Millbrook Community Credit Union (invented) reports to its members at the AGM: the year in one line, savings and loans, arrears, the proposed dividend and the three resolutions to vote on.

## What ships

- `example.html`: 6 slides on a fixed 1920×1080 stage. Sizes are written in stage units (`calc(N * var(--u))`), so the deck scales to any frame with no script; the gallery preview runs none and shows the slides stacked.
- A local runtime that follows the deck contract: arrows, Page Up/Down, Space, Home/End, wheel, swipe, clickable dots with `aria-current`, `.slide.active` (plus `.is-active`), the position kept in `localStorage`, and `od:slide-state` messages for the host counter.
- Print gives one slide per 1920×1080 page.
- No network request: system font stacks and inline SVG only.

## Visual system

Banker's green `#1f3d2f`, ledger cream `#f4efdf`, red-ink `#a5362a` for arrears only, and brass `#b8913a` for the dividend. Headlines in Georgia / Iowan Old Style; all figures in the system monospace with tabular numerals, right-aligned.

## Slide vocabulary

- `cover`: green cover with a gold rule, like the book's spine
- `summary`: three ledger lines: the year in figures
- `ledger`: balance sheet as ruled columns
- `loans`: loan book by purpose as stacked bar
- `dividend`: the proposed dividend as one large figure
- `resolutions`: numbered resolutions with for/against boxes

## Writing it

- Every figure right-aligned, tabular numerals, same decimals within a column.
- Red ink only for arrears and losses; brass only for the dividend.
- Every size in the per-deck `<style>` and in inline styles is written as `calc(N*var(--u))`, where N is the size in px on the 1920×1080 stage. Write new sizes the same way; a plain `px` value stops scaling with the frame.
- One idea per slide. Headlines of eight words or fewer.

## Self-check

- [ ] Every slide fits 1920×1080 with nothing clipped at the bottom.
- [ ] Charts label every value and their bar lengths match the numbers.
- [ ] Scripts off: the first slide still reads as a finished cover.
