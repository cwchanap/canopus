# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Architecture

Canopus is a **multi-crate Rust workspace** implementing a daemon-client architecture with comprehensive process management, health monitoring, and service supervision:

### Crate Structure
- **`core/`**: Shared utilities, error handling (`CoreError`), Unix process management, service supervision, and health checking
- **`schema/`**: JSON Schema-enabled data structures for communication protocol (`Message`, `Response`, `ServiceSpec`, etc.)
- **`ipc/`**: TCP-based async communication between daemon and CLI
- **`daemon/`**: Background server handling client connections and service management
- **`cli/`**: Command-line interface with subcommands (`status`, `start`, `stop`, `restart`, `custom`)
- **`xtask/`**: Development automation for JSON schema and TypeScript definition generation

### Key Features
- **Service Supervision**: Full lifecycle management with restart policies and health monitoring
- **Unix Process Management**: Process group isolation via `setsid()`, graceful termination with SIGTERM→SIGKILL escalation
- **Health Monitoring**: HTTP and TCP health probes with configurable timeouts and failure thresholds
- **Schema-driven Communication**: JSON-over-TCP protocol with TypeScript definition generation

## Development Commands

### Essential Build Commands
```bash
# Build all crates
just build
cargo build

# Run all tests (prefer nextest if available)
just test
just test-nextest
cargo test

# Test specific crate
just test-crate core
cargo test -p core
```

### Linting and Code Quality
```bash
# Basic linting
just lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Strict linting with pedantic rules
just lint-strict

# Fix linting issues automatically
just lint-fix

# Format check
just fmt-check
cargo fmt --check

# Complete CI pipeline
just ci
```

### Schema Generation
```bash
# Generate JSON schemas and TypeScript definitions
just gen-schemas
cargo run -p xtask gen-schemas
```

### Security and Dependencies
```bash
# Cargo-deny checks (licenses, advisories, duplicates)
just deny
cargo deny check

# Security audit
just audit
cargo audit
```

### Running the System
```bash
# Start daemon
just start-daemon
cargo run --bin daemon

# Use CLI
just cli status
just cli custom "hello world"
cargo run --bin canopus -- status
```

## Code Architecture and Patterns

### Error Handling Strategy
- **Crate-specific errors**: Each crate defines error enum with categorical codes (`CORE001`, `DAEMON_ALREADY_RUNNING`)
- **Result aliases**: `pub type Result<T> = std::result::Result<T, CrateError>;`
- **Error conversion**: Use `#[from]` and manual `From` implementations between crate error types

### Process Management (`core::process::unix`)
- **Process isolation**: All processes spawned in separate process groups using `unsafe { libc::setsid() }`
- **Graceful termination**: SIGTERM followed by SIGKILL with configurable timeouts
- **Race condition handling**: Robust error handling for ESRCH, EPERM edge cases
- **Safety documentation**: All unsafe blocks include safety comments and justification

### Service Supervision (`core::supervisor`)
- **State machine**: `ServiceSupervisor` manages service lifecycle through state transitions
- **Restart policies**: Configurable restart behavior (Always, OnFailure, Never) with exponential backoff
- **Health monitoring**: HTTP/TCP probes with readiness and liveness checks
- **Log management**: Bounded ring buffer for capturing service stdout/stderr

### Communication Protocol
- **JSON over TCP**: All daemon-client communication uses structured `Message`/`Response` types
- **Schema validation**: All types derive `JsonSchema`, `Serialize`, `Deserialize` with camelCase JSON
- **Async patterns**: Use `tokio::spawn` for concurrent connection handling

### Workspace Configuration
- **Dependency management**: Shared dependencies in root `Cargo.toml` under `[workspace.dependencies]`
- **Lint configuration**: Extensive workspace lints with `unsafe_code = "forbid"` (overridden in process modules)
- **Platform-specific code**: Unix process dependencies gated behind `cfg(unix)`

## Testing Conventions

- **Unit tests**: In-file with `#[cfg(test)] mod tests`
- **Integration tests**: Separate files in `tests/` directories
- **Platform-specific**: Use `#[cfg(unix)]`/`#[cfg(windows)]` for OS-specific tests
- **Async tests**: `#[tokio::test]` for async test functions
- **Process testing**: Comprehensive signal handling, cleanup verification, and race condition testing

## Current Development Status

### Completed Features (Phase 2 - Process Control)
- **Unix Process Management**: Full implementation with process group management and signal handling
- **Service Supervision**: Complete state machine with restart policies and health monitoring
- **Health Monitoring**: HTTP and TCP health probes with configurable parameters
- **Schema Generation**: JSON schemas and TypeScript definitions via xtask
- **Reverse Proxy**: Service routing with attach/detach lifecycle management
- **File-based Registry**: Persistence with atomic writes and crash recovery
- **TOML Configuration**: Loading and validation for services

### Key Implementation Details
- **Unsafe code**: Limited to essential system calls in `core::process::unix` with proper safety documentation
- **Error codes**: Systematic error code assignment (CORE009-CORE011 for process management)
- **Testing coverage**: 49 total tests across unit, integration, and documentation tests
- **Service Task**: Complete state machine in `ServiceSupervisor` with lifecycle management
- **Port Management**: Dynamic port allocation and management system

## Critical Files to Understand

- **`Cargo.toml`**: Workspace configuration with extensive lint rules and shared dependencies
- **`justfile`**: Comprehensive development commands and CI pipeline automation
- **`schema/src/lib.rs`**: Core data structures driving the entire communication protocol
- **`core/src/error.rs`**: Error handling patterns used throughout the codebase
- **`core/src/supervisor/service_task.rs`**: Service supervision state machine implementation
- **`core/src/process/unix.rs`**: Unix process management with safety-critical code
- **`core/src/proxy/mod.rs`**: Reverse proxy adapter for service routing
- **`core/src/persistence.rs`**: File-based registry with atomic operations
- **`core/src/port.rs`**: Port allocation and management system
- **`deny.toml`**: Security and license policy configuration
- **`.github/copilot-instructions.md`**: Additional AI coding agent guidance and project status