<script lang="ts">
  import { restartService, startService, stopService } from "../api";
  import { logPanelServiceId } from "../stores";
  import type { ServiceSummary } from "../types";
  import { extractErrorMessage } from "../utils";
  import "./overflow.css";

  export let service: ServiceSummary;
  export let onRefresh: () => void;
  /** The name of the project this card currently belongs to, or null for ungrouped. */
  export let projectName: string | null = null;
  /** Called when the user requests to move the service; parent handles the modal. */
  export let onMoveRequest: ((service: ServiceSummary, projectName: string | null) => void) | null = null;
  /** Called when this card's overflow menu opens, so the parent can close other open menus. */
  export let onMenuOpen: (() => void) | null = null;
  export let onActionStart: ((serviceId: string) => void) | null = null;
  export let onActionEnd: ((serviceId: string) => void) | null = null;
  /** Incremented by parent to force-close any open overflow menu on this card. */
  export let closeSignal = 0;

  let showMenu = false;
  let lastCloseSignal = closeSignal;
  let menuEl: HTMLElement;

  let loading = false;
  let actionError = "";

  const stateColor: Record<string, string> = {
    ready: "#22c55e",
    starting: "#f59e0b",
    spawning: "#f59e0b",
    stopping: "#ef4444",
    idle: "#64748b",
  };

  async function handle(action: () => Promise<void>) {
    loading = true;
    actionError = "";
    onActionStart?.(service.id);
    try {
      await action();
      await new Promise<void>((resolve) => setTimeout(resolve, 400));
      onRefresh();
    } catch (e) {
      console.error(e);
      actionError = extractErrorMessage(e);
      setTimeout(() => { actionError = ""; }, 4000);
    } finally {
      onActionEnd?.(service.id);
      loading = false;
    }
  }

  function openLogs() {
    // LogViewer calls startLogTail in its own onMount; starting it here as well
    // would abort and recreate the tail, potentially dropping log lines emitted
    // in between the two calls.
    logPanelServiceId.set(service.id);
  }

  const isRunning = () =>
    service.state === "ready" ||
    service.state === "starting" ||
    service.state === "spawning" ||
    service.state === "stopping";

  $: {
    if (closeSignal !== lastCloseSignal) {
      lastCloseSignal = closeSignal;
      showMenu = false;
    }
  }

  function openMenu(e: MouseEvent) {
    e.stopPropagation();
    showMenu = !showMenu;
    if (showMenu) onMenuOpen?.();
    if (showMenu) {
      setTimeout(() => {
        const firstItem = menuEl?.querySelector('.overflow-item') as HTMLElement;
        firstItem?.focus();
      }, 0);
    }
  }

  function closeMenu() {
    showMenu = false;
  }

  function handleMenuKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      showMenu = false;
      return;
    }

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const menu = (e.target as HTMLElement).closest('.overflow-menu');
      if (!menu) return;

      const items = Array.from(menu.querySelectorAll('.overflow-item')) as HTMLElement[];
      const index = items.indexOf(e.target as HTMLElement);

      if (e.key === "ArrowDown") {
        const next = items[index + 1] || items[0];
        next?.focus();
      } else {
        const prev = items[index - 1] || items[items.length - 1];
        prev?.focus();
      }
    }
  }

  function requestMove(e: MouseEvent) {
    e.stopPropagation();
    showMenu = false;
    onMoveRequest?.(service, projectName);
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="card" on:click={closeMenu}>
  <div class="header">
    <div class="info">
      <span class="dot" style:background={stateColor[service.state] ?? "#64748b"}></span>
      <div>
        <span class="name">{service.name}</span>
        <span class="meta">{service.id}</span>
      </div>
    </div>
    <div class="header-right">
      <span class="state-label">{service.state}</span>
      {#if onMoveRequest}
        <div class="overflow-wrap">
          <button class="btn-overflow" on:click={openMenu} aria-label="More options" aria-haspopup="menu" aria-expanded={showMenu}>⋯</button>
          {#if showMenu}
            <div class="overflow-menu" bind:this={menuEl} on:keydown={handleMenuKeydown} role="menu" tabindex="-1">
              <button class="overflow-item" on:click={requestMove} role="menuitem" tabindex="-1">
                Move to project…
              </button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if service.pid || service.port || service.hostname}
    <div class="details">
      {#if service.pid}<span class="detail-tag">pid {service.pid}</span>{/if}
      {#if service.port}<span class="detail-tag">:{service.port}</span>{/if}
      {#if service.hostname}<span class="detail-tag">{service.hostname}</span>{/if}
    </div>
  {/if}

  <div class="actions">
    {#if isRunning()}
      <button class="btn btn-danger" disabled={loading || service.state === "stopping"} on:click={() => handle(() => stopService(service.id))}>
        Stop
      </button>
      <button class="btn btn-secondary" disabled={loading || service.state === "stopping"} on:click={() => handle(() => restartService(service.id))}>
        Restart
      </button>
    {:else}
      <button class="btn btn-primary" disabled={loading} on:click={() => handle(() => startService(service.id))}>
        Start
      </button>
    {/if}
    <button class="btn btn-ghost" on:click={openLogs}>
      Logs
    </button>
  </div>

  {#if actionError}
    <div class="action-error">{actionError}</div>
  {/if}
</div>

<style>
  .card {
    background: #1a1d27;
    border: 1px solid #1e2130;
    border-radius: 8px;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition: border-color 0.15s;
  }

  .card:hover {
    border-color: #2a2f45;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .overflow-menu {
    right: 0;
  }

  .info {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .name {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: #e2e8f0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .meta {
    display: block;
    font-size: 11px;
    color: #475569;
    font-family: monospace;
  }

  .state-label {
    font-size: 11px;
    color: #64748b;
    flex-shrink: 0;
  }

  .details {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .detail-tag {
    background: #0f1117;
    border: 1px solid #1e2130;
    border-radius: 4px;
    padding: 1px 7px;
    font-size: 11px;
    color: #64748b;
    font-family: monospace;
  }

  .actions {
    display: flex;
    gap: 6px;
  }

  .btn {
    padding: 4px 12px;
    border-radius: 5px;
    font-size: 12px;
    cursor: pointer;
    border: 1px solid transparent;
    transition: background 0.15s, color 0.15s;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: #7c3aed;
    color: white;
    border-color: #7c3aed;
  }

  .btn-primary:hover:not(:disabled) {
    background: #6d28d9;
  }

  .btn-danger {
    background: transparent;
    color: #ef4444;
    border-color: #ef4444;
  }

  .btn-danger:hover:not(:disabled) {
    background: #ef444422;
  }

  .btn-secondary {
    background: transparent;
    color: #94a3b8;
    border-color: #2a2f45;
  }

  .btn-secondary:hover:not(:disabled) {
    background: #1e2130;
    color: #e2e8f0;
  }

  .btn-ghost {
    background: transparent;
    color: #64748b;
    border-color: transparent;
  }

  .btn-ghost:hover {
    color: #94a3b8;
  }

  .action-error {
    font-size: 11px;
    color: #ef4444;
    margin-top: 2px;
  }
</style>
