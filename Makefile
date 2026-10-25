# Contour build, test and release targets. `make help` lists them.

# One target dir per worktree. The shared target-dir in .cargo/config.toml is
# not safe across worktrees: workspace (path) crates get the same artifact
# hash in every worktree, so cargo reuses another worktree's build of e.g.
# od-contracts as fresh and compiles against stale types. Set
# CARGO_TARGET_DIR to override.
export CARGO_TARGET_DIR ?= $(HOME)/.cache/odrs-target-$(notdir $(CURDIR))
# Incremental artifacts double the target dir and buy little for full builds;
# four jobs keep a 16 GB machine responsive. Both can be overridden.
export CARGO_INCREMENTAL ?= 0
export CARGO_BUILD_JOBS ?= 4

# Tauri CLI, pinned. `cargo tauri` (cargo install tauri-cli) is used when present.
TAURI_CLI_VERSION := 2.12.4
TAURI := $(shell command -v cargo-tauri >/dev/null 2>&1 && echo cargo tauri || echo npx --yes @tauri-apps/cli@$(TAURI_CLI_VERSION))
WASM_TARGET := wasm32-unknown-unknown
UI := crates/od-ui
APP_NAME := Contour
BUNDLE_DIR = $(CARGO_TARGET_DIR)/$(1)/bundle

.DEFAULT_GOAL := help
.PHONY: help setup dev run check test e2e e2e-qa lint fmt clippy clippy-wasm deny ui-dist ui-release \
	app-debug app-release dmg install clean version ci links commitlint test-scripts

help: ## Show this help
	@awk 'BEGIN {FS = ":.*## "; printf "Usage: make <target>\n\n"} /^[a-zA-Z0-9_-]+:.*## / {printf "  \033[1m%-12s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\nTarget dir: %s\n" "$(CARGO_TARGET_DIR)"

setup: ## Check the toolchain; print the command for anything missing
	@scripts/setup.sh

dev: ui-dist ## Daemon on :7456 + `trunk serve` with hot reload on :8080
	@scripts/dev.sh

run: ui-dist ## Build and run the daemon, UI at http://127.0.0.1:7456
	cargo run -p od-cli --bin od

check: ui-dist ## fmt --check + clippy -D warnings + test (the pre-commit gate)
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace
	@$(MAKE) --no-print-directory test-scripts

test: ui-dist test-scripts ## Run the workspace tests and the script tests
	cargo test --workspace

test-scripts: ## Tests for the release scripts (scripts/tests)
	@for t in scripts/tests/*_test.sh; do $$t || exit 1; done

e2e: ## Playwright e2e against a real daemon and a mock agent
	$(UI)/e2e/run.sh

# Not part of `ci` or `e2e`: bugs it finds stay red on purpose (docs/qa).
e2e-qa: ## QA business-flow e2e (docs/qa/test-plan.md); pass / known bug / fail
	$(UI)/e2e/qa/run.sh $(F)

lint: fmt clippy clippy-wasm ## fmt --check, clippy native and wasm

fmt:
	cargo fmt --all --check

clippy: ui-dist
	cargo clippy --workspace --all-targets -- -D warnings

clippy-wasm:
	cargo clippy -p od-ui --target $(WASM_TARGET) -- -D warnings

deny: ## License and advisory check (cargo-deny, deny.toml)
	cargo deny check licenses advisories bans sources

links: ## Check that relative links in the docs resolve
	@scripts/check-links.sh

commitlint: ## Check commit messages since main (Conventional Commits)
	@scripts/commitlint.sh main HEAD

# apps/desktop bundles crates/od-ui/dist (gitignored); build it once if missing.
ui-dist:
	@test -f $(UI)/dist/index.html || (cd $(UI) && trunk build)

# Release bundles always rebuild the UI optimised (~12 MB WASM, not ~76 MB).
ui-release:
	cd $(UI) && trunk build --release

app-debug: ## Debug desktop bundle (Contour.app) with devtools
	cd $(UI) && trunk build
	cd apps/desktop && $(TAURI) build --debug --bundles app
	@echo "Built: $(call BUNDLE_DIR,debug)/macos/$(APP_NAME).app"

app-release: ui-release ## Release desktop bundle (Contour.app, optimised UI)
	cd apps/desktop && $(TAURI) build --bundles app
	@du -sh "$(call BUNDLE_DIR,release)/macos/$(APP_NAME).app"

# CI=true makes Tauri skip the Finder window styling, which needs Apple
# Events permission and fails in headless or sandboxed terminals.
dmg: ui-release ## Release .dmg (macOS)
	cd apps/desktop && CI=true $(TAURI) build --bundles dmg
	@ls -lh "$(call BUNDLE_DIR,release)"/dmg/*.dmg

install: ## Copy the release Contour.app into /Applications (replaces only Contour.app)
	@scripts/install-app.sh "$(call BUNDLE_DIR,release)/macos/$(APP_NAME).app"

clean: ## Remove the target dir, the UI bundle and e2e results
	cargo clean
	rm -rf $(UI)/dist $(UI)/e2e/test-results

version: ## Set the version everywhere: make version V=x.y.z
	@test -n "$(V)" || { echo "usage: make version V=x.y.z" >&2; exit 2; }
	@scripts/version.sh "$(V)"

ci: ## Run the CI steps locally (lint, test, deny, links, actionlint, commitlint, e2e)
	$(MAKE) lint
	$(MAKE) test
	$(MAKE) deny
	$(MAKE) links
	actionlint
	$(MAKE) commitlint
	$(MAKE) e2e
