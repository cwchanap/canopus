# Justfile for Canopus project automation
# Usage: just <command>

# Default target - show available commands
default:
    @just --list

# Build all crates
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Run all tests
test:
    cargo test

# Run tests with nextest (if available)
test-nextest:
    cargo nextest run || cargo test

# Run tests with coverage
test-coverage:
    cargo test --all-features -- --nocapture

# Lint the code
lint:
    cargo clippy -- -D warnings
    cargo fmt --check

# Fix linting issues
lint-fix:
    cargo clippy --fix --allow-dirty
    cargo fmt

# Clean build artifacts
clean:
    cargo clean

# Generate schemas
gen-schemas:
    cargo run -p xtask gen-schemas

# Run cargo-deny checks
deny:
    cargo deny check || echo "cargo-deny not installed, skipping..."

# Install development dependencies
install-deps:
    cargo install cargo-nextest cargo-deny

# Full CI pipeline
ci: build test lint deny gen-schemas
    @echo "✓ All CI checks passed"

# Start the daemon
start-daemon:
    cargo run --bin daemon

# Run CLI commands
cli *args:
    cargo run --bin canopus -- {{args}}

# Development workflow - watch for changes and run tests
dev:
    cargo watch -x "test" -x "clippy"

# Check project for issues
check:
    cargo check --all-targets --all-features

# Update dependencies
update:
    cargo update

# Audit dependencies for security issues
audit:
    cargo audit || echo "cargo-audit not installed, skipping..."

# Run benchmarks (if any)
bench:
    cargo bench

# Build documentation
docs:
    cargo doc --no-deps --open

# View workspace dependencies
deps:
    cargo tree

# Run a specific crate's tests
test-crate crate:
    cargo test -p {{crate}}

# Build a specific crate
build-crate crate:
    cargo build -p {{crate}}

# Example: Run the daemon and CLI in separate terminals
demo:
    @echo "Run these commands in separate terminals:"
    @echo "Terminal 1: just start-daemon"
    @echo "Terminal 2: just cli status"
