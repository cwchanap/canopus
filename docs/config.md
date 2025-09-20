# Canopus Configuration

This document describes the two configuration formats supported by Canopus and how to use them with the CLI and daemon.

- Service Spec TOML: full service definitions loaded by the daemon to create supervisors
- Simple Runtime TOML: lightweight per-service overrides (hostname/port) used to start and bind services

## Service Spec TOML (daemon --config)

The daemon accepts a full service specification file to define services, commands, health checks, and restart policies. The file schema is a top-level `services` array of `ServiceSpec` objects.

Example `services.toml`:
```toml
[[services]]
id = "web"
name = "Web Server"
command = "python3"
args = ["-m", "http.server"]
# Optional working directory
# workingDirectory = "/path/to/app"
# Optional hostname alias for reverse proxy
# route = "web.local"

# Restart policy: "Always" | "OnFailure" | "Never"
restartPolicy = "OnFailure"

# Backoff (defaults shown)
[services.backoffConfig]
initialMillis = 500
maxMillis = 10000
multiplier = 2.0

# Optional health check
# [services.healthCheck]
# type = { http = { url = "http://127.0.0.1:8000/health", timeoutMillis = 1000 } }
# intervalMillis = 2000
# failureThreshold = 3

# Optional readiness check
# [services.readinessCheck]
# type = { tcp = { host = "127.0.0.1", port = 8000, timeoutMillis = 1000 } }
# intervalMillis = 500
# successThreshold = 1

# Timeouts (seconds)
gracefulTimeoutSecs = 5
startupTimeoutSecs = 15
```

Run the daemon with a service spec file:
```bash
canopus-daemon --config services.toml
```

This defines which services the daemon manages. Each service becomes a supervisor with its own lifecycle, health monitoring, and restart policy.

## Simple Runtime TOML (CLI --config or daemon --runtime-config)

The simple runtime config is a lightweight format to specify per-service runtime overrides:
- hostname: optional domain alias for the reverse proxy
- port: optional fixed port; if omitted, the system may allocate a free port

Format: one top-level table per service ID.

Example `runtime.toml`:
```toml
[web]
hostname = "web.local"
port = 8000

[api]
hostname = "api.local"
# port is optional
```

### Using the CLI to apply runtime config

Use the CLI to start the daemon if needed and synchronize runtime state with the config:
```bash
canopus start --config runtime.toml
```
Behavior:
- Starts the daemon if not running
- Loads `runtime.toml`
- For each service in the file:
  - If the service is known to the daemon and idle, the CLI starts it and applies the hostname/port
  - If already running, it is skipped
- For any service known to the daemon but not present in `runtime.toml`:
  - CLI stops it
  - CLI removes its metadata row from SQLite to keep DB in sync

Note: The simple runtime config does not define commands or health checks. Services must be defined in the daemon’s Service Spec TOML (see above). If you list a service in `runtime.toml` that is unknown to the daemon, it will be skipped with a warning.

### Using the daemon to preload runtime config

You can also ask the daemon to preload hostname/port metadata at startup. This does not start/stop services by itself; it seeds the persisted metadata so that subsequent starts can reuse it.
```bash
canopus-daemon --config services.toml --runtime-config runtime.toml
```
Behavior:
- Loads service specs from `services.toml`
- Preloads hostname and port from `runtime.toml` into SQLite for matching services

## Persistence and IPC

- SQLite DB: `~/.canopus/canopus.db`
  - Stores service ID, name, state, PID, port, and hostname
- IPC (UDS) defaults:
  - Socket: `/tmp/canopus.sock` (override with `CANOPUS_IPC_SOCKET`)
  - Token: optional `CANOPUS_IPC_TOKEN` environment variable

## Common Workflows

- Define services once in `services.toml` and run the daemon:
  ```bash
  canopus-daemon --config services.toml
  ```
- Apply runtime settings and keep DB in sync:
  ```bash
  canopus start --config runtime.toml
  ```
- Optionally preload runtime settings on daemon startup:
  ```bash
  canopus-daemon --config services.toml --runtime-config runtime.toml
  ```

## Notes

- The CLI’s `--config` file is the simple runtime format. The daemon’s `--config` is the full Service Spec TOML.
- Removing a service from the runtime config is treated as a signal to stop it and delete its DB row when you use the CLI `canopus start --config` workflow.
- Future extensions could allow creating fully new services directly from a config file that includes the entire `ServiceSpec` per service ID.
