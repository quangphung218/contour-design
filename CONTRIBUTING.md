# Contributing to Contour

Thanks for helping. This page covers how to get a working checkout, what to
run before a pull request, how to write commit messages and how review works.
The developer guide ([docs/development.md](docs/development.md)) has the
details on the toolchain, crate layout and debugging.

## Getting set up

```bash
git clone https://github.com/quangphung218/contour-design.git
cd contour-design
make setup      # checks the toolchain, prints the command for anything missing
make run        # daemon + UI at http://127.0.0.1:7456
```

`make help` lists every target.

## Before you open a pull request

| Run | When | What it does |
|---|---|---|
| `make check` | always | `cargo fmt --check`, clippy with `-D warnings`, all workspace tests |
| `make lint` | you touched `crates/od-ui` | adds clippy for the `wasm32` UI build |
| `make e2e` | you touched the UI or an API the UI calls | Playwright against a real daemon and a mock agent |
| `make deny` | you added or bumped a dependency | licence and advisory policy (`deny.toml`) |
| `make links` | you edited Markdown | every relative link and heading anchor resolves |
| `make ci` | before asking for review | all of the above plus actionlint and commitlint, as CI runs them; the macOS gate (CI on pull requests is Linux only) |

Also:

- Add a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for
  anything a user would notice.
- Update the README or `docs/` when a command, flag or behaviour changes.
- Rust code follows the `rust-skills` guidelines in
  `.claude/skills/rust-skills` and the workspace lints in `Cargo.toml`
  (`unsafe_code = "forbid"`, no `unwrap()` outside tests).
- Behaviour is specified in [docs/spec.md](docs/spec.md); settled questions
  are in [docs/decisions.md](docs/decisions.md). Propose a change to a
  decision in the pull request description rather than working around it.
- Nothing may reach the network except the agent or provider the user
  configured. The e2e suite blocks every request that leaves loopback.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
in the Angular style. CI checks every commit of a pull request with
commitlint ([commitlint.config.mjs](commitlint.config.mjs)); run
`make commitlint` to check your branch against `main` locally.

```
type(scope): subject

Optional body: what changed and why, wrapped at 100 columns.

Optional footer: Fixes #123, BREAKING CHANGE: …
```

- **type**: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`,
  `ci`, `style`, `chore`, `revert`.
- **scope** (required): a crate directory name (`od-ui`, `od-server`,
  `od-store`, …), `desktop` for `apps/desktop`, or one of `brand`, `e2e`,
  `ci`, `docs`, `build`, `release`, `deps`.
- **subject**: English, imperative, starts lowercase, no full stop, and the
  whole first line fits in 72 characters.
- One logical change per commit. Split a branch into commits by kind
  (build, ci, docs, fix) so each can be reviewed and reverted on its own.

Examples:

```
fix(od-ui): give the hidden zip input an accessible name
feat(od-export): export a deck as editable pptx
build(release): add make targets for bundles and versioning
docs(docs): translate the spec to english
```

## Review

- Every change lands through a pull request into `main`; nobody pushes to
  `main` directly.
- CI must be green: lint, Linux tests, e2e, cargo-deny, docs links and
  commit messages. CI on pull requests runs on Linux only; macOS and
  Windows run at release time or on a manual run, so **`make ci` on macOS
  is the main gate** there: say in the pull request that it passed.
- A reviewer reads the diff against the spec and decisions, runs it when the
  change is visible, and asks for tests for every new guard or bug fix.
- Authors answer every comment, either with a change or with a reason.
  Resolve threads only once the reviewer agrees.

## Licence

By contributing you agree that your work is released under the
[Apache License 2.0](LICENSE), the licence of this project and of the
original OpenDesign app. Content copied from elsewhere must keep its licence
and be listed in [NOTICE](NOTICE).
