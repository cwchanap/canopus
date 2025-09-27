# Canopus Configuration

This document describes the two configuration formats supported by Canopus and how to use them with the CLI and daemon.

- Service Spec TOML: full service definitions loaded by the daemon to create supervisors
- Simple Runtime TOML: lightweight per-service overrides (hostname/port) used to start and bind services

## Service Spec TOML (daemon --config)

The daemon accepts a full service specification file to define services, commands, health checks, and restart policies.

Two equivalent formats are supported:

- Array-of-tables (canonical): top-level `[[services]]` array of `ServiceSpec` objects
- Keyed-by-id tables (shorthand): each top-level table is a service identified by the section name

Example (array-of-tables):
```toml
[[services]]
id = "web"
name = "Web Server"
command = "python3"
args = ["-m", "http.server"]
# Optional working directory (alias: `cwd`)
# cwd = "/path/to/app"
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

Example (keyed-by-id tables):
```toml
[web]
name = "Web Server"
command = "python3"
args = ["-m", "http.server"]
# Optional working directory (alias: `cwd`)
cwd = "/path/to/app"
# route = "web.local"
restartPolicy = "OnFailure"
gracefulTimeoutSecs = 5
startupTimeoutSecs = 15

[web.readinessCheck]
checkType = { type = "tcp", port = 8000 }
intervalSecs = 2
timeoutSecs = 3
successThreshold = 1

[api]
name = "API Server"
command = "python3"
args = ["-m", "http.server", "9000"]
restartPolicy = "Never"
```

Run the daemon with a service spec file:
```bash
canopus-daemon --config services.toml
```

This defines which services the daemon manages. Each service becomes a supervisor with its own lifecycle, health monitoring, and restart policy.

Notes for keyed-by-id format:
- Top-level tables must all be service definitions (no unrelated top-level keys)
- Nested tables like `[web.backoffConfig]`, `[web.healthCheck]`, `[web.readinessCheck]` are supported
- Field names use camelCase per schema (e.g., `workingDirectory`, `restartPolicy`, `backoffConfig`)

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

Use the CLI's services subcommand to synchronize runtime state with the config (starts idle services, stops and deletes unlisted services):
```bash
canopus services start --config runtime.toml
```
Behavior:
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
  See example: `examples/services.toml`
- Apply runtime settings and keep DB in sync (after daemon has loaded your services):
  ```bash
  canopus services start --config runtime.toml
  See example: `examples/runtime.toml`
- Optionally preload runtime settings on daemon startup:
  ```bash
  canopus-daemon --config services.toml --runtime-config runtime.toml
  ```

## Hostname aliases and local DNS (Unix)

- The daemon attempts to bind a hostname to 127.0.0.1 by appending a line to `/etc/hosts` (tagged with `# canopus`) when a service is started via the UDS control plane.
- The effective hostname is resolved in this precedence order:
  1) Start parameter `--hostname`
  2) Persisted runtime metadata (SQLite)
  3) Volatile in-memory metadata (current daemon process)
  4) Service spec `route` field (e.g., `route = "cetus.local.dev"`)
- On service stop, the daemon attempts to remove the corresponding `/etc/hosts` line using the same precedence (best-effort cleanup).
- Editing `/etc/hosts` requires elevated privileges on macOS/Linux. If the daemon does not have permission, it will log a warning and the alias will not be installed. In that case, add an entry manually:
  ```bash
  echo "127.0.0.1 cetus.local.dev # canopus" | sudo tee -a /etc/hosts
  ```
- Built-in local reverse proxy: the daemon starts a lightweight HTTP reverse proxy on `127.0.0.1:9080` and routes by Host header to the service's backend port when the service becomes Ready. You can override the listen address with `CANOPUS_PROXY_LISTEN` (e.g., `CANOPUS_PROXY_LISTEN=127.0.0.1:9090`).
  - With this proxy, you can access services using the alias and the proxy port (e.g., `http://cetus.local.dev:9080`).
- Without any reverse proxy, you must include the service port in the URL (the OS will default to port 80/443 otherwise):
  ```bash
  curl http://cetus.local.dev:4325
  ```
- To use aliases without specifying a port, run a local reverse proxy (e.g., Caddy, Nginx, Traefik) that listens on :80/:443 and routes by Host header. Example Caddyfile:
  ```
  cetus.local.dev {
    reverse_proxy 127.0.0.1:4325
  }
  ```

Troubleshooting:
- If `curl http://<alias>` says "Could not resolve host", check that `/etc/hosts` has the alias and that it wasn't removed by a previous stop.
- If the alias resolves but you get a connection error on `http://<alias>`, try `http://<alias>:<port>` or configure a reverse proxy.

## Notes

- The CLI’s `--config` file is the simple runtime format. The daemon’s `--config` is the full Service Spec TOML.
- Removing a service from the runtime config is treated as a signal to stop it and delete its DB row when you use the CLI `canopus services start --config` workflow.
- Future extensions could allow creating fully new services directly from a config file that includes the entire `ServiceSpec` per service ID.
