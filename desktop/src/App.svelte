<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { activeView, appendLog } from "./lib/stores";
  import { onLogUpdate } from "./lib/api";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import ProjectsView from "./lib/components/ProjectsView.svelte";
  import InboxView from "./lib/components/InboxView.svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let unlistenLog: UnlistenFn | undefined;

  onMount(async () => {
    try {
      unlistenLog = await onLogUpdate(appendLog);
    } catch (e) {
      console.error("Failed to register log-update listener:", e);
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
