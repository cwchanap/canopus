import { writable } from "svelte/store";
import type { InboxItem, LogEvent, Project, ServiceSummary } from "./types";

export const services = writable<ServiceSummary[]>([]);
export const projects = writable<Project[]>([]);
export const inboxItems = writable<InboxItem[]>([]);

// Per-service log lines, keyed by service ID.
export const logs = writable<Record<string, LogEvent[]>>({});

export function appendLog(event: LogEvent) {
  logs.update((current) => {
    const existing = current[event.serviceId] ?? [];
    // Keep the last 500 lines per service to avoid unbounded memory growth.
    const updated = [...existing, event].slice(-500);
    return { ...current, [event.serviceId]: updated };
  });
}

export function clearLogs(serviceId: string) {
  logs.update((current) => {
    const next = { ...current };
    delete next[serviceId];
    return next;
  });
}

// Currently active view: "projects" | "inbox"
export const activeView = writable<"projects" | "inbox">("projects");

// Which service's logs are being shown (null = none).
export const logPanelServiceId = writable<string | null>(null);
