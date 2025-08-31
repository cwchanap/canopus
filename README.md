# Canopus

A multi-crate Rust workspace for a daemon server and CLI management tool.

## Architecture

This workspace consists of three crates:

### 📦 Core (`core/`)
Common library containing shared types, utilities, and business logic used by both the daemon and CLI.

**Features:**
- Configuration management
- Message and response types for daemon-CLI communication
- Common error handling
- Shared data structures

### 🔧 Daemon (`daenib/`)
A Rust daemon server that runs in the background and handles various operations.

**Features:**
- TCP server for handling client connections
- Asynchronous message processing
- Status reporting and management
- Graceful shutdown handling

### 💻 CLI (`cli/`)
Command-line interface tool for managing and communicating with the daemon.

**Features:**
- Subcommands for daemon management (start, stop, restart, status)
- Custom command support
- Configurable daemon connection settings
- User-friendly output formatting

## Building

Build all crates:
```bash
cargo build
```

Build specific crate:
```bash
cargo build -p core
cargo build -p daenib
cargo build -p cli
```

## Running

### Start the daemon:
```bash
cargo run --bin daenib
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
├── daenib/             # Daemon server
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
