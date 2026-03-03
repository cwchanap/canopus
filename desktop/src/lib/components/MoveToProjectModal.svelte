<script lang="ts">
  import type { Project } from "../types";

  export let projects: Project[];
  export let currentProjectName: string | null;
  export let serviceName: string;
  export let onConfirm: (targetProjectName: string | null) => void;
  export let onClose: () => void;

  // "__none__" is a sentinel for "Other Services" (no project)
  const NONE = "__none__";

  let selected: string = currentProjectName ?? NONE;
  let showNewProject = false;
  let newProjectName = "";
  let newProjectInput: HTMLInputElement;

  function toggleNewProject() {
    showNewProject = !showNewProject;
    if (showNewProject) {
      selected = "__new__";
      setTimeout(() => newProjectInput?.focus(), 0);
    } else {
      if (selected === "__new__") selected = currentProjectName ?? NONE;
      newProjectName = "";
    }
  }

  function confirm() {
    if (selected === "__new__") {
      const name = newProjectName.trim();
      if (!name) return;
      onConfirm(name);
    } else if (selected === NONE) {
      onConfirm(null);
    } else {
      onConfirm(selected);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
    if (e.key === "Enter" && selected !== "__new__") confirm();
    if (e.key === "Enter" && selected === "__new__") confirm();
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="overlay" on:click|self={onClose}>
  <div class="modal" role="dialog" aria-modal="true" aria-label="Move {serviceName} to project">
    <div class="modal-header">
      <span class="modal-title">Move to project</span>
      <button class="btn-close" on:click={onClose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <p class="service-name">{serviceName}</p>

      <div class="options">
        <!-- "No project" option (Other Services) -->
        <label class="option">
          <input type="radio" bind:group={selected} value={NONE} />
          <span class="option-label">Other Services <span class="option-hint">(ungrouped)</span></span>
        </label>

        {#each projects as project}
          <label class="option">
            <input type="radio" bind:group={selected} value={project.name} />
            <span class="option-label">{project.name}</span>
          </label>
        {/each}

        <!-- New project option -->
        {#if showNewProject}
          <label class="option option-new">
            <input type="radio" bind:group={selected} value="__new__" />
            <input
              class="new-project-input"
              type="text"
              placeholder="New project name…"
              bind:value={newProjectName}
              bind:this={newProjectInput}
              on:click|stopPropagation={() => { selected = "__new__"; }}
            />
          </label>
        {:else}
          <button class="btn-add-project" on:click={toggleNewProject}>
            + New project
          </button>
        {/if}
      </div>
    </div>

    <div class="modal-footer">
      <button class="btn btn-cancel" on:click={onClose}>Cancel</button>
      <button
        class="btn btn-confirm"
        disabled={selected === "__new__" && !newProjectName.trim()}
        on:click={confirm}
      >
        Move
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: #1a1d27;
    border: 1px solid #2a2f45;
    border-radius: 10px;
    width: 320px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    gap: 0;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px;
    border-bottom: 1px solid #1e2130;
  }

  .modal-title {
    font-size: 13px;
    font-weight: 600;
    color: #e2e8f0;
  }

  .btn-close {
    background: none;
    border: none;
    color: #475569;
    font-size: 12px;
    cursor: pointer;
    padding: 2px 4px;
    line-height: 1;
  }

  .btn-close:hover {
    color: #94a3b8;
  }

  .modal-body {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .service-name {
    font-size: 12px;
    color: #64748b;
    margin: 0;
    font-family: monospace;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .option {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.1s;
  }

  .option:hover {
    background: #1e2130;
  }

  .option input[type="radio"] {
    accent-color: #7c3aed;
    flex-shrink: 0;
  }

  .option-label {
    font-size: 13px;
    color: #e2e8f0;
  }

  .option-hint {
    font-size: 11px;
    color: #475569;
  }

  .option-new {
    gap: 8px;
  }

  .new-project-input {
    background: #0f1117;
    border: 1px solid #2a2f45;
    border-radius: 5px;
    color: #e2e8f0;
    font-size: 12px;
    padding: 4px 8px;
    outline: none;
    flex: 1;
  }

  .new-project-input:focus {
    border-color: #7c3aed;
  }

  .btn-add-project {
    background: none;
    border: 1px dashed #2a2f45;
    border-radius: 6px;
    color: #475569;
    font-size: 12px;
    padding: 6px 10px;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.15s, color 0.15s;
    margin-top: 4px;
  }

  .btn-add-project:hover {
    border-color: #7c3aed;
    color: #a78bfa;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid #1e2130;
  }

  .btn {
    padding: 6px 16px;
    border-radius: 5px;
    font-size: 12px;
    cursor: pointer;
    border: 1px solid transparent;
    transition: background 0.15s;
  }

  .btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .btn-cancel {
    background: none;
    border-color: #2a2f45;
    color: #64748b;
  }

  .btn-cancel:hover {
    background: #1e2130;
    color: #94a3b8;
  }

  .btn-confirm {
    background: #7c3aed;
    color: white;
    border-color: #7c3aed;
  }

  .btn-confirm:hover:not(:disabled) {
    background: #6d28d9;
  }
</style>
