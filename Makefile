.PHONY: check ui-dist
check: ui-dist
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

# apps/desktop bundles crates/od-ui/dist (gitignored); build it once if missing.
ui-dist:
	@test -f crates/od-ui/dist/index.html || (cd crates/od-ui && trunk build)
