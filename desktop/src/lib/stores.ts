import { writable } from "svelte/store";
import type { InboxItem, LogEvent, Project, ServiceSummary } from "./types";

// Keep these as writable stores for now for backward compatibility
// Components will gradually migrate to using the rune-based state below
export const services = writable<ServiceSummary[]>([]);
export const projects = writable<Project[]>([]);
export const inboxItems = writable<InboxItem[]>([]);
export const inboxUnreadCount = writable<number>(0);
export const logs = writable<Record<string, LogEvent[]>>({});
export const activeView = writable<"projects" | "inbox">("projects");
export const logPanelServiceId = writable<string | null>(null);

export function appendLog(event: LogEvent) {
  logs.update((current) => {
    const existing = current[event.serviceId] ?? [];
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
