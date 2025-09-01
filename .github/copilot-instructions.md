# Canopus AI Coding Agent Instructions

## Project Architecture

Canopus is a **multi-crate Rust workspace** implementing a daemon-client architecture with comprehensive error handling and schema generation:

- **`core/`**: Shared utilities, error types (`CoreError`), configuration validation, and **Unix process management**
- **`schema/`**: JSON Schema-enabled data structures (`Message`, `Response`, `DaemonConfig`, etc.)
- **`ipc/`**: TCP-based async communication between daemon and CLI
- **`daemon/`**: Background server handling client connections via TCP
- **`cli/`**: Command-line interface with subcommands (`status`, `start`, `stop`, `restart`, `custom`)
- **`xtask/`**: Development automation for JSON schema and TypeScript definition generation

## Key Patterns & Conventions

### Error Handling Strategy
- **Crate-specific error types**: Each crate defines its own error enum (e.g., `CoreError`, `IpcError`, `CliError`)
- **Error codes**: All errors include categorical codes like `"CORE001"`, `"DAEMON_ALREADY_RUNNING"`
- **Result type aliases**: Use `pub type Result<T> = std::result::Result<T, CrateError>;` in each crate
- **From implementations**: Convert between error types using `#[from]` and manual conversions

### Communication Protocol
- **JSON over TCP**: All daemon-client communication uses JSON-serialized `Message`/`Response` types
- **Async pattern**: Use `tokio::spawn` for handling multiple connections concurrently
- **Buffer size**: Fixed 1024-4096 byte buffers for TCP communication

### Unix Process Management (NEW - Phase 2)
- **Safe process spawning**: All processes created in isolated process groups via `setsid()`
- **Reliable cleanup**: Process group signaling ensures no orphaned processes
- **Graceful termination**: SIGTERM → SIGKILL escalation with configurable timeouts
- **Error handling**: Robust handling of race conditions and edge cases (ESRCH, EPERM)
- **Module**: `core::process::unix` with `spawn()`, `signal_term_group()`, `signal_kill_group()`

### Workspace Dependencies
- **Internal crates**: Reference as `canopus-core = { path = "core" }` (note: `core` renamed to avoid std conflicts)
- **Shared deps**: Defined in root `Cargo.toml` under `[workspace.dependencies]`
- **Lint configuration**: Extensive workspace-level lints in root `Cargo.toml` with `forbid` on `unsafe_code`
- **Platform-specific**: Unix process deps (`nix`, `libc`) gated behind `cfg(unix)`

## Essential Development Workflows

### Building & Testing
```bash
just build          # Build all crates
just test            # Run all tests  
just lint-strict     # Clippy with pedantic rules
just ci              # Full CI pipeline
cargo test -p core   # Test specific crate
```

### Schema Generation
```bash
just gen-schemas     # Generate JSON schemas + TypeScript defs
cargo run -p xtask gen-schemas
```

### Running the System
```bash
# Terminal 1: Start daemon
just start-daemon    # or cargo run --bin daemon

# Terminal 2: Use CLI
just cli status      # or cargo run --bin canopus -- status
just cli custom "hello"
```

### Security & Dependencies
```bash
just deny            # cargo-deny checks (licenses, advisories, duplicates)
just audit           # Security vulnerability scanning
```

## Code Generation Patterns

### Schema Types
- All types in `schema/` must derive `JsonSchema`, `Serialize`, `Deserialize`
- Use `#[serde(rename_all = "camelCase")]` for external API consistency
- Provide `Default` implementations with sensible values
- Include `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields

### Error Types
- Use `thiserror::Error` for all error enums
- Include `code()` method returning static error codes
- Implement `From<std::io::Error>` and `From<serde_json::Error>` where appropriate

### Unsafe Code Handling (Process Management)
- **Workspace default**: `unsafe_code = "forbid"` at workspace level
- **Module-level override**: Use `#![allow(unsafe_code)]` only for process management modules
- **Safety documentation**: All unsafe blocks must include safety comments and justification
- **Lint enforcement**: Use `#[deny(unsafe_op_in_unsafe_fn)]` to ensure safety within unsafe functions
- **Minimal scope**: Limit unsafe code to essential system calls (`setsid()`, etc.)

### Async Handlers
- Clone `Arc`-wrapped state for `tokio::spawn` tasks
- Use `AtomicBool` for shutdown signaling
- Handle connection errors gracefully with `error!` logging

## Integration Points

### Schema Generation (`xtask/`)
- Generates both JSON schemas AND TypeScript definitions
- TypeScript uses discriminated unions for enums (e.g., `{ type: 'Status' }`)
- Schemas placed in `schemas/` directory, TypeScript in `schemas/ts/`

### Configuration Loading
- `DaemonConfig` and `ClientConfig` use workspace-wide defaults
- CLI overrides config via `--host` and `--port` flags
- Validation occurs in `core::utils::validate_config()`

### Graceful Shutdown
- Daemon listens for `Ctrl+C` signal using `tokio::signal::ctrl_c()`
- Uses `AtomicBool` for thread-safe shutdown coordination
- CLI commands can trigger daemon shutdown via `Message::Stop`

## Current Project Status (Phase 2 - Process Control)

### ✅ **Completed - T2.1: Unix Process Adapter**
- **Module**: `core::process::unix` with comprehensive process group management
- **API**: `ChildProcess`, `spawn()`, `signal_term_group()`, `signal_kill_group()`, `terminate_with_timeout()`
- **Safety**: Uses `unsafe { libc::setsid() }` for process group creation (properly documented)
- **Testing**: 49 total tests (38 unit + 11 integration + 4 doc tests)
- **Error codes**: CORE009 (ProcessSpawn), CORE010 (ProcessSignal), CORE011 (ProcessWait)

### 🚧 **Next Steps - T2.2: Windows Process Adapter**
- Goal: Job Object-based lifecycle management for Windows
- Implementation: `core::process::windows` using `KILL_ON_JOB_CLOSE` flag
- Pattern: Follow Unix implementation structure with Windows-specific APIs

## Testing Conventions

- **Unit tests**: In same file as implementation (`#[cfg(test)] mod tests`)
- **Integration tests**: Separate files (e.g., `error_tests.rs`, `core/tests/process_unix.rs`)
- **Platform-specific**: Use `#[cfg(unix)]` and `#[cfg(windows)]` for OS-specific tests
- **Async tests**: Use `#[tokio::test]` for async test functions
- **Schema validation**: Test both serialization and schema generation in `schema/`
- **Process tests**: Comprehensive signal handling, race conditions, and cleanup verification

## Critical Files to Understand

- `Cargo.toml`: Workspace configuration with extensive lint rules
- `schema/src/lib.rs`: Core data structures driving the entire communication protocol
- `core/src/error.rs`: Error handling patterns used throughout the codebase
- `justfile`: Comprehensive development commands and CI pipeline
- `deny.toml`: Security and license policy configuration
