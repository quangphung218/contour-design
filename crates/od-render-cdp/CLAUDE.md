# od-render-cdp

Renders pages with the Chrome / Edge / Chromium already installed, over CDP. Implements
`od_export::Renderer`. Map: `../../docs/architecture.md`. Paths below are relative to this folder.

## Owns
- Finding Chrome (`src/lib.rs:find_chrome`, env `OD_CHROME_PATH`) and starting it.
- The throwaway profile: mark, abandon, sweep (`src/lib.rs:Profile`).
- Page settle, animations, captures (`src/script.rs`), ffmpeg helpers.

## Entry points
- `src/lib.rs:CdpRenderer` (`impl Renderer`), built by od-server in `routes/export`, `routes/media`, `routes/w3`.
- Every browser starts in one place: `src/lib.rs:Session::open` → `Session::launch`
  → macOS `Session::launch_detached` (via `src/launch_services.rs:launch`), else or on failure `Session::launch_child`.

## Invariants
1. On macOS an `.app` bundle's binary starts through `/usr/bin/open -n -g -a` (LaunchServices),
   so macOS does not charge Chrome and its updater to Contour (App Management prompt).
   - `src/lib.rs::the_installed_chrome_starts_through_launch_services` (macOS)
   - `src/launch_services.rs::only_a_bundles_main_executable_goes_through_launch_services`
2. The LaunchServices path passes the same switches as chromiumoxide (`src/launch_services.rs:switches`).
   - `src/launch_services.rs::switches_carry_ours_and_the_sandbox_opt_out`, `tests/launch_flags.rs::chrome_gets_the_switches_as_written`
3. Fall back to a child when: no bundle, a quarantined bundle not yet approved (`awaits_approval`),
   `ps` cannot run (`sandbox-exec`), the mark cannot be written, or LaunchServices failed.
   - `src/lib.rs::a_quarantined_bundle_skips_launch_services` (macOS)
   - `src/launch_services.rs::only_an_unapproved_quarantine_waits_for_gatekeeper`
4. One 20 s deadline (`LAUNCH_SERVICES_TIMEOUT`) covers `open`, DevToolsActivePort and connect;
   the whole open stays within `OPEN_TIMEOUT` (90 s) (`src/lib.rs:spent_a_launch`).
   - `src/launch_services.rs::an_open_that_hangs_times_out_and_is_killed`
   - `src/lib.rs::only_a_slow_launch_services_failure_costs_a_child_attempt` (macOS)
5. The profile is marked `od-opening` with our pid before `open` runs (`src/lib.rs:Profile::mark_opening`).
6. Every Chrome started is ended, even one LaunchServices starts late: a failed or cancelled
   marked profile is kept, ended now and watched for 120 s (`Profile::abandon`, `watch_abandoned`, `Drop`).
   - `src/lib.rs::a_browser_that_launch_services_starts_late_is_ended` (macOS)
   - `src/lib.rs::a_render_cancelled_while_launch_services_opens_ends_the_late_browser` (macOS)
7. Every new profile sweeps stale ones (`src/lib.rs:sweep_stale_profiles`).
   - `src/lib.rs::the_sweep_ends_what_runs_on_a_profile_given_up_on`, `tests/tmp_clean.rs::renders_leave_tmpdir_empty`
8. Close of a detached browser waits until nothing uses the profile, capped at 10 s (`wait_profile_unused`).
   - `tests/stuck_exit.rs::a_browser_that_never_exits_does_not_hang_the_export`
9. The failure reason survives the retry: an empty child error keeps LaunchServices' reason (`with_detached_reason`).
   - `src/lib.rs::a_child_that_says_nothing_keeps_why_launch_services_failed`
10. Still exports finish entrance animations and pause infinite ones; MP4 frames leave them playing
    (`src/script.rs:settle`, `Animations::Play`).
    - `tests/animations.rs::exports_wait_for_entrance_animations` (real Chrome)
11. Test bundles are never quarantined (else Gatekeeper shows a dialog on the developer's screen).
    - `src/launch_services.rs::unquarantine_clears_a_bundle_all_the_way_down`
12. Every CDP call after the page opens is bounded by `STEP_TIMEOUT` (60 s, `Session::step`); a lost
    connection ends the handler task, which fails every call waiting at once (`unless_gone`). Failure goes
    through `Session::close` like any other.
    - `tests/step_timeout.rs::a_page_that_never_settles_fails_every_export_in_time`
    - `tests/step_timeout.rs::a_browser_killed_during_an_export_fails_it_at_once` (unix)
    - `src/lib.rs::a_command_past_chromiumoxides_wait_says_so_in_words`; QA e2e `e2e/qa/export.qa.mjs:"EXP-16"`
    - `src/lib.rs::the_browser_counts_as_gone_once_its_connection_is_lost_or_ends` (`drive`)
13. A deck exports as its slides look: the vector PDF page is the paper (13.333 in), the page's own
    `@page` size is overridden, every slide prints with the display the deck's active classes give (pinned, a deck script may take the classes off), unbroken, holding its
    own `position: fixed` boxes (`contain: size layout`), exactly `w`×`h` with its padding and border inside (`border-box`),
    under ancestors without an inline width, margin, padding, border or `min-height` (screenshots: without
    `transition` or any property that holds a `fixed` box: transforms, `perspective`, `transform-style`, `filter`,
    `backdrop-filter`, `contain`, `content-visibility`, `will-change`),
    held again after a deck script answers the classes (`script::print_slides`, `show_slide`); the deck's own `@media print`
    rules still apply (`<deck-stage>` needs them); the editable PPTX background is the first box
    from the slide up with an opaque paint (its colour, else a gradient's first opaque stop), and every
    colour goes out as `rgb()` (`od-export/src/pptx_layout.js:paint`, `rgb`).
    - `tests/deck_framework.rs::a_framework_deck_exports_as_the_user_sees_it` (real Chrome, the shipped deck skeleton, oklch)
    - `tests/deck_templates.rs::bundled_deck_templates_print_as_their_slides_look` (real Chrome, PDF pages rasterised
      against the slide screenshots; a sample and a deck with `fixed` boxes in contained slides by default, `OD_DECK_TEMPLATES=all`
      for every bundled deck in `content/design-templates` and `content/skills`)

## Traps
- Headless Chrome as our child woke GoogleUpdater, and macOS asked for App Management on every update.
  Do not go back to `Browser::launch` for `.app` bundles (#231).
- Inside `sandbox-exec`, `ps` fails and `open` writes no stderr file: fall back to a child, do not start a second browser (#231).
- The GitHub macOS runner's Chrome is quarantined (flags `0181`), so `open` waits for Gatekeeper forever (#233).
- Killing `open` does not cancel the queued LaunchServices request; Chrome can start later with nobody to end it (#235).
- A `ps -A` right beside the child launch loses a fast-exiting child's stderr (chromiumoxide 0.9.1 race, still open) (#236).
- Because of that race the child error can be empty; keep LaunchServices' reason (#238).
- A finished animation with fill `none` leaves `getAnimations()`; never `Finish` before MP4 frames (#259).
- chromiumoxide 0.9.1 waits forever for a page promise that never settles and for every call after the
  connection is lost; a single command stops at its own fixed 30 s whatever `request_timeout` says (#269).
- Its handler stays pending on a WebSocket Close frame (no error, no end); Chrome quitting, killed or
  closed by `Browser.close` drops the socket without one (probed), so that case only ends at `STEP_TIMEOUT` (#270).
- Chrome lays a CSS `@page` size out and then shrinks it onto the paper; with our print `scale` on top,
  a deck's `@page { size: 1920px 1080px }` came out at 2/3 of the page (QA-120).
- `getComputedStyle` keeps `oklch()`, `lab()`, `color(srgb …)` as written; only the browser can turn them into
  sRGB, so convert in the page script, not in Rust (QA-121).
- A print probe that renders a template's `example.html` straight from its folder sees no `assets/`: the renderer
  lets only the page itself load. Bundle it first (`standalone::bundle`), as the export route does.
- Counting filled rectangles in the PDF misses white and gradient backgrounds; compare rasterised pages with the
  slide screenshots instead (`tests/deck_templates.rs`).
- Decks run scripts that answer the classes we move (`kami-deck` watches `.slide.active` and sets its strip's
  inline `transform`), and that write replaces our inline `!important`. A stylesheet `!important` rule did not
  help: Chrome kept the stale value after the inline write. Set the ancestors' overrides again after
  `odFinishAnimations` (`script::print_slides`, `show_slide`; review of #285).
- Holding an ancestor's `transform: none` while it has `transition: transform` starts a transition, and a running
  transition outranks inline `!important`: the strip stayed at the slide's offset (`kami-deck`). Hold `transition: none`
  with it. `will-change: transform` captures `position: fixed` and stacking like a transform (`ib-pitch-book`);
  so do `translate`/`rotate`/`scale`, `perspective`, `preserve-3d`, `backdrop-filter`, `content-visibility`
  (`tests/deck_templates.rs` `containing-chain`). `container-type` does not in Chrome (probed, review of #287).
- A light slide captured blank (page background only) differs from its page by under 5 %: the cell compare misses it.
  Decks written in `tests/deck_templates.rs` check each slide's background colour on both sides.
- Emulating screen media for the PDF broke `<deck-stage>` decks (1 page for 11 slides); a deck that prints unlike
  its screen on purpose (`ib-pitch-book`'s `@media print`) keeps doing so, listed with why in
  `tests/deck_templates.rs` `OWN_PRINT_LAYOUT` (decided on review of #287).
- On a coloured slide a gradient is mostly texture (ruled lines, soft spots): taking its first stop as the
  background broke 6 bundled decks. Only a box painted by a gradient alone covers its ancestors (#283 review).

## Check
- Most launch tests run only on macOS. CI runs them in the `chrome-macos` job of `../../.github/workflows/ci.yml`,
  only when a PR changes a file matching `CHROME` in `../../scripts/ci-changes.sh` (od-render-cdp, od-cli, `Cargo.lock`, CI).
- Two launch tests in `tests/launch_services.rs` are `#[ignore]`; when you change launch, run them by hand:
  `cargo test -p od-render-cdp --test launch_services -- --ignored` (see `../../docs/development.md`, "Browser launch").
