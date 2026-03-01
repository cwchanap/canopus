<script lang="ts">
  import { onDestroy } from "svelte";
  import { dismissInboxItem, markInboxRead } from "../api";
  import { inboxItems, inboxUnreadCount } from "../stores";
  import type { InboxItem, InboxStatus } from "../types";
  import { extractErrorMessage } from "../utils";

  export let item: InboxItem;
  export let statusFilter: InboxStatus | "all" = "all";

  let actionError = "";
  let actionErrorTimeout: ReturnType<typeof setTimeout> | undefined;
  // Prevents concurrent read/dismiss actions on the same item (e.g. rapid double-click).
  let acting = false;

  function setActionError(msg: string) {
    clearTimeout(actionErrorTimeout);
    actionError = msg;
    actionErrorTimeout = setTimeout(() => { actionError = ""; }, 4000);
  }

  onDestroy(() => clearTimeout(actionErrorTimeout));

  const agentLabel: Record<string, string> = {
    claudeCode: "Claude Code",
    codex: "Codex CLI",
    windsurf: "Windsurf",
    openCode: "OpenCode",
    other: "Agent",
  };

  const agentColor: Record<string, string> = {
    claudeCode: "#f97316",
    codex: "#3b82f6",
    windsurf: "#06b6d4",
    openCode: "#8b5cf6",
    other: "#64748b",
  };

  function timeAgo(iso: string): string {
    const diff = Date.now() - new Date(iso).getTime();
    const min = Math.floor(diff / 60000);
    const hr = Math.floor(min / 60);
    const day = Math.floor(hr / 24);
    if (day > 0) return `${day}d ago`;
    if (hr > 0) return `${hr}h ago`;
    if (min > 0) return `${min}m ago`;
    return "just now";
  }

  async function read() {
    if (acting || item.status !== "unread") return;
    acting = true;
    try {
      await markInboxRead(item.id);
      // Decrement the global unread badge — checked before the await so the
      // status is still "unread" and we only decrement once per item.
      inboxUnreadCount.update((c) => Math.max(0, c - 1));
      if (statusFilter === "unread") {
        // Remove item from the unread list — it no longer satisfies the filter.
        inboxItems.update((items) => items.filter((i) => i.id !== item.id));
      } else {
        inboxItems.update((items) =>
          items.map((i) => (i.id === item.id ? { ...i, status: "read" } : i)),
        );
      }
    } catch (e) {
      setActionError(extractErrorMessage(e));
    } finally {
      acting = false;
    }
  }

  async function dismiss() {
    if (acting) return;
    acting = true;
    try {
      await dismissInboxItem(item.id);
      // Decrement the global unread badge if the dismissed item was unread.
      if (item.status === "unread") {
        inboxUnreadCount.update((c) => Math.max(0, c - 1));
      }
      inboxItems.update((items) => items.filter((i) => i.id !== item.id));
    } catch (e) {
      setActionError(extractErrorMessage(e));
    } finally {
      acting = false;
    }
  }
</script>

<div
  class="item"
  class:unread={item.status === "unread"}
  role="button"
  tabindex="0"
  aria-label="Mark {item.projectName} as read"
  on:click={read}
  on:keydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); read(); } }}
>
  <div class="agent-badge" style:background={agentColor[item.sourceAgent] ?? "#64748b"}>
    {agentLabel[item.sourceAgent] ?? "Agent"}
  </div>

  <div class="body">
    <div class="top">
      <span class="project">{item.projectName}</span>
      <span class="time">{timeAgo(item.createdAt)}</span>
    </div>
    <p class="summary">{item.statusSummary}</p>
    <p class="action">{item.actionRequired}</p>
  </div>

  <button
    class="dismiss-btn"
    title="Dismiss"
    on:click|stopPropagation={dismiss}
    on:keydown|stopPropagation={() => {}}
  >✕</button>

  {#if actionError}
    <div class="item-error">{actionError}</div>
  {/if}
</div>

<style>
  .item {
    display: flex;
    gap: 12px;
    padding: 14px 16px;
    border-bottom: 1px solid #1e2130;
    cursor: pointer;
    transition: background 0.15s;
    position: relative;
  }

  .item:hover {
    background: #1a1d27;
  }

  .item:focus-visible {
    outline: 2px solid #7c3aed;
    outline-offset: -2px;
  }

  .item.unread {
    border-left: 3px solid #7c3aed;
  }

  .agent-badge {
    flex-shrink: 0;
    align-self: flex-start;
    margin-top: 2px;
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    color: white;
    opacity: 0.9;
    white-space: nowrap;
  }

  .body {
    flex: 1;
    min-width: 0;
  }

  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 4px;
  }

  .project {
    font-size: 12px;
    font-weight: 500;
    color: #94a3b8;
  }

  .time {
    font-size: 11px;
    color: #475569;
    flex-shrink: 0;
  }

  .summary {
    font-size: 13px;
    color: #e2e8f0;
    margin: 0 0 3px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .action {
    font-size: 12px;
    color: #64748b;
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dismiss-btn {
    align-self: center;
    background: none;
    border: none;
    color: #334155;
    cursor: pointer;
    font-size: 12px;
    padding: 4px;
    border-radius: 4px;
    flex-shrink: 0;
    transition: color 0.15s;
  }

  .dismiss-btn:hover {
    color: #64748b;
  }

  .item-error {
    font-size: 11px;
    color: #ef4444;
    padding: 2px 16px 4px;
  }
</style>
