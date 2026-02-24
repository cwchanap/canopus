<script lang="ts">
  import { afterUpdate, onDestroy } from "svelte";
  import { stopLogTail } from "../api";
  import { clearLogs, logPanelServiceId, logs } from "../stores";

  export let serviceId: string;

  let autoScroll = true;
  let container: HTMLDivElement;

  $: lines = $logs[serviceId] ?? [];

  afterUpdate(() => {
    if (autoScroll && container) {
      container.scrollTop = container.scrollHeight;
    }
  });

  async function close() {
    try {
      await stopLogTail(serviceId);
    } finally {
      clearLogs(serviceId);
      logPanelServiceId.set(null);
    }
  }

  onDestroy(() => {
    stopLogTail(serviceId).catch(console.error);
  });
</script>

<div class="log-panel">
  <div class="log-header">
    <span class="log-title">
      <span class="dot"></span>
      {serviceId} — logs
    </span>
    <div class="log-controls">
      <label class="autoscroll-toggle">
        <input type="checkbox" bind:checked={autoScroll} />
        Auto-scroll
      </label>
      <button class="close-btn" on:click={close}>✕</button>
    </div>
  </div>

  <div class="log-body" bind:this={container}>
    {#if lines.length === 0}
      <p class="empty">Waiting for output…</p>
    {:else}
      {#each lines as line (line.timestamp + line.content)}
        <div class="log-line" class:stderr={line.stream === "stderr"}>
          <span class="ts">{line.timestamp.slice(11, 19)}</span>
          <span class="content">{line.content}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .log-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #0a0c12;
    border-left: 1px solid #1e2130;
  }

  .log-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    border-bottom: 1px solid #1e2130;
    flex-shrink: 0;
  }

  .log-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: #94a3b8;
    font-family: monospace;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #22c55e;
  }

  .log-controls {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .autoscroll-toggle {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: #64748b;
    cursor: pointer;
  }

  .close-btn {
    background: none;
    border: none;
    color: #475569;
    cursor: pointer;
    font-size: 13px;
    padding: 2px 6px;
    border-radius: 4px;
    transition: color 0.15s;
  }

  .close-btn:hover {
    color: #94a3b8;
  }

  .log-body {
    flex: 1;
    overflow-y: auto;
    padding: 8px 0;
    font-family: "JetBrains Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 12px;
    line-height: 1.6;
  }

  .empty {
    color: #475569;
    text-align: center;
    margin-top: 40px;
    font-size: 12px;
  }

  .log-line {
    display: flex;
    gap: 10px;
    padding: 0 16px;
    min-height: 20px;
  }

  .log-line:hover {
    background: #0f1117;
  }

  .log-line.stderr .content {
    color: #fca5a5;
  }

  .ts {
    color: #334155;
    flex-shrink: 0;
    user-select: none;
  }

  .content {
    color: #94a3b8;
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
