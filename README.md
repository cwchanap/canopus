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
- TCP-based communication protocol
- Async message handling
- Connection management with proper error handling
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
- Status reporting and management
- Graceful shutdown handling

### 💻 CLI (`cli/`)
Command-line interface tool for managing and communicating with the daemon.

**Features:**
- Subcommands for daemon management (start, stop, restart, status)
- Custom command support with structured error reporting
- Configurable daemon connection settings
- User-friendly output formatting

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
cargo run --bin daemon
```

### Use the CLI:
```bash
# Get daemon status
cargo run --bin canopus -- status

# Start daemon (if not running)
cargo run --bin canopus -- start

# Stop daemon
cargo run --bin canopus -- stop

# Restart daemon
cargo run --bin canopus -- restart

# Send custom command
cargo run --bin canopus -- custom "hello world"

# Use different host/port
cargo run --bin canopus -- --host 192.168.1.100 --port 9090 status
```

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
- `Status { running: bool, uptime: u64 }` - Status information

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
