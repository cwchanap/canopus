// ── Canopus service types ────────────────────────────────────────────────────

export type ServiceState =
  | "idle"
  | "spawning"
  | "starting"
  | "ready"
  | "stopping";

export interface ServiceSummary {
  id: string;
  name: string;
  state: ServiceState;
  pid?: number;
  port?: number;
  hostname?: string;
}

export type ServiceDetail = ServiceSummary;

// ── Project types ─────────────────────────────────────────────────────────────

export interface Project {
  name: string;
  serviceIds: string[];
}

export interface ProjectConfig {
  projects: Project[];
}

// ── Inbox types ───────────────────────────────────────────────────────────────

export type InboxStatus = "unread" | "read" | "dismissed";
export type SourceAgent =
  | "claudeCode"
  | "codex"
  | "windsurf"
  | "openCode"
  | "other";

export interface InboxItem {
  id: string;
  projectName: string;
  statusSummary: string;
  actionRequired: string;
  sourceAgent: SourceAgent;
  details?: unknown;
  status: InboxStatus;
  createdAt: string;
  updatedAt: string;
  dismissedAt?: string;
  notified: boolean;
}

export interface InboxFilter {
  status?: InboxStatus;
  sourceAgent?: SourceAgent;
  project?: string;
  limit?: number;
}

// ── Log event (Tauri event payload) ──────────────────────────────────────────

export interface LogEvent {
  serviceId: string;
  stream: "stdout" | "stderr";
  content: string;
  timestamp: string;
}
