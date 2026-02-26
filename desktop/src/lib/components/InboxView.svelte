<script lang="ts">
  import { listInbox } from "../api";
  import { inboxItems } from "../stores";
  import type { InboxStatus } from "../types";
  import { extractErrorMessage } from "../utils";
  import InboxItem from "./InboxItem.svelte";

  let loading = true;
  let error = "";
  let statusFilter: InboxStatus | "all" = "all";
  const filterOptions: ReadonlyArray<{ value: InboxStatus | "all"; label: string }> = [
    { value: "all", label: "All" },
    { value: "unread", label: "Unread" },
    { value: "read", label: "Read" },
  ];

  // Monotonically incrementing token used to discard stale in-flight responses
  // when statusFilter changes before a previous load() has completed.
  let currentRequestId = 0;

  async function load() {
    const requestId = ++currentRequestId;
    loading = true;
    error = "";
    try {
      const filter = statusFilter !== "all" ? { status: statusFilter } : undefined;
      const items = await listInbox(filter);
      if (requestId !== currentRequestId) return;
      inboxItems.set(items);
    } catch (e) {
      if (requestId !== currentRequestId) return;
      error = extractErrorMessage(e);
    } finally {
      if (requestId === currentRequestId) loading = false;
    }
  }

  // Reactive statement runs on init and whenever statusFilter changes.
  $: statusFilter, load();
</script>

<div class="inbox-view">
  <div class="inbox-header">
    <h1 class="title">Agent Inbox</h1>
    <div class="filters">
      {#each filterOptions as option (option.value)}
        <button
          class="filter-btn"
          class:active={statusFilter === option.value}
          aria-pressed={statusFilter === option.value}
          on:click={() => (statusFilter = option.value)}
        >
          {option.label}
        </button>
      {/each}
    </div>
  </div>

  {#if loading}
    <div class="empty-state">Loading…</div>
  {:else if error}
    <div class="empty-state error">{error}</div>
  {:else if $inboxItems.length === 0}
    <div class="empty-state">
      <p>No notifications.</p>
      <p class="hint">AI agents will post here when they need your attention.</p>
    </div>
  {:else}
    <div class="items-list">
      {#each $inboxItems as item (item.id)}
        <InboxItem {item} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .inbox-view {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .inbox-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px 16px;
    border-bottom: 1px solid #1e2130;
    flex-shrink: 0;
  }

  .title {
    font-size: 16px;
    font-weight: 600;
    color: #e2e8f0;
    margin: 0;
  }

  .filters {
    display: flex;
    gap: 4px;
  }

  .filter-btn {
    padding: 4px 12px;
    border-radius: 5px;
    border: 1px solid #1e2130;
    background: none;
    color: #64748b;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  .filter-btn:hover {
    background: #1e2130;
    color: #94a3b8;
  }

  .filter-btn.active {
    background: #1e2130;
    color: #a78bfa;
    border-color: #7c3aed;
  }

  .items-list {
    flex: 1;
    overflow-y: auto;
  }

  .empty-state {
    text-align: center;
    padding: 60px 20px;
    color: #475569;
    font-size: 13px;
  }

  .empty-state.error {
    color: #f87171;
  }

  .hint {
    color: #334155;
    font-size: 11px;
    margin-top: 6px;
  }
</style>
