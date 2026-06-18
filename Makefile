.PHONY: all build clean test check fmt lint doc publish-dry release

LEVEL ?= minor

# Default target
all: check build test

# Build the library
build:
	cargo build

# Clean build artifacts
clean:
	cargo clean

# Run tests
test:
	cargo test

# Type-check + clippy
check:
	cargo check
	cargo clippy --all-targets -- -D warnings

# Format code
fmt:
	cargo fmt

# Lint (check formatting + clippy)
lint:
	cargo fmt -- --check
	cargo clippy --all-targets -- -D warnings

# Build API docs
doc:
	cargo doc --no-deps --open

# Verify the crate would publish cleanly without uploading
publish-dry:
	cargo publish --dry-run

# Bump version, finalize CHANGELOG.md, tag, publish to crates.io, and push
# (requires cargo-release)
release:
	cargo release $(LEVEL) --execute --no-confirm
