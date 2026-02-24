<script lang="ts">
  import { activeView } from "../stores";
  import { inboxItems } from "../stores";
  import { derived } from "svelte/store";

  const unreadCount = derived(
    inboxItems,
    ($items) => $items.filter((i) => i.status === "unread").length,
  );
</script>

<aside class="sidebar">
  <div class="logo">
    <span class="logo-icon">⬡</span>
    <span class="logo-text">Canopus</span>
  </div>

  <nav>
    <button
      class="nav-item"
      class:active={$activeView === "projects"}
      on:click={() => activeView.set("projects")}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="3" y="3" width="7" height="7" rx="1" />
        <rect x="14" y="3" width="7" height="7" rx="1" />
        <rect x="3" y="14" width="7" height="7" rx="1" />
        <rect x="14" y="14" width="7" height="7" rx="1" />
      </svg>
      Projects
    </button>

    <button
      class="nav-item"
      class:active={$activeView === "inbox"}
      on:click={() => activeView.set("inbox")}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z" />
        <polyline points="22,6 12,13 2,6" />
      </svg>
      Agents
      {#if $unreadCount > 0}
        <span class="badge">{$unreadCount}</span>
      {/if}
    </button>
  </nav>
</aside>

<style>
  .sidebar {
    width: 200px;
    background: #0f1117;
    border-right: 1px solid #1e2130;
    display: flex;
    flex-direction: column;
    padding: 16px 0;
    flex-shrink: 0;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 20px 24px;
    color: #e2e8f0;
    font-weight: 600;
    font-size: 15px;
  }

  .logo-icon {
    font-size: 20px;
    color: #7c3aed;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 8px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    border-radius: 6px;
    background: none;
    border: none;
    color: #94a3b8;
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s, color 0.15s;
    position: relative;
  }

  .nav-item svg {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }

  .nav-item:hover {
    background: #1e2130;
    color: #e2e8f0;
  }

  .nav-item.active {
    background: #1e2130;
    color: #a78bfa;
  }

  .badge {
    margin-left: auto;
    background: #7c3aed;
    color: white;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 10px;
    min-width: 18px;
    text-align: center;
  }
</style>
