import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  InboxFilter,
  InboxItem,
  LogEvent,
  ProjectConfig,
  ServiceSummary,
} from "./types";

// ── Services ──────────────────────────────────────────────────────────────────

export const listServices = (): Promise<ServiceSummary[]> =>
  invoke("list_services");

export const startService = (
  serviceId: string,
  port?: number,
  hostname?: string,
): Promise<void> => invoke("start_service", { serviceId, port, hostname });

export const stopService = (serviceId: string): Promise<void> =>
  invoke("stop_service", { serviceId });

export const restartService = (serviceId: string): Promise<void> =>
  invoke("restart_service", { serviceId });

export const startLogTail = (serviceId: string): Promise<void> =>
  invoke("start_log_tail", { serviceId });

export const stopLogTail = (serviceId: string): Promise<void> =>
  invoke("stop_log_tail", { serviceId });

export const onLogUpdate = (
  handler: (event: LogEvent) => void,
): Promise<UnlistenFn> =>
  listen<LogEvent>("log-update", (e) => handler(e.payload));

// ── Projects ──────────────────────────────────────────────────────────────────

export const listProjects = (): Promise<ProjectConfig> =>
  invoke("list_projects");

export const saveProjects = (config: ProjectConfig): Promise<void> =>
  invoke("save_projects", { config });

// ── Inbox ─────────────────────────────────────────────────────────────────────

export const listInbox = (filter?: InboxFilter): Promise<InboxItem[]> =>
  invoke("list_inbox", { filter });

export const markInboxRead = (id: string): Promise<void> =>
  invoke("mark_inbox_read", { id });

export const dismissInboxItem = (id: string): Promise<void> =>
  invoke("dismiss_inbox_item", { id });
