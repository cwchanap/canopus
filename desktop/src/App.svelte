<script lang="ts">
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { listInbox, onLogUpdate } from "./lib/api";
  import InboxView from "./lib/components/InboxView.svelte";
  import ProjectsView from "./lib/components/ProjectsView.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import { activeView, appendLog, inboxUnreadCount } from "./lib/stores";
  import "./app.css";

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

  $effect(() => {
    return () => {
      unlistenLog?.();
    };
  });
</script>

<div class="flex h-screen overflow-hidden bg-background text-foreground">
  <Sidebar />
  <main class="flex-1 overflow-hidden flex flex-col">
    {#if $activeView === "projects"}
      <ProjectsView />
    {:else}
      <InboxView />
    {/if}
  </main>
</div>
