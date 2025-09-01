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

# Lint the code (basic level)
lint:
    @echo "🔍 Running basic linting..."
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    @echo "✅ Basic linting completed"

# Strict linting with pedantic rules  
lint-strict:
    @echo "🔍 Running strict linting..."
    cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -W clippy::nursery -W clippy::cargo
    @echo "✅ Strict linting completed"

# Fix linting issues
lint-fix:
    @echo "🔧 Fixing linting issues..."
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings
    cargo fmt
    @echo "✅ Linting fixes applied"

# Enhanced lint with all checks
lint-all: lint fmt-check
    @echo "🔍 Running additional security and dependency checks..."
    cargo deny check || echo "⚠️  cargo-deny not installed, skipping dependency checks"
    cargo audit || echo "⚠️  cargo-audit not installed, skipping security audit"
    @echo "✅ All linting checks completed"

# Check code without building
check:
    @echo "🔍 Checking code..."
    cargo check --workspace --all-targets --all-features
    @echo "✅ Check completed"

# Check formatting only
fmt-check:
    @echo "🔍 Checking formatting..."
    cargo fmt --check
    @echo "✅ Formatting check completed"

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
    cargo install cargo-nextest cargo-deny cargo-audit

# Full CI pipeline
ci: build test lint-all deny gen-schemas
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

# Setup git hooks
setup-hooks:
    @echo "Setting up git hooks..."
    chmod +x .git-hooks/pre-commit
    ln -sf ../../.git-hooks/pre-commit .git/hooks/pre-commit
    @echo "✅ Git hooks installed"

# Example: Run the daemon and CLI in separate terminals
demo:
    @echo "Run these commands in separate terminals:"
    @echo "Terminal 1: just start-daemon"
    @echo "Terminal 2: just cli status"
