# Canopus

A multi-crate Rust workspace for a daemon server and CLI management tool with comprehensive error handling and schema generation.

## Architecture

This workspace consists of six main crates:

### 📦 Core (`core/`)
Common library containing shared utilities, error handling, and business logic used across all components.

**Features:**
- Centralized error handling with `CoreError` types
- Configuration validation utilities
- Tracing setup and management
- Result type aliases for consistent error handling

### 🔗 IPC (`ipc/`)
Inter-process communication library for daemon-client communication.

**Features:**
- TCP-based control plane between CLI and daemon (default: `127.0.0.1:49384`)
- Local Unix Domain Socket (UDS) JSON-RPC control plane for managing services (`/tmp/canopus.sock`)
- Async message handling and robust error handling
- Serialization/deserialization of messages

### 🛠️ Schema (`schema/`)
Shared data structures and JSON schema generation.

**Features:**
- Message and response type definitions
- Configuration structures with JSON Schema support
- Event and state representations
- TypeScript definition generation

### 🔧 Daemon (`daemon/`)
A Rust daemon server that runs in the background and handles client operations.

**Features:**
- TCP server for handling client connections
- Asynchronous message processing with proper error handling
- Status reporting (now includes PID and version)
- Graceful shutdown handling

### 💻 CLI (`cli/`)
Command-line interface tool for managing and communicating with the daemon.

**Features:**
- Subcommands for daemon management (`start`, `stop`, `restart`, `status`, `version`)
- `services` subcommand for local UDS control plane operations (`list`, `status <SERVICE_ID>`, `start <SERVICE_ID>`, `stop`, `restart`, `health`, `tail-logs`)
- Auto-starts the daemon if not running
- Configurable daemon connection settings (`--host`, `--port`, default port `49384`)
- User-friendly output formatting (daemon `status` shows PID and version; `services status` shows per-service PID)

### ⚙️ XTask (`xtask/`)
Development automation and schema generation tool.

**Features:**
- JSON schema generation from Rust types
- TypeScript definition generation
- Development workflow automation

## Building

Build all crates:
```bash
cargo build
```

Build specific crate:
```bash
cargo build -p core
cargo build -p daemon
cargo build -p cli
```

## Running

### Start the daemon:
```bash
cargo run --bin daemon -- --host 127.0.0.1 --port 49384
```

### Use the CLI:
```bash
# Get daemon status
cargo run --bin canopus -- status

# Print daemon version (via TCP)
cargo run --bin canopus -- version

# Start daemon (if not running)
cargo run --bin canopus -- start

# Stop daemon
cargo run --bin canopus -- stop

# Restart daemon
cargo run --bin canopus -- restart

# Send custom command
cargo run --bin canopus -- custom "hello world"

# Use different host/port (TCP)
cargo run --bin canopus -- --host 192.168.1.100 --port 50000 status

# Manage services via local UDS control plane
cargo run --bin canopus -- services list
cargo run --bin canopus -- services status <SERVICE_ID>
cargo run --bin canopus -- services start <SERVICE_ID>
cargo run --bin canopus -- services stop <SERVICE_ID>
cargo run --bin canopus -- services health <SERVICE_ID>
```

## Configuration

See detailed formats and usage in `docs/config.md`.

Quick start:

- Daemon with service spec:
  ```bash
  canopus-daemon --config services.toml
  ```
  See example: `examples/services.toml`

- Daemon with runtime overrides (hostname/port):
  ```bash
  canopus-daemon --config services.toml --runtime-config runtime.toml
  ```
  See example: `examples/runtime.toml`

- CLI apply runtime config and sync DB (start/skip/stop+delete):
  ```bash
  canopus start --config runtime.toml
  ```
  See example: `examples/runtime.toml`

## Testing

Run all tests:
```bash
cargo test
```

Run tests for specific crate:
```bash
cargo test -p core
```

## Development

### Workspace Structure
```
canopus/
├── Cargo.toml          # Workspace configuration
├── core/               # Core library
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs
├── daemon/             # Daemon server
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       └── main.rs
└── cli/                # CLI tool
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        └── main.rs
```

### Communication Protocol

The daemon and CLI communicate over TCP using JSON-serialized messages:

**Message Types:**
- `Status` - Get daemon status
- `Start` - Start daemon operations  
- `Stop` - Stop daemon operations
- `Restart` - Restart daemon
- `Custom(String)` - Send custom command

**Response Types:**
- `Ok(String)` - Success with message
- `Error(String)` - Error with message
- `Status { running: bool, uptime_seconds: u64, pid: u32, version?: string }` - Status information

### Service Control (UDS JSON-RPC)

The daemon exposes a local UDS JSON-RPC control plane for service management at `/tmp/canopus.sock` by default. The CLI `services` subcommand uses this channel.

- `services start <SERVICE_ID>` waits until the service reaches the Ready state by default, with a timeout of `startup_timeout_secs + 5s`.
- `services status <SERVICE_ID>` returns the current state and the service's PID if it is running.
- Spawned service processes are detached from the terminal (stdin is set to null) and placed in their own process group for safe signal handling.

## Dependencies

The workspace uses shared dependencies defined in the root `Cargo.toml`:
- **tokio**: Async runtime
- **serde**: Serialization/deserialization
- **clap**: CLI argument parsing

Additional crate-specific dependencies:
- **tracing**: Logging framework
- **serde_json**: JSON serialization

## License

MIT OR Apache-2.0
