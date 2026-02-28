<script lang="ts">
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { listInbox, onLogUpdate } from "./lib/api";
  import InboxView from "./lib/components/InboxView.svelte";
  import ProjectsView from "./lib/components/ProjectsView.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import { activeView, appendLog, inboxUnreadCount } from "./lib/stores";

  let unlistenLog: UnlistenFn | undefined;

  onMount(async () => {
    try {
      unlistenLog = await onLogUpdate(appendLog);
    } catch (e) {
      console.error("Failed to register log-update listener:", e);
    }
    // Seed the sidebar unread badge before the user opens Inbox.
    // Errors are non-fatal: the badge will update correctly once InboxView loads.
    try {
      const unread = await listInbox({ status: "unread" });
      inboxUnreadCount.set(unread.length);
    } catch (e) {
      console.error("Failed to fetch initial unread count:", e);
    }
  });

  onDestroy(() => {
    unlistenLog?.();
  });
</script>

<div class="app">
  <Sidebar />
  <main>
    {#if $activeView === "projects"}
      <ProjectsView />
    {:else}
      <InboxView />
    {/if}
  </main>
</div>

<style>
  :global(*, *::before, *::after) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }

  :global(body) {
    background: #13151f;
    color: #e2e8f0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
    overflow: hidden;
  }

  :global(::-webkit-scrollbar) {
    width: 6px;
  }

  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    background: #1e2130;
    border-radius: 3px;
  }

  .app {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  main {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
