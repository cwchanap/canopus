# Canopus Desktop — Product Requirements Document

**Version:** 0.1 draft
**Date:** 2026-02-23
**Status:** Draft

---

## Executive Summary

Canopus Desktop is a Tauri v2 + Svelte native application that surfaces the Canopus daemon and agent-inbox system through a polished GUI. The v0.1 build delivers the essential skeleton: a Projects view that groups services into named projects with per-service start/stop/restart controls and real-time log tailing, and an Agents/Inbox view that lists AI agent notifications from Claude Code, Codex, Windsurf, and OpenCode with read/dismiss actions. While functional, the app is not yet usable as a daily driver: project membership requires direct JSON editing, service state polls on a five-second timer with no event-driven push, inbox notifications require a manual page visit, the window has no persistent geometry, and several Canopus core capabilities (Exec health probes, log replay, log persistence beyond 1 024 in-memory lines) remain unfinished. This document defines the full set of features needed to reach a production-quality release, ordered by priority.

---

## Background and Problem Statement

### The Canopus daemon and its gaps

Canopus supervises arbitrary processes — web servers, API back-ends, compilers, local AI tools — with restart policies, health checks (TCP only today), and exponential back-off. It exposes two IPC planes: a TCP control plane for daemon lifecycle and a UDS JSON-RPC plane (`/tmp/canopus.sock`) for per-service operations. The schema includes a fully-typed `Exec` health check variant (`HealthCheckType::Exec`) but the implementation in `core/src/health/mod.rs` returns `HealthError::UnsupportedProbeType` immediately. The `tailLogs` RPC accepts a `fromSeq` parameter so that callers can replay buffered history, but `SupervisorControlPlane::tail_logs` ignores the parameter and only streams future events; the in-memory ring buffer (`LogRing`, capacity 1 024) is never consulted at subscription time. No metrics or observability endpoint exists.

### The desktop app and its gaps

The desktop app works but has several rough edges that prevent daily use:

1. **Project membership is manual.** `~/.canopus/projects.json` is the only way to assign a service to a project. The UI can create an empty project but has no drag-and-drop or checkbox UI for assigning services.

2. **Service state is polled, not pushed.** `ProjectsView.svelte` calls `list_services()` every five seconds via `setInterval`. During a start or stop operation, the card can display stale state for up to five seconds. Meanwhile, the daemon's event bus (`broadcast::Sender<ServiceEvent>`) already emits `StateChanged` events that could drive instant updates.

3. **Inbox is pull-only.** `InboxView.svelte` loads items once on mount. New items inserted by AI agents after page load are invisible until the user switches views or refreshes. The `notified` column exists in the schema and the `DesktopNotifier` in `inbox/src/notify.rs` can send macOS/Linux/Windows notifications, but neither the Tauri backend nor any background task uses them.

4. **No system tray.** When the window is hidden or the user switches to another app, there is no ambient signal for unread inbox count or service health problems. A system tray icon with a badge is the standard affordance for this class of tool.

5. **No window state persistence.** The `tauri.conf.json` sets a fixed 1 280 × 800 initial size with no `saveWindowState` or equivalent. Closing and reopening the app always restores the same position and size.

6. **No service spec inspector.** The only fields shown per service are id, name, state, pid, port, and hostname. A developer cannot see or edit the restart policy, backoff configuration, health check type, working directory, or environment variables from the desktop app.

7. **Log viewer has no search or filter.** The 500-line in-memory slice kept in `stores.ts` is rendered as-is. There is no way to find a specific string, filter by stream (stdout vs stderr), or jump to a point in time.

8. **No bulk project actions.** Stopping or starting all services in a project requires clicking each card individually.

9. **Single daemon, local only.** `AppState::new()` hard-codes the socket path from `CANOPUS_IPC_SOCKET` and uses no UI to add remote daemons. Teams running Canopus on a remote machine cannot connect from the desktop.

---

## Goals

- Make Canopus Desktop a daily-driver tool for developers who manage multiple local services.
- Expose all actionable Canopus capabilities through the GUI so users never need to touch JSON files or a terminal for routine operations.
- Deliver real-time feedback so users always know the true state of their services and their agent inbox.
- Keep the implementation consistent with the existing Tauri v2 / Svelte / Rust architecture and dark-themed UI design.

## Non-Goals

- Cloud dashboard or hosted SaaS offering (out of scope for this release).
- Windows support for process management (the process adapter is Unix-only; Windows Named Pipe IPC is also stubbed out; tracked separately as a platform work item).
- Log storage in a persistent database (the in-memory ring buffer is the source of truth for now; full persistence is a follow-on).
- A built-in code editor or AI agent IDE integration.
- Mobile or tablet form factor.

---

## Feature Sections

---

### F1 — Service-to-Project Assignment UI

**Priority: P0**

#### Overview

Currently the only way to assign a service to a project is to manually edit `~/.canopus/projects.json`. The `saveProjects` Tauri command already accepts a full `ProjectConfig`; the data model is correct. What is missing is a UI that lets users drag a service card into a project section or use a checkbox/picker to assign membership.

The "Other Services" section at the bottom of `ProjectsView.svelte` renders ungrouped services. This is a natural staging area: a service dropped onto a project header (or selected via a context menu) should call `saveProjects` with the updated membership list and re-render immediately.

#### User Stories

- As a developer, I want to drag a service card from "Other Services" into a named project section so that my workspace is organized by context without editing JSON.
- As a developer, I want to right-click a service card and choose "Move to project..." so that I have a fallback when drag-and-drop is unavailable.
- As a developer, I want to remove a service from a project (returning it to "Other Services") without deleting the project or the service.
- As a developer, I want to rename or delete a project from the UI, with a confirmation dialog before deletion.

#### Acceptance Criteria

- Dragging a service card from one project section (or "Other Services") to another updates the in-memory store and calls `save_projects` within 200 ms of drop.
- The move is reflected immediately in the view with no full reload.
- A "Move to project" context menu entry appears on right-click (or a three-dot overflow button on the card) and opens a modal listing current projects with a radio selector.
- Double-clicking a project heading puts it into inline edit mode; pressing Enter or clicking away commits the rename via `save_projects`.
- A "Delete project" option (accessible via project header overflow menu) shows a confirmation dialog and, on confirm, removes the project and returns its services to "Other Services".
- All changes are persisted to `~/.canopus/projects.json` atomically (write to temp file, rename).

#### Technical Considerations

- Svelte 4 drag-and-drop can be implemented without a library using the native HTML Drag and Drop API; `dataTransfer` carries the service id string.
- `saveProjects` already performs a full JSON write; call it after every membership mutation.
- Project rename and delete require no new Tauri commands — the existing `save_projects` command is sufficient.
- The context menu can be built as a small absolutely-positioned Svelte component anchored to the card; no external library is required at this scale.

---

### F2 — Real-Time State Updates via Event Subscription

**Priority: P0**

#### Overview

`ProjectsView.svelte` re-fetches the full service list every five seconds using `setInterval`. This means a user sees stale state for up to five seconds after clicking Stop or Start. The daemon already broadcasts typed `ServiceEvent` values on a `broadcast::Sender<ServiceEvent>` channel. The IPC server's `tailLogs` subscription mechanism already forwards log events as JSON-RPC notifications; the same pattern can carry `StateChanged`, `ProcessStarted`, `ProcessExited`, and `ServiceUnhealthy` events.

The goal is to replace the polling loop with a persistent event subscription so that service card states update within one render frame of the actual state transition.

#### User Stories

- As a developer, I want a service card's status indicator to change within one second of the daemon transitioning a service to a new state so I can see restarts and crashes without waiting.
- As a developer, I want the service list to be updated automatically when a new service is registered with the daemon, without needing to refresh.

#### Acceptance Criteria

- A persistent connection (or long-lived UDS subscription) to the daemon delivers `StateChanged`, `ProcessStarted`, `ProcessExited`, and `ServiceUnhealthy` events to the Tauri backend.
- The Tauri backend emits a Tauri event (`service-state-changed`) for each received daemon event.
- `ProjectsView.svelte` subscribes to `service-state-changed` and patches the relevant `ServiceSummary` in the `services` store without replacing the entire list.
- The polling `setInterval` in `ProjectsView.svelte` is replaced by this subscription; a single initial fetch on mount populates the list.
- If the subscription connection drops (daemon stopped), the app falls back to a reconnect loop and shows a "Daemon offline" indicator in the sidebar.
- End-to-end latency from daemon state change to card UI update is under one second in normal conditions.

#### Technical Considerations

- The cleanest implementation adds a new `canopus.subscribeEvents` JSON-RPC method to `IpcServer` that registers the connection to receive `StateChanged` and other operational events as JSON-RPC notifications (same mechanism as `canopus.tailLogs.update`).
- The Tauri backend spawns a background Tokio task (similar to the existing `start_log_tail` command) that subscribes to the daemon's broadcast channel and forwards relevant events as Tauri window events.
- The `SupervisorControlPlane` already has an `event_tx: broadcast::Sender<ServiceEvent>`; extending `tail_logs` or adding a parallel subscription method requires minimal new Rust code.
- A 30-second reconnect timer with exponential back-off is sufficient for daemon restart scenarios.

---

### F3 — Real-Time Inbox Push and Desktop Notifications

**Priority: P0**

#### Overview

`InboxView.svelte` loads inbox items once on mount and does not refresh. If an AI agent posts a new inbox item while the user is on the Projects view (or while the app is minimized), the badge on the Agents nav item does not update and no OS-level notification is delivered.

The infrastructure for notifications already exists: `inbox/src/notify.rs` provides a `DesktopNotifier` that calls `osascript` on macOS and `notify-rust` on Linux. The `notified` column in the `inbox_items` table tracks whether a notification was delivered. What is missing is a background Tauri task that polls or watches `inbox.db` for unread items with `notified = 0` and dispatches them.

#### User Stories

- As a developer, I want to receive a macOS/Linux system notification when an AI agent posts a new inbox item, even if the Canopus Desktop window is focused on the Projects view.
- As a developer, I want the Agents nav badge to update immediately when a new item arrives, without switching views.
- As a developer, I want clicking a system notification to bring the Canopus Desktop window to the foreground and navigate to the Agents/Inbox view.

#### Acceptance Criteria

- A background Tauri task polls `inbox.db` for rows where `status = 'unread' AND notified = 0` every ten seconds.
- For each such item, the task calls `DesktopNotifier::send_notification`, then calls `mark_notified` on the store.
- The task emits a `inbox-new-items` Tauri event carrying the count of new items and a snapshot of their ids.
- The frontend `stores.ts` subscribes to `inbox-new-items` and prepends the new items to the `inboxItems` store, updating the sidebar badge immediately.
- Clicking the system notification on macOS (using `osascript`) brings the window to front and sets `activeView` to `"inbox"`.
- The polling interval is configurable via an environment variable `CANOPUS_INBOX_POLL_INTERVAL_SECS` (default 10).
- The feature degrades gracefully: if `is_available()` returns false, notification delivery is skipped but in-app updates still occur.

#### Technical Considerations

- Tauri's `app.notification()` plugin (`tauri-plugin-notification`) provides cross-platform native notifications that integrate better with the Tauri lifecycle than calling `osascript` directly; evaluate whether to migrate `DesktopNotifier` or wrap it.
- The background poll task should be started in `lib.rs`'s `setup` closure after `AppState` is initialized.
- The `SqliteStore::count` method can efficiently check for `{ status: Unread, notified: false }` without fetching full rows.
- File-system watching (`notify` crate) on `inbox.db` would allow instant notification rather than polling, but SQLite WAL writes do not reliably trigger inotify on all platforms; polling is simpler and sufficient at 10-second intervals.

---

### F4 — System Tray Icon with Notification Badge

**Priority: P0**

#### Overview

There is currently no system tray icon. When the Canopus Desktop window is hidden or behind other windows, there is no ambient indicator for unread inbox items or unhealthy services. A system tray icon is the expected pattern for a daemon-management tool: it keeps the app accessible and provides at-a-glance health status.

#### User Stories

- As a developer, I want a Canopus tray icon in my macOS/Linux menu bar so I can tell at a glance whether any services are unhealthy.
- As a developer, I want the tray icon to show a badge or change color when there are unread inbox items so I notice agent notifications without switching windows.
- As a developer, I want to click the tray icon to show or hide the Canopus Desktop window.
- As a developer, I want a tray context menu with quick actions: "Show window", "Quit".

#### Acceptance Criteria

- A system tray icon is registered at application start using `tauri-plugin-tray`.
- The icon changes to a highlighted state (e.g., a dot overlay) when `unreadCount > 0`.
- Left-clicking the tray icon shows the main window if hidden, or hides it if visible.
- Right-clicking shows a context menu with at minimum: "Show Canopus", separator, "Quit".
- The tray icon tooltip displays a summary string: "Canopus: N services running, M unread".
- The tray state is updated whenever the `services` or `inboxItems` stores change.
- On macOS, the icon uses a monochrome template image that adapts to dark/light menu bar themes.

#### Technical Considerations

- Tauri v2 provides `tauri-plugin-tray` for cross-platform tray support; add it as a dependency alongside `tauri-plugin-notification`.
- The `tauri.conf.json` must declare `trayIcon` in the `app` section and set the window to `visible: false` initially if tray-first launch is desired (controlled by a user preference).
- Icon assets (16 × 16 and 32 × 32 PNG, plus a macOS template image) must be added to `src-tauri/icons/`.
- The Rust side constructs the tray in the `setup` closure and listens to Svelte store changes via channel.

---

### F5 — Window State Persistence

**Priority: P1**

#### Overview

`tauri.conf.json` sets a fixed initial window size of 1 280 × 800. Every restart returns the window to its default position and size. Users who resize or reposition the window must do this on every launch.

#### User Stories

- As a developer, I want the Canopus Desktop window to reopen at the same position and size I last used it so that it fits my monitor layout without manual adjustment.

#### Acceptance Criteria

- On window close or on a periodic save (every 30 seconds), window position, size, and maximized state are written to `~/.canopus/window-state.json`.
- On next launch, if the file exists and the saved bounds are within the current screen geometry, the window opens at the saved position and size.
- If the saved position is off-screen (e.g., secondary monitor removed), the window falls back to the default centered position.
- Maximized state is restored correctly.

#### Technical Considerations

- `tauri-plugin-window-state` provides this feature out of the box for Tauri v2; adding it requires a one-line plugin registration in `lib.rs` and a capability entry.
- If the plugin is not used, a custom implementation using `window.outerPosition()`, `window.outerSize()`, and `fs::write` is straightforward but duplicates plugin logic.

---

### F6 — Service Spec Inspector and Editor

**Priority: P1**

#### Overview

The `ServiceCard` component shows only `id`, `name`, `state`, `pid`, `port`, and `hostname`. A developer cannot inspect the restart policy, backoff configuration, health check type, or working directory. A read-only inspector (with optional edit capability for a subset of fields) would make the app significantly more useful for debugging.

The `get_service_detail` Tauri command already exists and calls `ipc.status()`, but the `ServiceDetail` struct in `ipc/src/server.rs` only contains the same six fields as `ServiceSummary`. The `SupervisorControlPlane` has a `GetSpec` control message that returns the full `ServiceSpec`; the IPC layer needs to surface this.

#### User Stories

- As a developer, I want to click on a service card to open a detail panel that shows the full service spec (command, args, working directory, restart policy, health check, environment variables) so that I can verify what the daemon is running.
- As a developer, I want to change the restart policy of a running service from the GUI (e.g., switch from `Always` to `Never`) without restarting the daemon.

#### Acceptance Criteria

**Read-only Inspector (must-have):**
- Clicking a service card opens a right-side detail panel (or modal) showing all `ServiceSpec` fields: `command`, `args`, `workingDirectory`, `restartPolicy`, `backoffConfig`, `healthCheck`, `readinessCheck`, `gracefulTimeoutSecs`, `startupTimeoutSecs`, `environment`.
- The detail panel also shows runtime state: current `state`, `pid`, `port`, `hostname`, and last health check result if available.
- Closing the panel returns focus to the projects list.

**Spec Edit (should-have):**
- `restartPolicy` is editable via a dropdown (Always / OnFailure / Never).
- `gracefulTimeoutSecs` and `startupTimeoutSecs` are editable via number inputs.
- Saving an edit sends the updated spec to the daemon via a new `canopus.updateSpec` IPC method that maps to `ControlMsg::UpdateSpec`.

#### Technical Considerations

- The daemon's `SupervisorHandle` already handles `ControlMsg::GetSpec` via a oneshot channel. The IPC server needs a new `canopus.getSpec` method and the `SupervisorControlPlane` needs to implement it.
- `ServiceDetail` in `ipc/src/server.rs` should be extended to include a `spec: Option<ServiceSpec>` field, serialized with `skip_serializing_if`.
- `ControlMsg::UpdateSpec` uses `Box<ServiceSpec>` and is already defined; wiring it up through the IPC layer is the main work.
- On the frontend, environment variables should be displayed as a key-value table with copy-to-clipboard on key/value cells.
- Sensitive values (anything with a key matching `*_TOKEN`, `*_SECRET`, `*_KEY`, `*_PASSWORD`) should be masked by default with a reveal button.

---

### F7 — Log Search and Filtering

**Priority: P1**

#### Overview

The `LogViewer` component renders up to 500 lines in order with no way to search or filter. For services that emit high volumes of output (build tools, language servers), finding a specific error requires manual scrolling. Common log viewer features — a search input that highlights matches, a stream toggle to show only stderr, and a line count indicator — would make the panel significantly more useful.

#### User Stories

- As a developer, I want to type a search string in the log panel and have matching lines highlighted so that I can find errors without scrolling.
- As a developer, I want to toggle between showing all log lines, stdout only, and stderr only.
- As a developer, I want to know how many lines are in the buffer and how many are hidden by my current filter.

#### Acceptance Criteria

- A search input appears at the top of the log panel. As the user types, lines that do not contain the search string (case-insensitive by default) are hidden; matches are highlighted with a contrasting background.
- A "stderr only" / "stdout only" / "all" toggle button group is placed next to the search input.
- A line counter shows "Showing N of M lines" where M is the total buffer size and N is the count after filtering.
- Pressing Escape clears the search and restores all lines.
- The auto-scroll toggle from the current implementation is preserved.
- Filtering is purely client-side (no round-trip to daemon); it operates on the existing `LogEvent[]` in the `logs` store.

#### Technical Considerations

- Svelte's reactive statements handle this efficiently: derive a filtered array from `$logs[serviceId]` and the search/filter state.
- For highlighting, split each line's `content` on the search term and render the match segments in a `<mark>` element styled with a dim yellow or purple background consistent with the existing dark theme.
- Performance: with a 500-line cap, client-side filtering is synchronous and negligible; no virtualization is needed at this scale.

---

### F8 — Project-Level Bulk Actions

**Priority: P1**

#### Overview

A developer with a project containing six services must click Stop six times to bring down an environment. Bulk "Start all" and "Stop all" buttons at the project header level are a standard affordance for grouped service managers.

#### User Stories

- As a developer, I want a "Start all" button on a project header that starts all idle services in that project with a single click.
- As a developer, I want a "Stop all" button that gracefully stops all running services in that project.
- As a developer, I want visual feedback during a bulk operation showing which services are being acted on.

#### Acceptance Criteria

- Each project section header displays "Start all" and "Stop all" buttons. "Start all" is enabled when at least one service in the project is `Idle`. "Stop all" is enabled when at least one service is running (`Ready`, `Starting`, or `Spawning`).
- Clicking "Start all" sends individual `start_service` calls concurrently (via `Promise.all`) for all `Idle` services in the project.
- Clicking "Stop all" sends individual `stop_service` calls concurrently for all running services.
- Each affected service card shows its loading state independently during the operation.
- A bulk operation does not block the UI; cards for services not affected remain interactive.

#### Technical Considerations

- No new Tauri commands are needed; the existing `start_service` and `stop_service` commands handle individual services.
- Use `Promise.all(services.map(s => startService(s.id)))` in the click handler.
- The service state updates should be driven by the real-time event subscription from F2 so card states change as each service actually starts rather than all at once after the promises resolve.

---

### F9 — Log History Replay via fromSeq

**Priority: P1**

#### Overview

When a user opens the log panel for a service, the current implementation calls `tail_logs` with `fromSeq: undefined`. The IPC server passes `from_seq` to `SupervisorControlPlane::tail_logs` but the implementation ignores it (parameter is `_from_seq`) and only subscribes to future events. The `LogRing` in `service_task.rs` holds the last 1 024 lines and has an `iter_after(seq)` method and a `snapshot()` method. What is missing is the plumbing between the ring buffer and the `tail_logs` subscription.

#### User Stories

- As a developer, I want opening the log panel to immediately show the recent history of the service's output (up to the buffer limit) rather than a blank "Waiting for output..." screen.

#### Acceptance Criteria

- When `tail_logs` is called with `from_seq: None` (or `from_seq: 0`), the supervisor sends the current ring buffer snapshot as a sequence of `LogOutput` events before switching to live streaming.
- When `from_seq` is provided as a positive integer, only entries with `seq > from_seq` are replayed.
- The frontend receives these historical events before the live stream and renders them in order.
- The desktop app calls `start_log_tail` with `fromSeq: 0` (or the current last-known seq stored in the logs store) to receive history on open.

#### Technical Considerations

- `SupervisorControlPlane::tail_logs` needs access to the `LogRing` for the given service. This requires either: (a) adding a `GetLogSnapshot` control message to `ControlMsg` and handling it in `ServiceSupervisor`, or (b) storing the `Arc<Mutex<LogRing>>` in `SupervisorHandle` as a shared reference alongside the existing channels.
- Option (b) is simpler: expose a `log_ring: Arc<Mutex<LogRing>>` field on `SupervisorHandle` (currently only `ControlMsg` channels and state watchers are exposed).
- Once the ring is accessible, `tail_logs` drains the snapshot into the output channel before spawning the live-stream task.
- The Tauri `start_log_tail` command should pass `from_seq: Some(0)` to get the full buffer.

---

### F10 — Exec Health Check Implementation

**Priority: P1**

#### Overview

The `HealthCheckType::Exec` variant is fully defined in `schema/src/service.rs` with `command: String` and `args: Vec<String>`. It appears in the config docs and users can write it in their `services.toml`. However `core/src/health/mod.rs` returns `HealthError::UnsupportedProbeType("Exec probes not yet implemented")` for any Exec check. Any user relying on this probe type silently gets no health checking.

#### User Stories

- As a developer, I want to define a health check using a shell command (e.g., `curl -f http://localhost:8080/health`) so that I can use the full HTTP health check semantics without the daemon needing a built-in HTTP probe.
- As a developer, I want startup to wait until my Exec readiness check exits 0 before the service is marked Ready.

#### Acceptance Criteria

- `create_probe` in `core/src/health/mod.rs` returns an `ExecProbe` for `HealthCheckType::Exec`.
- `ExecProbe::check()` runs the command with the provided args, waits for it to exit, and returns `Ok(())` if exit code is 0, `Err(HealthError::ExecFailed(...))` otherwise.
- A configurable timeout is applied; if the command does not exit within the timeout, it is killed and `Err(HealthError::Timeout(...))` is returned.
- The command is run in the service's configured `working_directory` if set.
- Environment variables from the `ServiceSpec` are inherited by the check process.
- Unit tests cover: successful check (exit 0), failed check (exit 1), timeout, and command-not-found error.

#### Technical Considerations

- Use `tokio::process::Command` for the async exec with `tokio::time::timeout` wrapping the `.wait()` call.
- Process is spawned without `setsid()` (it is a short-lived probe, not a supervised service), so `kill()` on the `Child` handle is sufficient.
- The `ExecProbe` struct should hold `command: String`, `args: Vec<String>`, `timeout: Duration`, and `working_dir: Option<PathBuf>`.
- `unsafe_code` is not needed for this implementation.

---

### F11 — Inbox Grouping by Project

**Priority: P2**

#### Overview

The Agents/Inbox view renders all items in a flat chronological list. When a user is working with multiple projects, notifications from different projects are interleaved. Grouping inbox items by project name (mirroring the Projects view layout) makes it faster to process notifications for a specific context.

#### User Stories

- As a developer, I want inbox items grouped by project name so that I can process all notifications for one project before moving to the next.
- As a developer, I want to collapse a project group in the inbox to reduce visual noise from projects I am not currently focused on.

#### Acceptance Criteria

- The Agents view offers a toggle between "Flat" and "By project" display modes, persisted in localStorage.
- In "By project" mode, items are grouped under project name headings, ordered by most-recent activity within each group.
- Each group heading shows the project name and an unread count badge.
- Clicking a group heading collapses or expands the group; collapsed state persists in localStorage per project.
- The existing status filter (All / Unread / Read) applies within each group.

#### Technical Considerations

- Grouping is purely client-side: derive a `Map<projectName, InboxItem[]>` from `$inboxItems` using a Svelte reactive statement.
- No backend changes are required.
- Collapse state per project name can be stored in a `Map<string, boolean>` in a Svelte writable store backed by `localStorage`.

---

### F12 — Agent Session Tracking

**Priority: P2**

#### Overview

The inbox today is a notification feed — AI agents post items when they need a human decision. There is no concept of an agent "session": when Claude Code starts running on a project directory, when it pauses waiting for input, and when it completes. This information would let users see at a glance which projects have active agent sessions versus idle ones.

This is primarily a schema and daemon enhancement; the desktop surfaces whatever the schema provides.

#### User Stories

- As a developer, I want to see a visual indicator on a project section when an AI agent is actively running on it so that I know where to focus my attention.
- As a developer, I want the indicator to disappear when the agent session ends (either successfully or due to an error).

#### Acceptance Criteria

- A new `InboxItemKind` field (or a separate `agent_sessions` table) distinguishes between one-off notifications and session lifecycle events (`SessionStarted`, `SessionPaused`, `SessionCompleted`, `SessionFailed`).
- The desktop project header displays a colored dot (e.g., amber for active, green for completed) when the most recent session event for that project is `SessionStarted` or `SessionPaused`.
- Session lifecycle events are not shown in the flat notification list by default; they appear under an "Activity" sub-view.
- The `InboxItem` schema change is additive and backwards-compatible with existing rows.

#### Technical Considerations

- This requires a schema migration on `inbox.db`: add a `kind` column to `inbox_items` defaulting to `'notification'`.
- The CLI `inbox` subcommand needs new verbs: `session start`, `session complete`, `session fail`.
- The Rust `NewInboxItem` struct gains an optional `kind: InboxItemKind` field.
- Frontend changes are isolated to `InboxView.svelte` and `ProjectsView.svelte` (project header).

---

### F13 — Multi-Daemon Support

**Priority: P2**

#### Overview

`AppState::new()` reads `CANOPUS_IPC_SOCKET` from environment and constructs a single `JsonRpcClient`. There is no UI for adding a second daemon connection (e.g., a remote machine over an SSH tunnel or a second local daemon on a different socket path). Teams using Canopus on dev boxes, staging servers, or CI runners would benefit from a "connections" model similar to how database GUIs handle multiple servers.

#### User Stories

- As a developer, I want to add a named daemon connection (e.g., "Local", "Dev server") by specifying a socket path and optional auth token so that I can manage services on multiple machines.
- As a developer, I want to switch between connections without restarting the app.
- As a developer, I want the Projects view to scope to the selected connection.

#### Acceptance Criteria

- A "Connections" panel (accessible from the sidebar footer or a gear icon) lists configured connections. The default "Local" connection reads from `CANOPUS_IPC_SOCKET`.
- Adding a connection requires: a display name, a socket path (or `host:port` for TCP tunnels), and an optional bearer token. The config is saved to `~/.canopus/connections.json`.
- Switching connections re-initializes the `JsonRpcClient` and re-fetches the service list; existing log tails for the previous connection are closed.
- The sidebar shows the active connection name below the Canopus logo.
- Projects are stored per-connection in `~/.canopus/connections/<name>/projects.json`.

#### Technical Considerations

- `AppState` must become a runtime-mutable structure or the active connection must be stored in an `Arc<Mutex<AppState>>` that Tauri commands lock before use.
- The `JsonRpcClient` is already `Clone`; creating a new one for each switch is straightforward.
- TCP tunnel support (SSH port forwarding to the remote UDS socket over a local TCP port) does not require daemon changes; the user sets up the tunnel manually. The client connects to `127.0.0.1:<tunnelPort>` and the daemon authenticates via `CANOPUS_IPC_TOKEN`.

---

### F14 — Log Persistence Beyond the In-Memory Ring

**Priority: P2**

#### Overview

The `LogRing` in `service_task.rs` has a hardcoded capacity of 1 024 entries. When a service is restarted, its ring is discarded. When the daemon process itself restarts, all log history is lost. For debugging crash loops, production incidents, or CI failures, persistent log storage is essential.

#### User Stories

- As a developer, I want to view log output from a service's previous run (before the most recent restart) so that I can diagnose why it crashed.
- As a developer, I want log entries to survive a daemon restart so that I can inspect output from a session that ended before I opened the desktop app.

#### Acceptance Criteria

- Each service's log entries are written to `~/.canopus/logs/<service-id>/<date>.log` as plain text (one line per entry with ISO 8601 timestamp prefix).
- Log files are rotated daily and retained for a configurable number of days (default: 7).
- The `tailLogs` subscription can replay from file-backed history when the in-memory ring does not cover the requested `from_seq`.
- The desktop log viewer has a "Load history" button that fetches older log lines (paginated, 500 lines per request).
- A new `CANOPUS_LOG_RETENTION_DAYS` environment variable controls retention.

#### Technical Considerations

- The log writer runs as a side-effect in `spawn_log_reader` in `service_task.rs`: after pushing to the ring, append the line to an async file writer (using `tokio::fs::OpenOptions`).
- File rotation is handled by checking the date at each write and opening a new file when the date changes.
- Replay requires scanning the log directory for files within the requested sequence range; this implies adding a sequence number prefix to each line or maintaining a separate index file.
- This is a significant implementation task; the in-memory ring remains the primary path and file persistence is additive.

---

## Technical Considerations

### IPC Connection Lifecycle

The current `JsonRpcClient` opens a new UDS connection for every method call (each `connect_and_handshake()` invocation creates a fresh `UnixStream`). This means every `start_log_tail` and every `list_services` call pays the Unix socket connection and handshake overhead. For high-frequency operations and long-lived subscriptions this model is acceptable but it means the Tauri backend can accumulate many concurrent connections during log tailing. A connection-pooling or persistent-connection model should be considered for a v2 IPC refactor.

### Tauri Permissions Model

The current `capabilities/default.json` grants `core:default`, `core:event:default`, and `core:window:default`. Adding system tray (`tauri-plugin-tray`), desktop notifications (`tauri-plugin-notification`), and window state (`tauri-plugin-window-state`) each require additional capability declarations. Each new plugin must be added to both `Cargo.toml` and `capabilities/default.json`.

### Frontend State Architecture

The current `stores.ts` contains four flat writable stores (`services`, `projects`, `inboxItems`, `logs`). As features accumulate, a more structured state model may be warranted. However, Svelte 4's store model is sufficient for the features described in this document. Migration to Svelte 5 runes is a future consideration.

### Daemon Version Compatibility

The `canopus.handshake` response already includes the daemon `version` string. The desktop app should validate compatibility on connect and display a clear warning if the daemon version is too old to support a requested feature (e.g., `canopus.getSpec` requires daemon >= 0.2.0). Version checks should be non-blocking for features that degrade gracefully.

### Security

- The `CANOPUS_IPC_TOKEN` bearer token prevents unauthorized connections to the UDS socket. The desktop app should surface token configuration in the Connections UI.
- Environment variables in the `ServiceSpec` may contain secrets. The spec inspector (F6) must mask sensitive values by default.
- `projects.json` and `connections.json` are written to `~/.canopus/` with no encryption; this is consistent with how tools like `~/.ssh/config` and `~/.kube/config` handle local secrets on developer machines.

### Testing

- New Tauri commands should have unit tests using Tauri's built-in test harness where the `AppState` can be constructed with an in-memory `SqliteStore` and a mock `JsonRpcClient`.
- Frontend components should be tested with Vitest + `@testing-library/svelte` for interaction logic (drag-and-drop, filter, bulk actions).
- E2E tests for desktop features are out of scope for this document given the known flakiness of the existing E2E test suite.

---

## Open Questions

1. **Persistent connection vs. per-call UDS connection.** Should `JsonRpcClient` maintain a persistent connection for the lifetime of the app, or continue with per-call connections? A persistent connection simplifies event subscriptions but requires reconnection logic; per-call connections are simpler but incur handshake overhead. Decision needed before implementing F2.

2. **`tauri-plugin-notification` vs. `osascript` for inbox notifications.** The existing `DesktopNotifier` uses `osascript` on macOS and `notify-rust` on Linux. `tauri-plugin-notification` provides a unified API but requires additional capability grants. Which path should be standardized going forward?

3. **Should log history (F14) use a structured format or plain text?** Plain text is human-readable and trivially grep-able. A structured format (JSON Lines, or SQLite) would enable richer queries from the desktop. The ring buffer already has typed `LogEntry` structs. What is the intended use case for persisted logs — terminal grep, or GUI query?

4. **Agent session tracking schema design (F12).** Should agent sessions be a separate `agent_sessions` table, or should `inbox_items` gain a `kind` discriminator column? A separate table avoids a migration on the existing items table but adds a join for combined queries. Input from the team on the intended query patterns would resolve this.

5. **Remote daemon connectivity (F13).** The daemon's UDS socket is a local file path. Connecting to a remote machine requires SSH port forwarding or a TCP-based IPC mode. Does the team want to support native TCP for daemon-to-client IPC (analogous to the existing TCP control plane on port 49384), or is SSH tunneling sufficient as a user-managed concern?

6. **Tauri window close behavior.** Should closing the main window quit the app or minimize to tray? Many service-management tools (Docker Desktop, Postgres.app) minimize to tray by default. This behavior should be configurable in a Preferences panel but requires a decision on the default.

7. **Windows support timeline.** Process management is Unix-only (`core/src/process/unix.rs`) and the IPC server returns an error on Windows (`Windows Named Pipe server not yet implemented`). Is Windows support in scope for any near-term release, and should desktop features be designed to degrade gracefully on Windows or to block on Windows parity?
