<script lang="ts">
  import { startService, stopService, restartService, startLogTail } from "../api";
  import { logPanelServiceId } from "../stores";
  import type { ServiceSummary } from "../types";

  export let service: ServiceSummary;
  export let onRefresh: () => void;

  let loading = false;

  const stateColor: Record<string, string> = {
    Ready: "#22c55e",
    Starting: "#f59e0b",
    Spawning: "#f59e0b",
    Stopping: "#ef4444",
    Idle: "#64748b",
  };

  async function handle(action: () => Promise<void>) {
    loading = true;
    try {
      await action();
      setTimeout(onRefresh, 400);
    } catch (e) {
      console.error(e);
    } finally {
      loading = false;
    }
  }

  async function openLogs() {
    await startLogTail(service.id);
    logPanelServiceId.set(service.id);
  }

  const isRunning = () =>
    service.state === "Ready" || service.state === "Starting" || service.state === "Spawning";
</script>

<div class="card">
  <div class="header">
    <div class="info">
      <span class="dot" style:background={stateColor[service.state] ?? "#64748b"}></span>
      <div>
        <span class="name">{service.name}</span>
        <span class="meta">{service.id}</span>
      </div>
    </div>
    <span class="state-label">{service.state}</span>
  </div>

  {#if service.pid || service.port}
    <div class="details">
      {#if service.pid}<span class="detail-tag">pid {service.pid}</span>{/if}
      {#if service.port}<span class="detail-tag">:{service.port}</span>{/if}
      {#if service.hostname}<span class="detail-tag">{service.hostname}</span>{/if}
    </div>
  {/if}

  <div class="actions">
    {#if isRunning()}
      <button class="btn btn-danger" disabled={loading} on:click={() => handle(() => stopService(service.id))}>
        Stop
      </button>
      <button class="btn btn-secondary" disabled={loading} on:click={() => handle(() => restartService(service.id))}>
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
</style>
