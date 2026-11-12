# Changelog

All notable changes to Contour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).
`make version V=x.y.z` turns the Unreleased section into a release
([docs/release.md](docs/release.md)).

## [Unreleased]

### Fixed

- Closing a panel, a dialog or a menu, switching file or conversation, or leaving a screen while a request is still out no longer crashes the app: History, the file dialogs (delete, move, rename, new), image upload in a Markdown document, Copy under a turn, the folder dialog, the export menu, the side chat, the Text tool and Manual edit, speaker notes and Settings pages finish what was asked and update only what is still on screen. A side chat question, a brief sent from Home, a file dialog's deletes and moves, and a speaker note are now sent to the end even if the Studio is left meanwhile, and a sketch saves its last strokes when its tab is closed within the autosave delay.
- Home: the files attached to a brief are uploaded to the end even if Home is left meanwhile, and a file that could not be uploaded is now said in a toast on whatever screen is open; before, it was dropped without a word. Copying a screenshot no longer crashes the app when the Studio is left before the snapshot arrives, and a folder link that fails after its dialog was closed is said in the Studio.
- Studio: a project deleted from another tab or window is now known from the first call that finds it, by every pane at once (chat, preview, files, the phone's tabs, notices), and stays so on every screen of the app.
- Picking a file in the quick switcher (Cmd/Ctrl+P) or a command in the palette with the mouse no longer throws an error in the page.
- Studio: a text edit made with the Text tool that could not be written now stays on the page with one line saying so, Try again and Discard; Enter on the same text writes it too. Before, the preview reloaded and the typed text was gone, and the note said "No permission to write" twice (QA-087).
- Projects: a duplicate or delete that fails now says "Could not duplicate the project. Try again." or "Could not delete. Try again." instead of "Contour could not finish this. Try again."; renaming, folder import, project instructions, creating a project (Home, New project, templates, plugin authoring), saving a template, skill or automation, and memory say what failed the same way (QA-088).
- More actions that fail now say what failed, e.g. "Could not start a new conversation. Try again." or "Could not attach the screenshot. Try again.": conversations (new, rename, delete, linked folders), the composer's design system, steer and stop, design systems (create, copy, edit, delete, download), plugins and skills, templates, media and MP4 export, automations and their proposals, Settings saves and connection tests, MCP servers (adding one from the catalog too), the terminal, handoff, export, Report issue, uploads (Files panel, Markdown documents, comment images, manual edit images), a new sketch, attaching a screenshot, sketch or annotation to the chat, and revoking a remembered GenUI answer. A failure inside Contour no longer reads "Upload failed: Contour could not finish this. Try again."; a list or read-only panel that could not load still shows the reason under its own heading. A Claude Design ZIP import that fails inside Contour says "Import failed. Try again." instead of "Import failed: HTTP 500" (QA-081, QA-088).
- Studio: a Markdown file or sketch that could be read on a second try no longer keeps the "Could not read" banner over it, and a failed request whose reply is a proxy's web page or a stack trace shows a short reason instead of the raw page (the page goes to the browser console).
- Studio: a file Contour has no permission to read is reported as "No permission to read" (in the Lint panel and elsewhere), not as a write problem (QA-085); the preview frame shows a short page saying the file could not be read, with Reload preview, instead of the raw `{"error":…}` reply (QA-086).
- Studio: a Markdown document whose file could not be read now says "Could not read this document. Nothing was saved over it." with Retry, instead of a loading placeholder that never ends; a lint that failed inside Contour or behind a proxy reads "Lint failed. Try again." instead of the raw reply.
- Desktop (macOS): after the window gave up on a design that kept crashing it, View ▸ Reload loads the page again instead of saying "This page isn't responding" about the dead page, and a stopped preview no longer says it hung when it crashed. A crash of the page a restart just loaded now reloads it instead of leaving the window blank.

## [0.2.0-beta.32] - 2026-11-11

### Fixed

- Studio: in a project that is gone (deleted from another tab, or a wrong link), the preview frame now says "This project was deleted or the link is wrong." with All projects instead of "Nothing to preview yet", and on a phone the Files and Preview tabs are off instead of opening an empty screen; opening a file from the chat, the quick switcher or the palette keeps the phone on Chat too (QA-080).
- Studio: a sketch whose file could not be read now says "Could not read this sketch. Nothing was saved over it." with Retry, instead of opening blank and saving the next stroke over the earlier drawing (QA-084); a lint that could not run keeps its Lint button, which shows why and runs it again with Retry (QA-083).
- Studio: a call that fails inside Contour (for example "database is locked") now says what was not done and to try again, such as "Could not save the comment. Try again.", instead of "The daemon hit an error: database is locked (HTTP 500)"; no message shows an HTTP status any more (it goes to the browser console). This covers Design Jury, comments, new sketches, GenUI answers, duplicate, transcript export, the Files panel, versions, memory, session mode and the side chat (QA-081).
- Studio: a manual edit that could not be written now says "The edit was not applied", not only the daemon's reason (QA-082).

## [0.2.0-beta.31] - 2026-11-11

### Fixed

- Studio: in a project deleted from another tab, the preview, Files panel, Versions and side chat now close with the chat's "This project was deleted or the link is wrong.", as for a wrong link, and no toast or turn says "Not found: project not found (HTTP 404)", "Restore failed" or "Upload failed" (QA-079).
- Studio: on a phone-width window, a project deleted from another tab while the Files or Preview tab is shown now switches to the Chat tab and its "This project was deleted or the link is wrong.", instead of leaving an empty screen.

## [0.2.0-beta.30] - 2026-11-11

### Fixed

- Studio: a run stopped while it was still being prepared, then Contour quit right away, now always ends as "Stopped"; if the quit came just as the run took the Stop, the turn was told as cut off by Contour closing.
- Studio: in a project deleted from another tab, every other action on it (files panel uploads, new files and folders, rename, move, delete, ZIP and copy path; comments; Design Jury, memory and the side chat; save as template; manual edits and lint; sketches; versions; exports) now also switches the Studio to "This project was deleted or the link is wrong." instead of showing "Not found"; and an export whose Save dialog is closed no longer hides another export's error (QA-076, QA-075).

## [0.2.0-beta.29] - 2026-11-11

### Fixed

- Automations: pressing Add routine twice quickly (a double-click) saves one routine, not two that each run the prompt on the schedule; the button waits while the routine is saved, and the save finishes even if the page is left (QA-074).
- Media: a failed generation says which provider failed and why in words (`OpenAI returned an error: upstream exploded`) instead of the provider's raw JSON; a refused key (401, 403) has a Set up providers button for every provider (fal.ai, Grok, MiniMax, Leonardo…), in every language (QA-077).
- Media library: an image generated on the Media page is labelled "Generated", not "Uploaded"; files the user uploads keep "Uploaded" (QA-078).
- Studio: an export still running when the Studio is left (an MP4 takes half a minute) shows again on return, and pressing the same format on the same file joins it instead of rendering and downloading it a second time; its result shows when it ends (QA-075).
- Studio: in a project deleted from another tab, sending a message, attaching a file, the handoff actions or the project's event stream ending now switch the Studio to "This project was deleted or the link is wrong." with All projects, instead of "Reconnecting automatically…", "HTTP 404" and "check the agent in Settings" (QA-076).

## [0.2.0-beta.28] - 2026-11-10

### Fixed

- Desktop app on macOS: a design that crashes its page's web process twice within 10 s no longer makes the whole app panic and quit; the app opens the project with the preview stopped and the Studio says the design crashed the page, as beta.25 meant to. View ▸ Reload and a design hung in a dialog loop no longer read the page's address in a way that panics once its web process has ended (QA-073).

## [0.2.0-beta.27] - 2026-11-10

### Fixed

- Desktop app on macOS: "This page isn't responding" closes by itself once the page answers View ▸ Reload, instead of staying up after the page has reloaded; a Reload pressed on a question the page has just answered does nothing (QA-072).
- Desktop app on macOS: a page's `alert`, `confirm`, `prompt` or file chooser (`<input type="file">`) that is open when the page's web process crashes now closes by itself, as if cancelled, instead of staying up over the reloaded page.
- Desktop app: a design that queues thousands of short timers no longer keeps a finished click, key press or touch open for a key the user presses in the meantime. A touch in a design opens at most one browser tab however many fingers it uses, and once a finger is lifted only the tap's own click can still open one: a drag, a long press or a second finger elsewhere in the app leaves nothing open for a later key. A key let go between a tap and its click, which can come a moment later, leaves that click opening nothing.

## [0.2.0-beta.26] - 2026-11-10

### Fixed

- Desktop app on macOS: stopping a design that hangs the window (a dialog loop, or Reload in "This page isn't responding") no longer leaves the window white with ⌘Q and the menu dead until Force Quit; the app opens the project again with the preview stopped, as beta.25 meant to (QA-070).
- Desktop app: the Studio's "Preview stopped" notice now says why: the design kept opening dialogs, stopped responding, or crashed the page twice in a row, in every language (QA-071).

## [0.2.0-beta.25] - 2026-11-09

### Fixed

- Desktop app on macOS: a design that keeps calling `alert()` after its dialogs were blocked no longer freezes the window. After 50 refused dialogs within a second the app ends the page's web process, opens the same project again with that design's preview stopped, and says so in the Studio with a Run again button. View ▸ Reload (⌘R) reloads the page, asking first when a manual edit is not applied; for a page that does not answer within a second, a native "This page isn't responding" alert offers Wait or Reload, which stops the design's preview. ⌘Q quits even while a page hangs the window. A web process that crashes twice within 10 s opens the page with the preview stopped instead of reloading for ever (QA-067).
- Desktop app on macOS: the preview's reload, or a file change that reloads it, lifts the dialogs blocked for the design, as opening another design does (QA-068).
- Desktop app on macOS: a long `alert`, `confirm` or `prompt` message shows in a scrolling box of fixed height, so the dialog's buttons stay on screen (QA-066).
- Media ▸ Providers and Settings ▸ Media: saving a provider with a new base URL and no key now says the saved key was dropped and must be entered again, instead of "Saved"; a key that comes from the environment is not reported as dropped (QA-069).
- Accessibility: the chat's file buttons have a name that matches their visible text in every language (WCAG 2.5.3).

### Security

- Bundled SQLite updated to 3.54.1 (RUSTSEC-2026-0459). Building from source now needs Rust 1.89.

## [0.2.0-beta.24] - 2026-11-09

### Fixed

- Leaving a screen while one of its requests is still out no longer stops the app with a panic (Studio, Media, Automations, chat). The work already sent to the server still finishes: a message sent just before leaving Studio is cleared from the composer, a media item queued for the Library is still created, every asset picked for deletion is deleted, an MP4 export is still recorded, and a message steered from the queue, or from a held queue, does not come back.
- Home: the template cards' accessible name now matches the name shown on the card (WCAG 2.5.3, axe `label-content-name-mismatch`).
- Desktop app: a `target="_blank"` web link, or a `window.open` the user's click or key runs, inside an `<iframe srcdoc>` in a design opens in the system browser instead of doing nothing, with the same rules as the design's own links: a link the page cancels opens none, nor does a `window.open` with no press of its own. One user activation opens at most one tab across the design and its frames, so a second link in a nested frame opens once the activation has run out (a few seconds) or after a press in the design itself. The design reads such a frame's `srcdoc` back as it wrote it (QA-065).
- Desktop app on macOS: a design's `alert()`, `confirm()` and `prompt()` show a dialog, titled with the page's origin, and wait for it, instead of returning at once with nothing shown (`confirm` gave `false`, `prompt` `null`). From the second dialog within 3 s, a checkbox stops the page's dialogs until it reloads, so an `alert` loop cannot hold the app. Windows and Linux already showed them (QA-064).

### Changed

- Updated 15 Rust dependencies and axe-core 4.14 for the accessibility tests.

## [0.2.0-beta.23] - 2026-11-08

### Fixed

- Desktop app: a design's `window.open` no longer opens the system browser on its own, on every load of the preview; only one the user's click or key runs does. The window opens no new window itself; the app's own links go through the shell (QA-061).
- Desktop app: an `<iframe srcdoc>` in a design shows its content instead of staying blank (QA-062).
- Desktop app: ⌘-click (Ctrl-click on Windows and Linux) on a web link in a design opens it in the system browser, as a plain click on a `_blank` link does (QA-063).
- Desktop app: one click or key press in a design opens at most one browser tab, as a browser's popup blocker allows, though the page calls `window.open` in each of its events (`pointerup`, `mouseup`, `click`; `keydown`, `keyup`) or the click is on a `target="_blank"` link. A `window.open` of a non-web address (`about:blank`, `mailto:`) does not use up the press. A key pressed outside the design and let go inside it opens none, nor does picking an element in comment mode.
- Desktop app: Enter on a `target="_blank"` link that the design cancels still lets the design open one tab on that key's `keyup`; a `window.open` after a `confirm()`, `alert()` or `prompt()` in the same click opens its tab, and a key let go after the dialog closes opens none; a drag-and-drop, or a click whose element the design removes, or a touch whose `touchstart`, `touchmove` or `touchend` the design cancels (a tap, a swipe on a carousel), no longer leaves the press open for a later key.
- Chat: when a run ends, the chat keeps following to its end and shows the Next chips, instead of sometimes stopping short with "Jump to latest" when the chips landed just as the panel under the chat went.
- Use template on a Motion film shows the film's own ratio and length (App feature tour: 9:16, 12 s) instead of Motion's defaults (16:9, 10 s); the length is offered in the length list when it is none of the usual ones. The template list carries each film's size and length (`film`) (QA-055).

## [0.2.0-beta.22] - 2026-11-07

### Fixed

- Comments: clicking the comment that is already picked scrolls the preview back to its element.
- Desktop app: a `target="_blank"` web link in a design opens in the system browser instead of doing nothing (QA-054).
- Use template on a Motion film sets Motion's ratio and length (16:9, 10 s), as the Motion chip does, instead of showing 1:1 and 3 s that were never sent (QA-055).
- Remix names the project as its template's card reads ("Before and after"), not by the template id (QA-056).
- Comments on an element taller than the preview: the note box stays inside the preview (QA-057), picking the comment shows the element's top (QA-058), and its pin hides once the element is scrolled away instead of sitting on the top edge (QA-059).
- The comment note box names its element with spaces between blocks, cut at a word (QA-060).

## [0.2.0-beta.21] - 2026-11-06

### Added

- 13 more Motion templates in Templates, under the Industry "Motion":
  product launch, event ticket promo, a three-step explainer, a 9:16
  kinetic type manifesto, a flip-clock countdown, before and after,
  a 9:16 app feature tour, a square customer testimonial, an animated
  line chart, a logo stinger, a 9:16 year in review, a hiring post and
  a café menu board. Each is a 5–14 s HyperFrames film with English and
  Chinese names that previews on its card, opens with Remix or "Use
  template", and renders to MP4 with `media generate`. Brands in them
  are made up; they use system fonts and inline SVG, download nothing
  and are silent.

- A chat turn shows its total run time on the line over its work:
  "Step 2 of 3 · Writing the page · 0:12" counts up while the agent
  runs, and "Worked for 1:23 · 4 steps" stays put once it is done, also
  after a reload.
- Comments: when reloading the comment list fails (Contour away, a
  request error), the Comments tab keeps the comments it shows and adds
  a line saying the list may be out of date, with a Retry button.
- Comments: a dot on the Comments button marks comments that came in
  since the conversation was opened and were not shown yet. Only the
  comments the tab actually lists are marked read, not those on another
  file or those that arrive later.

### Changed

- A chat turn's plan, thinking and tool calls now read as one task you
  can fold, in the order the agent did them: the plan sits where the
  agent wrote it instead of at the top of the reply. The task is open
  while the agent works and folds once it succeeds with its plan done;
  the reply, errors and the files from the turn stay outside it, in
  view. While the agent works, that line is the one place that shows
  its progress and clock; the "Step N/M" pill over the composer shows
  only while that line is scrolled out of view.
- Comments: a late answer for another project, another conversation or
  an older reload no longer replaces the comments of the one open.
- Comments: the outline in the preview follows the comment picked in
  the tab, again after the page reloads, and goes away when that comment
  is no longer listed. When its element was removed from the page, the
  comment says so instead of outlining nothing.

### Fixed

- ⌘W in the Studio quit the app on macOS and stopped the running
  agent. It now closes the tab, as the shortcuts dialog says; File ▸
  Close Window no longer takes ⌘W (QA-042).
- On macOS the window's close button quit the app and stopped its runs.
  It now hides the window, leaving full screen first; the app and its
  runs keep going, and the Dock icon brings the window back. ⌘Q still quits. Windows and Linux still
  quit when the window closes (QA-043).
- Links that open in a new tab, such as "Install guide ↗" during setup,
  "Get Chrome" and an update's "View", did nothing in the app. They now
  open in the system browser (QA-044).
- Integrations ▸ Use everywhere told app users to run `od`, which on a
  Mac without the CLI is the system's octal dump. In the app it now
  shows the app's own command (QA-046).
- Export in the app said "Exported index.pdf" even when the file was
  saved as `index (1).pdf`, and never said where. It now asks where to
  save and names the file it wrote (QA-048).
- The menu bar stayed in English after switching the language in
  Settings. It now follows the app's language (QA-049).
- The shortcuts dialog listed only `?` for itself; it now lists ⌘/ too,
  which also works in the browser (QA-050).
- Opening Settings with ⌘, before finishing setup ran the agent's
  connection test again, a second paid prompt. The earlier answer is
  kept while the agent stays the same (QA-051).
- Settings ▸ Notifications: the "Failure" label no longer wraps away
  from its sound picker (QA-052).
- On a Mac with Amp installed, every launch asked "Contour would like to
  access files on a network volume", and Allow did not stick across
  updates. Checking which agents are installed ran `amp --version`, and
  Amp looks for two optional add-ons under `/home`, a network mount on
  macOS. Amp now runs, for the check and for chats, with those add-ons
  turned off, so it no longer touches `/home`.

## [0.2.0-beta.20] - 2026-11-06

### Fixed

- Check for Updates never offered Update now in the installed app, and
  Settings ▸ Project storage ▸ Add folder opened no folder picker: the
  app refused every call from its own window, which it loads from the
  daemon's address. Those calls now work for that address only. Update
  now works from this release on; 0.2.0-beta.18 and 0.2.0-beta.19 are
  updated by hand.

## [0.2.0-beta.19] - 2026-11-06

### Fixed

- A file saved from the preview while one run ended and another
  began showed up as "Edit" in the second run's turn, though its agent
  never touched it.
- Stop sent the queued message right away and started a new run. Now
  the queue waits after a Stop, showing "1 message waiting" with Send
  and Discard (UI-003).
- When Contour quit or crashed mid-run, the turn said "Stopped" though
  nobody pressed Stop, and after a restart it read failed or canceled
  depending on the screen. A run cut off by the app going away now ends
  failed as "Lost the connection to Contour" everywhere, with Retry
  only. A message sent while Contour is unreachable gets that one line,
  without the browser's "TypeError: Failed to fetch", the word
  "daemon" or an Open Settings button, and Retry turns on once the
  connection is back (UI-008).
- The "Connection to Contour interrupted" card covered the composer's
  buttons; it is now one banner across the top of the Studio, covering
  nothing and seen with the chat folded or on a phone's Preview tab
  (UI-009).
- A file uploaded while a run was going showed in that turn's files as
  written by the agent ("Wrote sample.txt"). Uploads, and files made in
  the app (New file, a sketch, a preview capture), stay out of the turn
  unless the agent changes them (UI-012).
- Dragging the chat's scrollbar up, or pressing Home or Page Up, while a
  run streamed could snap back to the end when output landed in the same
  moment. The move up now counts, and the list keeps its place.
- The chat (and the side chat) no longer pulls you back to the bottom on
  every new token while a run streams: scrolled up to read earlier turns,
  even by a trackpad's few pixels at a time, the list keeps its place and
  offers "Jump to latest"; back at the end (within 48 px) it follows the
  output again, and sending a message always goes to the end. A message
  the queue sends on its own after a run leaves you where you are reading.
  At the end, the list keeps following when a panel under it goes (a
  queued message removed) or the window grows, and a trackpad pinch does
  not stop it.
  "Jump to latest" is a full touch target on touch screens, and pressed
  from the keyboard it puts focus back in the composer.
- While a run goes on, the Queue button sits on one row with its icon
  beside the label; it took the queue panel's column layout and dropped
  onto a line of its own in narrow windows. On a phone, Stop and Queue
  show as icons so the composer's buttons keep to one row.
- The run status line says what the agent does in words ("Writing
  index.html", "Waiting for the agent") with the last file it wrote and a
  run clock, instead of the agent's raw label ("requesting") that stayed
  put for minutes. The clock counts from the run's start, also after a
  reload.
- Empty "Thinking" blocks no longer show in the chat.
- Conversations keep their number ("Conversation 1" is the oldest) when
  another one moves up the list, and the side chat no longer shows in the
  main conversation list.
- The preview's file name ends in an ellipsis in narrow windows instead of
  being cut to a letter.
- Audio a turn produced shows its length before it plays (it read 0:00).
- A page with a carousel built from `.slide` elements (a testimonial
  slider on a landing page) was taken for a slide deck: the preview
  shrank to a 16:9 frame with Present, Notes and a thumbnail rail. A bare
  `.slide` now makes a deck only when the slides fill the page, never
  inside an ARIA carousel (`aria-roledescription="carousel"`/`"slide"`)
  or a carousel library's box (`.swiper`, `.carousel`, `.splide`…); every
  bundled deck is still a deck.
- While the agent works on a project's first page, the empty preview
  marks "Describe what you want" done and the agent's step current,
  instead of asking for a description the user already gave.
- A Motion composition started with `od media scaffold` fills the preview:
  its stage scales down to the window, where before only its dark
  top-left corner showed for the whole run. The render is unchanged.
- Projects lists a Motion film as "Motion" (with its own filter), not
  "Media", with a frame from the middle of the film as its cover.
- In a narrow window the file tabs shrink and scroll by themselves (a
  mouse wheel scrolls them too), so the "+" launcher and the focus toggle
  after them stay in reach; the active tab scrolls into view and Side chat
  stays pinned at the end of the row.
- Settings ▸ Models: no tall gap between the Local CLI / API switch and
  the card under it, and "Advanced: proxy & custom paths" opens with the
  same chevron as "Other agents" instead of the browser's triangle.
- Onboarding numbers its first step ("1 Agent") like the brand step under
  it.
- Settings in a window under 1180 px wide puts each section in one
  column; the Models card no longer shrinks to a 200 px column
  ("Installe…").
- A deck's thumbnail rail opens at slide 1: a thumbnail moving its copy of
  a deck with no keyboard handling to its slide no longer scrolls the rail
  (or any page around the preview).
- Project covers and template thumbnails no longer log a "Blocked script
  execution" console error for each script of the page: the daemon leaves
  the scripts out (`raw/<path>?still=1`, `design-templates/<id>/example?still=1`)
  and keeps the page's no-network CSP.
- Moving up a fast-streaming chat with Page Up, Arrow Up, Home, a finger
  or the scrollbar could be pulled back to the end before the move
  landed. Those now stop the follow at once, and nothing scrolls under a
  finger on the list or a grabbed scrollbar. A wheel in a box inside the
  chat (a failed turn's agent output) scrolls that box and keeps the
  follow.
- A page there before a run, which the agent left alone, showed in that
  turn as "Wrote page.html" once the user saved it from the preview. Only
  a file the agent changed stays the turn's.
- Deleting a conversation with a message waiting after a Stop left that
  message in the browser's storage for good; it now goes with the
  conversation. A message sent past such a queue while Contour was
  unreachable changed its hint from "the turn before was stopped" to
  "the connection to Contour was lost"; it keeps the first reason.
- A project cover of a very large page (images inlined, tens of MB) held
  about three copies of it in memory per cover shown. A page over 5 MiB
  now gets a blank cover, and stripping a smaller one's scripts no
  longer copies it once more.
- A deck whose slides render after the page loaded (fetched content, a
  framework's first render) never got Present, Notes or the thumbnail
  rail; the preview now looks for slides again for 10 seconds after
  load.
- A Motion project's cover stayed blank for a recorded WebM film, whose
  header gives no length; it now shows the film's middle frame too.

## [0.2.0-beta.18] - 2026-11-05

### Added

- Update now in the desktop app. When Check for Updates finds a newer
  release, Update now downloads its build for this system, installs it
  only if its signature matches Contour's updater key, and offers Restart;
  the dialog shows the download's percent, Cancel, and a clear message
  when the download fails or its signature does not match (nothing is
  installed then). Nothing downloads until the click. The `.deb`, dev
  builds and the browser keep View release, with a short hint. Releases
  carry `latest.json` and a `.sig` for each build
  ([docs/release.md](docs/release.md#updater-signing)).

### Fixed

- The installed app's binary can run its daemon alone:
  `od-desktop daemon start --port 0` (and `daemon status`), with `od`'s
  flags and `OD_DATA_DIR`, starts the daemon without a window or a Dock
  icon instead of opening a second app, and serves the app's bundled
  content (macOS `Contents/Resources`, `/usr/lib/Contour` on Linux), so
  QA and agents can test the real app beside the one that is open. See
  [getting started](docs/getting-started.md#run-the-installed-apps-daemon-without-its-window).
- `od-desktop --json daemon status` and `od-desktop --daemon-url <url>
  media …` (`od`'s global flags before the command) opened a second app;
  they now run the command like `od` does.
- `od-desktop --json mcp` (`od`'s global flags before `mcp`) opened a
  second app instead of the MCP stdio server.
- `od-desktop --daemon-url URL mcp` ignored the URL and served the daemon
  in `daemon.lock`; it now uses that URL, as `od` does (a `--daemon-url`
  after `mcp` still wins).
- `od-desktop daemon start` finds the agents the open app finds: it asks
  the login shell for its `PATH` (the registry on Windows) like the app,
  instead of using the terminal's, which an agent running QA often has
  cut down.
- On Windows, a second daemon on a data folder already in use (the open
  app's, or another `od daemon start`) is refused with the first one's
  URL, as on macOS and Linux; before, both ran on one store and the
  second overwrote `daemon.lock`. The lock is held on
  `daemon.lock.held`, so `daemon.lock` stays readable for `od mcp` and
  the client commands.

- A reference link typed with the colon or a slash of its scheme missing
  (`https//vimeo.com/1`, `http:/youtu.be/x`) is refused as a link, with
  how to attach the file instead; before, the user saw "No such file or
  directory (os error 2)".
- Two `od media reference` runs of the same film into one folder at once
  no longer wreck each other: the second cleared the frames the first was
  writing, which then finished with a `facts.json` that listed missing
  frames. A run now holds the folder with an OS file lock on
  `.od-running` while it writes, and a second one, from the same daemon
  too, stops with "another run is writing `<dir>`; wait for it to
  finish". The same for `od media compare --out`. The OS frees the lock
  however the run ends, so a run that was killed does not keep the folder
  held; a finished run never removes another run's lock, and files an
  older lock left behind (`.od-running-stale-*`) do not make the folder
  count as someone's.
- `od media compare --out` that fails in a folder it made no longer leaves
  that folder behind with only its `.od-compare` mark, like
  `od media reference`.
- Two reference films with one file name in different folders
  (`a/clip.mp4`, `b/clip.mp4`) no longer share `reference/clip/`: the
  folder's `.od-reference` mark names the film by its path from that
  folder (`../../a/clip.mp4`), so the second goes to `reference/clip-mp4/`.
  One film keeps its folder however it is given (relative to any working
  folder, absolute, `a/../a/clip.mp4`, other letter case on macOS and
  Windows), from the CLI or through `--project`, and after the project is
  moved. A folder from beta.17 or older (its mark names only the file) is
  taken by the first film of that name, in any letter case, to run after
  the update, whichever folder that film is in; from then on it is that
  film's.
- A bare host given as the film (`vimeo.com`, `youtube.com`) is refused as
  a link, like `vimeo.com/123` and `www.…`; a missing `clip.mov`,
  `logo.ai` or `config.in` is still told as a missing file.
- `od media reference --project` prints the daemon's refusal as a plain
  sentence, without the `400 BAD_REQUEST:` before it; it still exits 1.

## [0.2.0-beta.17] - 2026-11-04

### Fixed

- A song with cover art is refused as having no video stream, like any
  sound file; before, it failed half way with "ffmpeg wrote no frame" and
  left an empty `reference/<name>/` folder. A reference run that fails no
  longer leaves a folder it created.
- A link pasted without `https://` (`youtube.com/watch?v=…`,
  `youtu.be/…`, `www.…`) is refused as a link, with how to attach the file
  instead; before, the user was told a project file was missing.
- A reference that is not a video (a text file named `.mp4`, a PDF, an
  image) is refused with "`<file>` is not a video Contour can read" or "is
  a still image, not a video", and what to attach instead; before, the
  user saw ffprobe's own errors, or for an image an ffmpeg command that
  cannot work.
- Two reference films with the same name (`clip.mp4` and `clip.mkv`) no
  longer share `reference/clip/`, where the second silently replaced the
  first: the second gets `reference/clip-mkv/`, also when both run at
  once. `--out` into another film's reference folder is refused.
- Running `od media reference` again no longer deletes a frame the user
  saved in its `scenes/` or `grid/` under a numbered name such as
  `010.png`: only the frames the last run listed in `facts.json` are
  replaced. `od media compare --out` likewise replaces only the pairs it
  wrote before. A rerun that would write a frame over such a file stops
  and says so.

## [0.2.0-beta.16] - 2026-11-03

### Added

- `od media reference <video>` breaks a reference film the user attached
  into a folder to build from, by default `reference/<video name>/` in the
  project: `facts.json` (length, size, fps, sound, the seconds of each
  scene cut), a frame per scene in `scenes/`, about a frame a second in
  `grid/`, `contact-sheet.png` and `audio.m4a`. Local files only, at most
  200 MB and 180 s; a link is refused with how to attach the file instead,
  and nothing is downloaded. It uses the installed ffmpeg and ffprobe, as
  the MP4 render does. A portrait phone clip is described as it plays, and
  sound that runs past the picture does not lengthen the film, in MKV and
  WebM as in MP4. `--out`
  takes only a new or empty folder, or one it wrote before: other files are
  never removed or replaced.
- `od media compare <a> <b>` scores two films 0–1 at the same points of
  their length (default 8, `--points`), as a table or `--json`, and with
  `--out` writes each pair of frames side by side.
- The motion-design skill reads an attached reference video with these
  commands before planning, and after rendering compares the film with it
  and repairs the lowest-scoring scenes once.

## [0.2.0-beta.15] - 2026-11-02

### Fixed

- In the @ list, Enter picks the top row for what was typed even when the
  pointer rests over the list; before, the row under the pointer kept
  taking the highlight and the list redrew in a loop.
- The @ list stays open when you type @ right after clicking elsewhere;
  the click used to close the list 150 ms later.
- pulldown-cmark 0.13.6 (RUSTSEC-2026-0433).
- The last30days template's report page names system fonts only, so its
  preview no longer asks Google Fonts for Inter and JetBrains Mono.
- A run that writes its page only with a shell command (no Write tool) no
  longer ends with "The run ended without writing any file." beside the
  turn that lists the page: the run's written files now include the files
  found outside the file tools.
- Past 50 rows, the header of "Files from this turn" counts the files not
  listed too, so it agrees with "Done · N files written" (it said "50 new"
  beside 60 written).
- Remixing a Motion film makes a film project (Media, intent
  `motion-design`, the HyperFrames video model), as Use template does; it
  used to be filed as a Prototype.

## [0.2.0-beta.14] - 2026-11-02

### Added

- `OD_HYPERFRAMES_BIN` points Contour at a HyperFrames CLI in place of the
  `hyperframes` found in Homebrew's folder, the system folders or on
  `PATH`. Agent runs get the CLI found as `OD_HYPERFRAMES_BIN`, so the
  HyperFrames template's `"$OD_NODE_BIN" "$OD_HYPERFRAMES_BIN" lint` runs
  it (`od` hands that command line to the CLI). Without one the variable
  is not set; MP4 rendering never needed it.

### Fixed

- "Files from this turn" no longer lists files another run of the same
  project wrote while this one ran. When two runs of a project overlap, a
  file written outside the file tools joins a turn only when it changed
  while that run ran and no other run of the project did; a file changed
  while both ran is listed by neither (a Write or Edit tool row still shows
  it in the run that used the tool).
- The counts of "Files from this turn" ("N new · M edited", "+N more")
  and the "Done · N files written" line now count files, not rows: a
  folder row such as `frames/ (1000 files)` counts as 1000 files.
- Design templates load nothing from outside loopback, so the preview no
  longer blocks a request and shows a broken page. This covers each
  example page and the pages and assets a template's output copies
  (`examples/`, `assets/`, `templates/`). Google Fonts are gone and every
  font stack ends in system fonts (condensed display faces fall back to
  Avenir Next Condensed, Arial Narrow or Impact). Chart.js charts are drawn
  as SVG in the page: three example decks, the html-ppt demo deck and the
  html-ppt bar, line, pie and radar chart pages. The html-ppt code page
  colours its snippet in the page instead of loading highlight.js. The
  guizang-ppt deck carries its 34 Lucide icons (ISC) instead of loading
  Lucide from unpkg, and warns in the console for an icon it lacks.
  Contributor avatars show initials, and demo links to outside sites
  (tickers, Gmail, Linear, Notion, GitHub) point at `#`. A test checks
  every template, counting only `localhost`, `127.0.0.1` and `[::1]` as
  loopback.

## [0.2.0-beta.13] - 2026-11-02

### Added

- 18 Motion templates in Templates, filed under the Industry "Motion": 13
  templates to adapt (logo reveal, kinetic typography, animated bar chart,
  KPI counters, lower third, quote card, countdown, vertical social story,
  channel intro, transition pack, end screen, chapter title cards, product
  feature callouts) and 5 finished examples (a product teaser, a bike
  share year in numbers, a 9:16 app story, an event opener, a weather
  card). Each is a 5–15 s HyperFrames film that previews on its card,
  opens with Remix or "Use template", and renders to MP4 with
  `media generate`. They are written for Contour under Apache-2.0, use
  system fonts and inline SVG, download nothing and are silent.
- 30 new gallery templates written for Contour: 10 WebGL pages (noise
  gradient, particle flock, rotating 3D product, data globe, topographic
  contours, liquid chrome, star warp, cell field, ocean point grid,
  dithered pixel scene), 14 decks (field notebook, transit map, star
  chart, menu cards, lab notebook, gallery wall, ledger, departure board,
  warming stripes, taped moodboard, redline postmortem, floodlit season
  review, clinic flow, type specimen) and 6 landing pages (risograph bike
  co-op, tea subscription, board-game café, CLI tool, architecture studio,
  farm box). Filter by Prototype or Deck to find them. Each loads nothing
  from the network: system fonts and drawings in the page. A WebGL page
  shows a still poster in the gallery and in browsers without WebGL, and
  stacks its text above the visual on a phone; a deck scales to any frame
  without script and moves with keys, wheel, swipe or its dots. Remixed,
  none of them shows a P0 lint finding.

## [0.2.0-beta.12] - 2026-11-02

### Fixed

- Files the agent writes with a shell command (`ffmpeg`, `python`, `cp`)
  now show in "Files from this turn", so a rendered MP4 or image opens
  from the turn like a file written with the Write tool. New files are
  listed as written, changed files as edited; folders the file list skips
  (`.git`, `node_modules`, build output, hidden folders) are skipped here
  too, and a project with more than 20,000 files is not compared. A
  folder with more than 20 such files of one kind (frames from `ffmpeg`)
  shows them as one row, `frames/ (1000 files)`; a film, page, PDF or file
  of another kind in that folder keeps its own row and opens. A turn lists
  at most 50 rows, and one "+N more" counts the rest.
- A live artifact refresh that fails while reading or writing its files
  is logged as `refresh_failed` with its source type, not as `error` with
  `unknown`. Other daemon errors during a refresh stay `error`.
- Renaming a project returns the time it was last updated as stored, even
  when a message was sent in the meantime.

## [0.2.0-beta.11] - 2026-11-01

### Added

- `od tools live-artifacts create|list|update|refresh`: the command the
  Live data page skill calls to register its page. It reads
  `artifact.json` with the `data.json`, `template.html` and
  `provenance.json` next to it and uses the agent run's token, so the
  page shows in the project's Live tab without the agent calling the API
  or writing `index.html`. `od tools connectors list` prints an empty list
  and `execute` says Contour has no connectors yet.
  The desktop app answers these commands too.

### Changed

- The six Live data page examples no longer promise data synced from your
  apps or email: they say the page refreshes from a file in your project.

### Fixed

- Motion films have their sound: `od media generate --model
  hyperframes-html` mixes the `<audio>` tracks of the composition and of
  the sub-compositions it loads into the MP4 (one AAC track, cut to the
  film's length, with each track's start, length and volume). A track
  whose file is missing fails with the file's name and writes no MP4,
  unless the film is declared silent; a remote URL fails as "local only".
  A film without `<audio>` renders as before. Export ▸ MP4 and
  `od export --format mp4` mix the same tracks. Mixing 2 or more tracks needs ffmpeg 4.4 or newer.
- A Motion render that requires a timeline waits up to 15 s for it to
  register, even when the composition declares `data-duration`; a
  timeline added by a slow script no longer fails the render.
- Client commands run by hand (`od-desktop media models`, …) reach the
  running app through `daemon.lock` when nothing answers at the default
  `127.0.0.1:7456`, as `od-desktop mcp` does. A URL given with
  `--daemon-url` or `OD_DAEMON_URL` is used as is.
- Recent projects follow your last message: renaming or opening a
  project and the agent's reply no longer move it to the top; sending a
  message or Retry does. A queued message counts from when you queued it,
  and a scheduled run moves nothing.
- The template gallery no longer asks Google Fonts for every preview
  (the page's policy blocked it anyway): the example pages are served
  without those links and use their fallback fonts.

## [0.2.0-beta.10] - 2026-11-01

### Added

- Motion makes an MP4 with Contour's own tools. `od media scaffold`
  creates an editable composition (`index.html`, `hyperframes.json`,
  `meta.json`) and leaves a folder that already has a source alone.
  `od media generate --model hyperframes-html --composition-dir <dir>`
  renders it with the Chrome and ffmpeg already installed, as a task that
  `od media wait` follows, and prints the film's length, fps and size plus
  three review frames. `od media models` lists the model. It prints
  `queued <task-id>` on stderr first; for a film over 20 s the skill
  queues with `--no-wait` and waits in 90 s calls, so a render longer than
  the agent's shell limit is not lost. A failed render is logged.
- The rendered MP4 shows in "Files from this turn" (an edit when a
  re-render replaced it), counts as the turn's deliverable, and opens in
  the preview when the composition declares
  `<meta name="od-motion-output" content="video">`.
- A test checks that every `$OD_*` variable and `"$OD_BIN" <group>
  <command> --flag` in the bundled content exists in Contour; the few
  that do not yet are listed with a reason.

### Fixed

- The Motion chip no longer leaves the agent to build its own headless
  Chrome and ffmpeg pipeline: `"$OD_NODE_BIN" "$OD_BIN" …` in skills now
  runs `od` (both variables name it), and the desktop app answers those
  commands without opening a window. Without ffmpeg or Chrome, the render
  fails with "ffmpeg required" or "Chrome required" and how to install
  it, and the skill tells the agent to stop and report it.
- `od media wait` accepts `--since` and exits 2 while the task still runs,
  as the copied HyperFrames and e-commerce skills expect. The desktop app
  keeps these exit codes (2, 66) instead of turning them into 1.
- A composition whose timeline never runs (a missing local script, or a
  script that throws before registering `window.__timelines.main`) now
  fails with the reason instead of rendering a still MP4.

## [0.2.0-beta.9] - 2026-11-01

### Fixed

- Opening a folder Contour cannot write to is refused and no longer adds a
  project, one more per try. Read-only folders are not supported.
- A folder Contour may not write to, or a full disk, is named in plain
  words ("No permission to write: …", "Disk full: …") instead of "The
  daemon hit an error: io error: … (os error 13) (HTTP 500)". The API
  answers `NO_WRITE_PERMISSION` (403) and `STORAGE_FULL` (507).
- MCP: a file `write_file` saves under another name ("trang chủ.html" as
  "trang-chủ.html") says so in `savedAs`, and `get_file` and
  `delete_file` find it under the agent's own name.
- Quitting Contour (or stopping `od`) while the agent works stops the
  agent, as Stop does, and records the turn as stopped. Before, an agent
  that ignores a closed pipe (Claude Code) kept editing the project after
  the app was gone (QA-026). `od` stops the agent as soon as it gets Ctrl-C
  or SIGTERM, even with the run open in the browser, and no longer waits
  for the browser to close an open project. Quitting can take a few
  seconds when the agent ignores SIGTERM: Contour waits for it to end.
- A turn cut short because Contour closed or crashed says so ("Contour
  closed while this turn was running") and offers Retry, instead of
  "daemon restarted" and a pointer to the agent in Settings (QA-030).
- Your usage: an agent found from its setup dialog (DeepSeek Harness,
  detected again after installing `dsh`) is setup step 2 at once. Before,
  the step waited for the next agent list, so quitting before the test
  read "stopped at: agent" and the step got a later time.
- Your usage: previewing a project an automation made, or the media
  library, is no longer the first preview of setup, as neither is the
  first project. Before, setup could read "Reached step 5 of 5" with no
  project made.
- Live reload: when macOS drops file events under load (FSEvents
  `MustScanSubDirs`) or Linux's inotify queue overflows, the preview and
  file list reload, instead of missing the change until the next write.
- Manual edit: clicking text inside a marked section picks that text, so
  a paragraph of a page the agent wrote can be edited. Before, the click
  picked the whole section (QA-027).
- Export: PowerPoint and MP4 are offered only when the page has slides or
  a composition root. Before, a plain page offered both and the export
  failed with HTTP 422 (QA-031).
- Studio: with an edit not applied, the browser's Back asks every time,
  even after the preview reloaded. Before, a reload could make Back do
  nothing or Leave stay on the page (QA-033).
- Home: Prototype, Slide deck and Video with no template send no skill,
  as the original does; the daemon decides. Before, a featured skill the
  user never picked was used.
- Onboarding: Continue while the connection test runs no longer panics.

## [0.2.0-beta.8] - 2026-10-30

### Added

- Settings ▸ About ▸ Your usage shows how far first-run setup got, out of
  5 steps: the app opened, an agent found, an agent (or API key) passed
  its test, the first project, the first preview. Each step is recorded
  once, the first time it happens, on this machine only and never sent
  anywhere; an automation's project is not a first project. It reads
  "Reached step 5 of 5" with the time from the first open to the first
  preview, or the step still to come ("Next step: agent" for someone who
  quit at onboarding without an agent). Copy summary adds a `Setup:` line
  that names where setup stopped (`stopped at: agent`). A data folder
  made before this version says "not recorded" instead of a guess. In
  every UI language.

### Changed

- Your usage: a number without enough data reads "Needs 3 projects to
  measure (you have 1)", not "Not enough projects yet (1 of 3)", which
  read like 1 of 3 projects exported. "Since 0.2.0-beta.7" is said once,
  under the title, instead of under every number (in the summary, once on
  its first line). Copy summary points to the "Share your usage" thread in
  [GitHub Discussions](https://github.com/quangphung218/contour-design/discussions),
  and to an issue with steps to reproduce for a bug. In every UI language.

### Fixed

- A link to a Settings section (the quick menu, the Memory toast,
  `/settings#settings-about`) opens Settings at that section and keeps it
  there while the sections above finish loading. Before, the page always
  opened at its top. The chat's and Home's links to MCP servers now point
  at that section too.
- The Settings nav marks the section in view, and follows as the page
  scrolls (`aria-current` for screen readers).
- When a run ends the chat scrolls down to the Next chips under the reply.
  Before, they showed only after the run and stayed half hidden behind the
  composer.
- "Your first design is ready" shows at the bottom of the preview, no
  longer over the page's header and its first button.
- On a phone (390 px) the preview head is one row: Preview/Code, device and
  More, which opens reload and the edit and zoom tools on a second row.
  The Next chips under a finished reply include Export, which opens the
  Export menu on the Preview pane; before, the Chat pane had no way to it.
- A reply with a Plan no longer repeats "Updated plan" as rows under it
  each time the agent updates the plan: the Plan list shows the changes.
- Integrations ▸ MCP opens with one line, "Forwarded to: …"; the notes on
  stdio-only adapters and on agents not forwarded to are under Details.
  With no MCP server yet, Add server is in the empty list, and Save stays
  a plain, disabled button until there is a change to save.
- On a phone (480 px or narrower) the rail, the menus, the Settings nav,
  the Chat/Preview/Files switch, the Next chips, the preview head and the
  Settings buttons are 44 px touch targets. The rail drops Collapse, which
  does nothing there, and Keyboard shortcuts.
- Home's template gallery links to every template, "All templates (124)",
  next to "Show all 60" (the chosen type only), so the two counts no longer
  look like a mismatch. Its industry tags read like the Templates page's:
  "HR" and "AI", not "Hr" and "Ai".

## [0.2.0-beta.7] - 2026-10-29

### Added

- Settings ▸ About ▸ Your usage: three numbers worked out on this machine
  and never sent anywhere. Time from creating a project to its first
  preview (median), the share of projects exported or with 3+ chat turns,
  and the share of runs that failed, by cause (agent missing or didn't
  start, agent error, timed out…); runs you stopped are listed apart and
  not counted. Copy summary puts a short English summary on the clipboard
  (app version, OS, agent and the three numbers; no project name, path,
  prompt or file content) to paste into a GitHub discussion or issue; the
  text shows below the button, so it can be read first or copied by hand.
  With fewer than 3 projects or runs it says how many there are, "Not
  enough projects yet (1 of 3)", instead of a number. Previews (HTML and
  React components) and exports (Export menu, MP4, screenshot PNG, ZIP)
  are recorded from this version on, so the first two numbers count
  projects created since 0.2.0-beta.7 that someone started empty: not
  imported, remixed, duplicated or made from a saved template, nor the
  media library or an automation's project for one run. The bug report
  and feature request templates point to it. In every UI language.

### Fixed

- A deck lays out at its 1920×1080 design size and is scaled to fit the
  preview, so a narrow window or a phone shows the same slide, smaller.
  Before, a deck that does not scale itself (the html-ppt templates) was
  laid out again at the pane's width and its text ran off the slide. The
  slide thumbnails use the same size. On a phone the slide sits at the
  top of the preview, by the deck bar. A deck rewritten as a page is laid
  out as a page again.
- `od mcp` (the CLI) finds the app like `<app> mcp` does: a
  `--daemon-url` or `OD_DAEMON_URL` with nothing answering within 1 s
  gives way to the running app's URL in `daemon.lock`, said on stderr,
  and with neither given it uses `daemon.lock` (the default port only
  when there is none).
- Settings ▸ About's subtitle mentions updates in every UI language, as
  it does in English and Vietnamese.
- The three Export plugins no longer share the tile "ET": a plugin whose
  initials match another's shows other letters of its name (Export to
  Vue is EV, to React ER, to Next.js EN), in every UI language.
- Templates: the type chips no longer include "Template" between
  Prototype and Video. The two dashboards it held are under Prototype,
  where they open; the chips read Prototype, Slide deck, Image, Video,
  Audio.
- Templates ▸ All industries lists "Operations" once: the GitHub and live
  dashboards said "operation", so the list showed "Operation" next to it.
- When the agent crashes mid-turn, the chat shows its error ("Error:
  ENOSPC: no space left on device, write") instead of the last line of
  its stack trace ("at writeFileSync (node:fs:…)") or Node's version
  line. A failed turn has an Agent output disclosure (with a chevron)
  with everything the agent wrote to stderr, next to Retry, and "Failed"
  no longer breaks over two lines.
- A failed turn keeps its error message when the chat is reopened. Before,
  only "See the message above" was left, over nothing.
- PDF, PNG and PowerPoint exports try Chrome or Edge once more when it
  does not start within 30 s, so the first export on a slow or freshly
  set up PC does not fail with "Timeout while resolving websocket URL".
- Picking a `/` command or an `@` mention puts the caret after it at once:
  text typed straight after the pick no longer lands in the wrong place
  (`/search endstea tr` instead of `/search tea trends`).
- Text edited in the preview right after it reloads is saved; it was
  dropped without a word when typed before the reload had finished
  setting up.
- E (or Edit) on a design system that is still loading opens the DESIGN.md
  editor once it has loaded, instead of showing the DESIGN.md tab with no
  editor.

## [0.2.0-beta.6] - 2026-10-29

### Added

- Help ▸ Check for Updates… and Settings ▸ About ▸ Check for updates. On
  that click only, Contour asks GitHub Releases for the newest release,
  pre-releases included and drafts skipped, and shows its version with
  View release (the release page: notes and downloads), "You're up to
  date", or a short error with Try again (its own message when GitHub
  rate limits the network). Nothing is checked or downloaded on its own.
  Keyboard and screen reader friendly; in every UI language.

### Changed

- The Windows upgrade smoke test runs for releases and manual runs, not
  on pull requests. Fetching the previous `.msi` counted as a download, so
  pull requests made up most of the `.msi` download count.
- The chat's Chat mode is called Ask in English, so a phone-width Studio
  no longer shows two "Chat" controls (the Chat/Preview/Files switch and
  the mode tabs).
- Home's example briefs read "e.g. …", so they do not look like an
  instruction.

### Fixed

- OpenCode 2 works. Onboarding's agent test and every run failed with
  "Unrecognized flag: --dir": OpenCode 2 dropped `--dir` and takes the
  project from its working folder. Contour tells the two apart by
  `opencode run --help`: 2.x runs with `--standalone` (a private server,
  so the run's MCP settings apply and no background service is left
  behind) and `--auto` instead of `--dangerously-skip-permissions`; 1.x
  keeps `--dir`; when the help does not answer in time, `opencode
  --version` decides. Upgrading OpenCode while Contour runs takes effect
  on the next run (and on Check again) without restarting the app.
  OpenCode 2's tool calls (no `callID`, `path` for the file) show in the
  chat and count as written files.
- Studio on a phone: a deck hides the thumbnail column, so the slide gets
  the width, and Present wraps under the slide arrows instead of past the
  edge. The slide buttons are 44 px touch targets.
- Onboarding on a phone: the brand URL field takes the whole row; the
  Extract button goes under it.
- Integrations on a phone shows all three tabs ("Connect an agent" is the
  shorter label), and the agent launch command wraps inside its card.
- Integrations ▸ Install says what happened with the client's name and
  what to do next ("Contour is now available in Cursor. Restart it to
  load the server.") instead of "installed contour into cursor", and shows
  that it is working while the agent's settings are updated.
- "Saved to … as vN" shows only on the tab of the file that was saved.
- The two PowerPoint exports download under different names:
  `<file>-images.pptx` and `<file>-editable.pptx`.
- A PDF export of a page without a `<title>` is titled after its file,
  not "index.html".
- Template industries read "HR" and "AI", not "Hr" and "Ai", in the
  filter and on the template's detail badge.
- MCP works again after upgrading from 0.2.0-beta.4 or earlier without
  pressing Install again. Those versions registered
  `<app> mcp --daemon-url http://127.0.0.1:<port>` with a port that is
  dead after the app restarts. When nothing answers at `--daemon-url` or
  `OD_DAEMON_URL` within 1 s, `mcp` now uses the running app's URL from
  `daemon.lock` and says so on stderr. See "MCP does not connect after an
  upgrade" in docs/troubleshooting.md.

## [0.2.0-beta.5] - 2026-10-28

### Added

- Pull requests run the workspace tests on Windows (`test (windows-2025)`
  in `ci.yml`), not only a compile check.
- The Windows smoke test installs the MCP server into Claude Code and
  Codex the way Integrations does, through npm-style `.cmd` shims, and
  checks the command each one gets. It then installs the latest published
  release, makes a project, installs the new `.msi` over it and checks
  there is one uninstall entry with the new version, the app starts and
  the data folder is untouched (`scripts/windows-upgrade-smoke.ps1`).

### Fixed

- Integrations ▸ Install (MCP into Claude Code, Codex) works from the
  desktop app. It registered `<app> mcp`, which opened a second window
  instead of serving MCP; the app now runs the MCP stdio server for `mcp`
  (as `od mcp`). The app's port changes per launch, so it registers no
  `--daemon-url`: `mcp` takes `--daemon-url`, then `OD_DAEMON_URL`, then
  the running app's URL from `daemon.lock`. An AppImage registers its
  `.AppImage` file rather than the mount that goes when it quits. The
  Linux and Windows smoke tests run the registered command.
- Folder import, project locations and linked folders refuse the home
  folder on Windows too (they read `HOME`, which Windows does not set).
- Design system cards show the right colors when a `DESIGN.md` has
  Windows (CRLF) line ends, as every bundled one has in the Windows
  installer. Color tables were skipped, so 11 bundled systems showed other
  swatches than on macOS and Linux.
- On Windows, installing the MCP server into an agent gives it the app's
  plain path (`C:\Program Files\…`), not the `\\?\C:\…` form.

## [0.2.0-beta.4] - 2026-10-28

### Added

- Windows 10 and 11 (x64) are supported platforms. Every release's `.msi`
  is installed silently, opened from its Start menu shortcut, runs an agent
  turn and a PDF export, and is uninstalled on clean Windows Server 2025
  and 2022 before the release is drafted (`windows-smoke.yml`). An
  installer that builds but fails this stops the draft; the Windows build
  itself stays best effort.
- Getting started explains installing Contour on Windows (SmartScreen:
  More info ▸ Run anyway), silent installs, uninstalling and where the logs
  are; troubleshooting covers SmartScreen, a failed install and a blank
  window.

### Fixed

- Windows finds agent CLIs installed with npm (`claude.cmd`) or a native
  installer (`claude.exe`). Detection looked for the bare name, which on
  Windows is npm's `sh` script or nothing. It also looks in
  `%USERPROFILE%\.local\bin` and `%APPDATA%\npm`, and reads `Path` from
  the registry, so an agent installed after signing in to Windows is found
  without signing out; onboarding has Check again on Windows too.
- Integrations ▸ install the Contour MCP server into Claude Code or Codex
  works on Windows: it ran npm's `sh` script (`claude`, "not a valid Win32
  application") or found nothing, instead of `claude.cmd` or `claude.exe`.
- PDF, PNG and PPTX export work on Windows: Edge was handed a page address
  it could not open (`file:////%3F/C:/…`) and the export failed after two
  minutes with "loading the page: Request timed out".

## [0.2.0-beta.3] - 2026-10-27

### Added

- Ubuntu 22.04 LTS and 24.04 LTS (x64) are supported platforms. Every
  release's `.deb` and AppImage are installed and opened on clean Ubuntu
  22.04 and 24.04 before the release is drafted (`linux-smoke.yml`):
  the app starts, an agent runs a turn and PDF export works.
- Getting started explains installing, updating and removing Contour on
  Ubuntu, FUSE for the AppImage, and where the logs are; troubleshooting
  covers a blank window under Wayland.

### Fixed

- The Linux `.deb` and AppImage open the app instead of "UI bundle not
  found".
- Contour is listed under Graphics and Development in the Linux app menu,
  with a proper description, and the `.deb` recommends `xdg-utils`.
- The menu bar has one Help menu, and Project comes before Window.
- The Windows installer (`.msi`) builds for pre-release versions: WiX gets a
  numeric version (`0.2.0-beta.2` → `0.2.0.102`) from `scripts/msi-version.sh`.

## [0.2.0-beta.2] - 2026-10-26

### Changed

- The macOS app needs macOS 13.3 or later: the preview's reload guard relies
  on the browser telling the app's own loads from the page's, which older
  versions of the system web view do not do.

### Fixed

- A plugin run whose agent finished at once (a quick failure, a fast model) no
  longer loses its GenUI questions: they are asked before the agent starts.
- The preview no longer stops with "the page kept reloading itself" when files
  are saved in quick succession (an agent writing many files, fast manual
  edits); a page that really reloads itself is still stopped.
- A page that loads itself again under a new preview address each time is
  stopped like any other reload loop: only loads the app starts get an edit
  key, so the page cannot pass for the app saving fast.
- A plugin run canceled while its GenUI questions were being asked no longer
  leaves them waiting in the chat for a run that never started.
- The Windows build compiles without warnings.

## [0.2.0-beta.1] - 2026-10-26

### Added

- The desktop app writes its log to `<data folder>/logs/daemon.log` (rolled over
  at 5 MB, 5 files kept), so a launch from Finder keeps it; `od daemon start
  --log-file` or `OD_LOG_FILE=1` does the same for the CLI daemon. A panic
  writes its message and stack there before the app quits.
- Help ▸ Report Issue and Export diagnostics include the last 200 lines of that
  log, with secret-looking values redacted wherever they sit (`api_key=…`, a
  `?token=…` in a URL, `Bearer …`). Nothing is sent anywhere. Report Issue now
  opens that log folder instead of the run folder.

### Changed

- Templates show a name and a description written for people ("CRM table (live
  data)" instead of "Crm Table Live"); the agent-facing "Use when the brief asks
  for…" text no longer appears in the UI, and each example has its own
  description. Languages without a translation show the English text.
- The Swiss-print web page and four other templates are filed under the right
  type (a web page was listed as Video, decks as Image).
- Templates are sorted by the name on the card; no two share a name, and the
  sample decks are about made-up companies, not about Contour.
- Plugins say what they do for you; kind, atoms, pipeline stages and
  capabilities moved under Developer details, and the filter chips read
  "Workflows" and "Building blocks".
- Design systems are described by their look (the first paragraph of their
  DESIGN.md) on their page and in Home's picker, instead of the maintainer note
  "Bundled … package for X, derived from curated DESIGN.md…"; in the picker, a
  brand-named system also carries the trademark note.
- Home's template picker is sorted by the name shown.
- The example briefs of the Code migration and Media generation plugins read as
  a user would write them.
- Brand-named design systems show "Inspired by <Brand>'s public design language.
  Not affiliated with or endorsed by <Brand>. Trademarks belong to their owners."
  on their page, in every UI language.
- The README opens with what Contour is (no telemetry, runs on your machine, uses
  the agent CLI you have), a recorded demo, and how it differs from OpenDesign.
- Home keeps one composer with one row of controls: the "Recommended for you"
  strip is gone, and New design system, Create plugin and From folder moved into
  the "+" menu. New design system opens the same dialog as the Design systems
  page, with every source.
- The Studio names the project at the top of the chat, with the conversation
  under it; the files panel is headed "Files".
- The Export menu folds the image formats under Image and the two PowerPoint
  kinds under PowerPoint, lists only the formats the open file exports to, and
  says "MP4 video".
- Settings saves Permissions, Appearance and Language as soon as they change;
  the agent card ("Save agent") and the API provider form ("Save and use") each
  have their own Save, never both at once.
- Messages are named by what was asked, with the state as a badge, and the page
  is called Messages everywhere. Each run keeps the message it answered
  (`prompt` in `GET /api/runs` and `GET /api/runs/:id`).
- The chat says what the agent did ("Updated plan", "Wrote", "1 new") instead of
  tool names such as `TodoWrite`.
- New routines start on "New project each run", and the form goes back to it
  after an edit is saved or cancelled.
- "+" on Home names its two folder items by what they do: "Open a folder as a
  project" and "Let the agent read a folder".
- The run status says "1 file written" / "N files written".
- Media and Templates say "Audio"; Media ▸ Generate has one way in, real selects
  and a Set up providers button.
- Segmented controls, tab lists and radio chips move with the arrow keys and are
  one Tab stop each.
- Onboarding: when an installed agent fails its test, the reason and "<Agent> did
  not pass its test. Update it, or install one of these instead." sit right under
  it, on the first screen; "Still no agent found" names both `claude --version`
  and `codex --version`.

### Fixed

- `/api/runs` keeps only the first line of each run's prompt (up to 200
  characters) instead of the whole message, which the run list polled every
  few seconds; runs saved before this are cut the same way when loaded.
- The "deliverable checked" log line compares whole paths: a run that writes
  `old/index.html` next to the entry `index.html` counts as another page.
- Renaming a project to a blank name keeps the old name and says a name is
  required; failed renames, duplicates and deletes now show their error (QA-001).
- The API provider form no longer saves a provider that needs a key without one;
  it says "API key required" and Continue stays off (QA-005).
- Esc closes the drop-down menus: Export, Screenshot, Present, the project menu and
  the files panel menus (QA-004).
- Leaving a Markdown file right after typing no longer panics (QA-009).
- Cancel in the delete-conversation dialog no longer throws a JS error (QA-010).
- Tablet and Phone previews lay the page out at 820 and 390 px and scale it down when
  the pane is narrower, so device media queries apply (QA-003).
- Edit panel: an Apply in one tab, or typing while it saves, no longer wipes what
  was typed and not applied in another field (QA-006).
- With an edit panel change not applied, closing or reloading the page asks first,
  and so does a link that leaves the page (QA-007).
- A message sent right after opening a project no longer vanishes from the chat
  when the stored transcript arrives (QA-002).
- Drafts are kept: the Studio composer per project and conversation, and Home's brief
  and design system, across reloads and visits until sent (QA-008).
- A run that writes another page (a deck next to `index.html`) opens it in a tab
  instead of warning that the entry file was not updated.
- The "Media library" project that holds uploads no longer shows in Projects,
  Recent or the Automations project list.
- Onboarding's brand extraction runs the agent the user picked and tested, not
  the first one found; "Not logged in" and "not signed in" count as a missing
  sign-in.
- A link to a project that does not exist says "This project was deleted or the
  link is wrong." and offers only All projects: no Retry, tabs, Export, files or
  pickers, and the composer is off. Unknown addresses get a "Page not found"
  page with Go to Home.
- An empty template search offers Clear filters; Design systems search matches
  the description people read, not the package note; the Skills tab drops the
  "Use when…" text meant for the agent.
- The "first design is ready" hint closes when the next message is sent.
- At 390 px, Preview closes the side chat and New project leads the Projects
  header; comment chips read "h1: …"; the command palette no longer matches
  scattered letters ("set" found "Quarterly marketing site").

## [0.1.0] - 2026-10-24

First release: a Rust rewrite of [OpenDesign](https://github.com/nexu-io/open-design)
that runs entirely on your machine, at feature parity with the original app
(207 of 207 scored rows, [docs/parity.md](docs/parity.md)).

### Added

- **Daemon and CLI** (`od`): HTTP + SSE + WebSocket API on `127.0.0.1:7456`,
  one daemon per data folder, thin CLI clients for projects, files, runs,
  lint, export, media, plugins and MCP.
- **Desktop app** (Tauri 2): the daemon runs inside the app on a random
  loopback port; native folder picker, menus, login-shell `PATH` for agents.
- **Agents**: 6 coding-agent CLIs (Claude Code, Codex, Cursor Agent, Copilot,
  OpenCode, Qwen Code) and 10 ACP agents, detected on `PATH`; auto or ask
  permission mode; BYOK with 26 provider presets (Anthropic, OpenAI-compatible,
  Azure OpenAI and more).
- **Studio**: chat with the agent, live preview with comments, inspect and
  direct edits written back to the source, file versions, tweaks, decks,
  code view with highlighting, terminal tabs (xterm.js, bundled).
- **Content**: 169 skills, 115 design templates, 152 design systems, craft
  guides and plugins copied from OpenDesign with their licences (`NOTICE`).
- **Export**: self-contained HTML, ZIP, PDF (raster and vector), PNG/JPEG,
  PPTX (image and editable), MP4, React component as HTML; uses the Chrome,
  Edge or Chromium already installed.
- **Automations**: routines on a local cron schedule, never overlapping.
- **MCP**: Contour as an MCP server for other agents (`od mcp install`) and
  as an MCP client for stdio/HTTP servers (OAuth 2.1 PKCE).
- **Media**: image, video and audio generation with your own keys.
- **UI** in 20 languages, light and dark themes, keyboard shortcuts and a
  command palette.
- **Data folder migration**: the first run moves an existing
  `open-design-rs` data folder to `contour`, leaving a link at the old path
  on macOS and Linux.
- **Project tooling**: `make help` targets for setup, dev, test, e2e, lint,
  bundles, install and versioning; GitHub Actions for CI and releases;
  cargo-deny policy; docs for users and contributors.

### Fixed

- The hidden ZIP input on the Projects page has an accessible name (axe `label`).

[Unreleased]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.32...HEAD
[0.2.0-beta.32]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.31...v0.2.0-beta.32
[0.2.0-beta.31]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.30...v0.2.0-beta.31
[0.2.0-beta.30]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.29...v0.2.0-beta.30
[0.2.0-beta.29]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.28...v0.2.0-beta.29
[0.2.0-beta.28]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.27...v0.2.0-beta.28
[0.2.0-beta.27]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.26...v0.2.0-beta.27
[0.2.0-beta.26]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.25...v0.2.0-beta.26
[0.2.0-beta.25]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.24...v0.2.0-beta.25
[0.2.0-beta.24]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.23...v0.2.0-beta.24
[0.2.0-beta.23]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.22...v0.2.0-beta.23
[0.2.0-beta.22]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.21...v0.2.0-beta.22
[0.2.0-beta.21]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.20...v0.2.0-beta.21
[0.2.0-beta.20]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.19...v0.2.0-beta.20
[0.2.0-beta.19]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.18...v0.2.0-beta.19
[0.2.0-beta.18]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.17...v0.2.0-beta.18
[0.2.0-beta.17]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.16...v0.2.0-beta.17
[0.2.0-beta.16]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.15...v0.2.0-beta.16
[0.2.0-beta.15]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.14...v0.2.0-beta.15
[0.2.0-beta.14]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.13...v0.2.0-beta.14
[0.2.0-beta.13]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.12...v0.2.0-beta.13
[0.2.0-beta.12]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.11...v0.2.0-beta.12
[0.2.0-beta.11]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.10...v0.2.0-beta.11
[0.2.0-beta.10]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.9...v0.2.0-beta.10
[0.2.0-beta.9]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.8...v0.2.0-beta.9
[0.2.0-beta.8]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.7...v0.2.0-beta.8
[0.2.0-beta.7]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.6...v0.2.0-beta.7
[0.2.0-beta.6]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.5...v0.2.0-beta.6
[0.2.0-beta.5]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.4...v0.2.0-beta.5
[0.2.0-beta.4]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.3...v0.2.0-beta.4
[0.2.0-beta.3]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.2...v0.2.0-beta.3
[0.2.0-beta.2]: https://github.com/quangphung218/contour-design/compare/v0.2.0-beta.1...v0.2.0-beta.2
[0.2.0-beta.1]: https://github.com/quangphung218/contour-design/compare/v0.1.0...v0.2.0-beta.1
[0.1.0]: https://github.com/quangphung218/contour-design/releases/tag/v0.1.0
