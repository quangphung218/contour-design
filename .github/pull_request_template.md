<!--
Title: a Conventional Commit, checked by commitlint (CONTRIBUTING.md), e.g.
"fix(od-export): wait for entrance animations before a PDF". Header ≤ 72 characters.
The first line of the body below is a plain sentence a user would understand; it can go into
the release notes as it is.
-->

<!-- One sentence: what a person using Contour sees change. "Fixes #123" if there is an issue. -->

## What changed

<!-- For the user first, then anything a reviewer needs (why this approach, what it replaces). -->

## Verified

<!-- What you ran and what it printed (`make quick`: 1,234 passed), what you tried in the real app
(which build, which OS, which agent), screenshots for UI changes. -->

## Not verified

<!-- What you could not try (Windows, Linux, a real agent, a released build), so STATUS.md can say so.
"Nothing" is an answer only when it is true. -->

## Checklist

- [ ] `make quick` passes, and `make check` for Rust changes (fmt, clippy `-D warnings`, tests)
- [ ] The e2e specs this change touches pass locally (`crates/od-ui/e2e/run.sh <name>.e2e.mjs`); CI runs the rest
- [ ] `CHANGELOG.md` has a line under `[Unreleased]` for a user-visible change
- [ ] `scripts/commitlint.sh origin/main HEAD` passes
- [ ] Invariants touched are listed below, from each changed crate's `CLAUDE.md`, with the guard tests run
- [ ] Docs updated (README, `docs/`, [docs/STATUS.md](../docs/STATUS.md)) when behaviour, commands or what works changed

**Invariants touched:** <!-- e.g. "od-store 1, never write through a link: ran src/files.rs::writing_through_a_link_is_refused", or "none" -->
