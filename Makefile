.PHONY: check ui-dist

# One target dir per worktree. The shared target-dir in .cargo/config.toml is
# not safe across worktrees: workspace (path) crates get the same artifact
# hash in every worktree, so cargo reuses another worktree's build of e.g.
# od-contracts as fresh and compiles against stale types. Set
# CARGO_TARGET_DIR to override.
export CARGO_TARGET_DIR ?= $(HOME)/.cache/odrs-target-$(notdir $(CURDIR))

check: ui-dist
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

# apps/desktop bundles crates/od-ui/dist (gitignored); build it once if missing.
ui-dist:
	@test -f crates/od-ui/dist/index.html || (cd crates/od-ui && trunk build)
