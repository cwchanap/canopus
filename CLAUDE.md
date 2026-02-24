# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Architecture

Canopus is a **multi-crate Rust workspace** implementing a daemon-client architecture with comprehensive process management, health monitoring, and service supervision:

### Crate Structure
- **`core/`** (`canopus-core`): Shared utilities, error handling (`CoreError`), Unix process management, service supervision, health checking, persistence, and port management
- **`schema/`**: JSON Schema-enabled data structures for communication protocol (`Message`, `Response`, `ServiceSpec`, etc.)
- **`ipc/`**: TCP-based async communication between daemon and CLI; also UDS JSON-RPC for service management
- **`daemon/`**: Background server handling client connections and service management
- **`cli/`**: Command-line interface with subcommands (`status`, `start`, `stop`, `restart`, `services`, `custom`)
- **`e2e-tests/`**: End-to-end integration tests (run separately; known to be flaky with 45s timeouts)
- **`xtask/`**: Development automation for JSON schema and TypeScript definition generation

> Note: The core crate is named `canopus-core` in `Cargo.toml` to avoid conflicts with Rust's built-in `core`.

### Two IPC Planes
- **TCP control plane** (`127.0.0.1:49384`): CLI ↔ daemon; handles daemon lifecycle (`start`, `stop`, `status`, etc.)
- **UDS JSON-RPC** (`/tmp/canopus.sock`, override via `CANOPUS_IPC_SOCKET`): CLI `services` subcommand ↔ daemon; handles per-service operations

### Persistence
- **SQLite DB** at `~/.canopus/canopus.db`: stores service ID, name, state, PID, port, hostname
- **File-based registry**: atomic writes and crash recovery in `core::persistence`

## Development Commands

```bash
# Build
just build
cargo build

# Run all tests (nextest preferred for per-test timeouts)
just test                         # tries nextest, falls back to cargo test
just test-ci                      # CI profile with longer timeout
cargo test --workspace --lib      # reliable unit-only run

# Test a specific crate
cargo test -p canopus-core
cargo nextest run -p canopus-core

# Run a single test by name
cargo test -p canopus-core -- test_name
cargo nextest run -p canopus-core -E 'test(test_name)'

# Linting
just lint                         # basic clippy -D warnings
just lint-strict                  # pedantic rules
just lint-fix                     # auto-fix + fmt
just fmt-check

# Full CI pipeline
just ci                           # build + lint + deny + gen-schemas + test

# Schema generation
just gen-schemas                  # JSON schemas + TypeScript definitions

# Security
just deny                         # cargo-deny: licenses, advisories, duplicates
just audit                        # cargo-audit vulnerability scan

# Install dev tools
just install-deps                 # cargo-nextest, cargo-deny, cargo-audit

# Run the system
just start-daemon                 # cargo run --bin daemon
just cli status                   # cargo run --bin canopus -- status
just cli services list
```

## Code Architecture and Patterns

### Error Handling
- Each crate defines its own error enum with categorical codes (`CORE001`, `DAEMON_ALREADY_RUNNING`)
- `pub type Result<T> = std::result::Result<T, CrateError>;` in each crate
- Use `#[from]` and manual `From` implementations for cross-crate conversions

### Process Management (`core::process::unix`)
- All processes spawned in separate process groups via `unsafe { libc::setsid() }`
- SIGTERM → SIGKILL escalation with configurable timeouts
- Robust handling of race condition error codes (ESRCH, EPERM)
- `unsafe_code = "forbid"` at workspace level; modules use `#![allow(unsafe_code)]` with mandatory safety comments

### Service Supervision (`core::supervisor`)
- `ServiceSupervisor` is the state machine managing service lifecycle
- Restart policies: `Always`, `OnFailure`, `Never` with exponential backoff
- Health probes: HTTP and TCP with readiness/liveness checks
- Log management: bounded ring buffer for stdout/stderr

### Schema Types
- All types in `schema/` derive `JsonSchema`, `Serialize`, `Deserialize`
- `#[serde(rename_all = "camelCase")]` for external API consistency
- Use `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields

### Communication Protocol
- JSON over TCP (daemon ↔ CLI); JSON-RPC over UDS (services subcommand ↔ daemon)
- `Message`/`Response` types; `Arc`-wrapped state cloned for `tokio::spawn` tasks

## Configuration

Two distinct config formats:

**Service Spec TOML** (`daemon --config`): full service definitions with commands, health checks, restart policies. Supports both array-of-tables (`[[services]]`) and keyed-by-id shorthand formats. See `examples/services.toml`.

**Runtime TOML** (`cli --config` or `daemon --runtime-config`): lightweight per-service hostname/port overrides. See `examples/runtime.toml`.

```bash
# Daemon with service spec
canopus-daemon --config services.toml

# CLI to sync running services with runtime config (starts idle, stops unlisted)
canopus services start --config runtime.toml

# Daemon with both
canopus-daemon --config services.toml --runtime-config runtime.toml
```

## Testing Conventions

- **Unit tests**: In-file with `#[cfg(test)] mod tests`
- **Integration tests**: Separate files in `tests/` directories
- **E2E tests**: In `e2e-tests/` crate; run separately; known to be flaky with long timeouts
- **Platform-specific**: `#[cfg(unix)]` / `#[cfg(windows)]`
- **Async tests**: `#[tokio::test]`

## Critical Files

- **`Cargo.toml`**: Workspace configuration with lint rules and shared dependencies
- **`justfile`**: All development commands and CI automation
- **`schema/src/lib.rs`**: Core data structures for the communication protocol
- **`core/src/error.rs`**: Error handling patterns
- **`core/src/supervisor/service_task.rs`**: Service supervision state machine
- **`core/src/process/unix.rs`**: Unix process management (safety-critical)
- **`core/src/proxy/mod.rs`**: Reverse proxy adapter
- **`core/src/persistence.rs`**: SQLite-backed registry with atomic operations
- **`core/src/port.rs`**: Dynamic port allocation
- **`docs/config.md`**: Full configuration reference
