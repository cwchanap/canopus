<script lang="ts">
  import { onMount } from "svelte";
  import { listProjects, listServices, saveProjects, startService, stopService } from "../api";
  import { logPanelServiceId, projects, services } from "../stores";
  import type { Project, ServiceSummary } from "../types";
  import { extractErrorMessage } from "../utils";
  import LogViewer from "./LogViewer.svelte";
  import MoveToProjectModal from "./MoveToProjectModal.svelte";
  import ServiceCard from "./ServiceCard.svelte";

  let loading = true;
  let error = "";
  let showAddProject = false;
  let newProjectName = "";
  let refreshInterval: ReturnType<typeof setInterval> | undefined;
  let isLoading = false;
  // Tracks project names with an active bulk start/stop operation
  let bulkLoading = new Set<string>();

  // Move-to-project modal state
  let moveService: ServiceSummary | null = null;
  let moveServiceCurrentProject: string | null = null;

  // Inline rename state
  let renamingProject: string | null = null;
  let renameValue = "";
  let renameInput: HTMLInputElement;

  // Project header overflow menu
  let openHeaderMenu: string | null = null;

  // Delete confirmation
  let deletingProject: string | null = null;

  async function load() {
    if (isLoading) return;
    isLoading = true;
    error = "";
    try {
      const [svcResult, projResult] = await Promise.allSettled([listServices(), listProjects()]);
      if (svcResult.status === 'rejected') {
        throw svcResult.reason;
      }
      services.set(svcResult.value);
      if (projResult.status === 'fulfilled') {
        projects.set(projResult.value.projects);
      } else {
        throw projResult.reason;
      }
    } catch (e) {
      error = extractErrorMessage(e);
    } finally {
      loading = false;
      isLoading = false;
    }
  }

  function getProjectServices(project: Project): ServiceSummary[] {
    return $services.filter((s) => project.serviceIds.includes(s.id));
  }

  $: ungrouped = (() => {
    const grouped = new Set($projects.flatMap((p) => p.serviceIds));
    return $services.filter((s) => !grouped.has(s.id));
  })();

  function idleServices(project: Project): ServiceSummary[] {
    return getProjectServices(project).filter((s) => s.state === "idle");
  }

  function runningServices(project: Project): ServiceSummary[] {
    return getProjectServices(project).filter(
      (s) => s.state === "ready" || s.state === "starting" || s.state === "spawning"
    );
  }

  async function startAll(project: Project) {
    const ids = idleServices(project).map((s) => s.id);
    if (ids.length === 0) return;
    bulkLoading = new Set([...bulkLoading, project.name]);
    try {
      const results = await Promise.allSettled(ids.map((id) => startService(id)));
      const failed = results.filter((r) => r.status === "rejected");
      if (failed.length > 0) {
        error = `${failed.length} service(s) failed to start.`;
      }
    } finally {
      bulkLoading = new Set([...bulkLoading].filter((n) => n !== project.name));
      load();
    }
  }

  async function stopAll(project: Project) {
    const ids = runningServices(project).map((s) => s.id);
    if (ids.length === 0) return;
    bulkLoading = new Set([...bulkLoading, project.name]);
    try {
      const results = await Promise.allSettled(ids.map((id) => stopService(id)));
      const failed = results.filter((r) => r.status === "rejected");
      if (failed.length > 0) {
        error = `${failed.length} service(s) failed to stop.`;
      }
    } finally {
      bulkLoading = new Set([...bulkLoading].filter((n) => n !== project.name));
      load();
    }
  }

  async function addProject() {
    if (!newProjectName.trim()) return;
    const updated = [...$projects, { name: newProjectName.trim(), serviceIds: [] }];
    try {
      await saveProjects({ projects: updated });
      projects.set(updated);
      newProjectName = "";
      showAddProject = false;
    } catch (e) {
      console.error("Failed to save project:", e);
      error = extractErrorMessage(e);
    }
  }

  // ── Move-to-project logic ────────────────────────────────────────────────────

  function handleMoveRequest(service: ServiceSummary, currentProject: string | null) {
    moveService = service;
    moveServiceCurrentProject = currentProject;
  }

  async function handleMoveConfirm(targetProjectName: string | null) {
    if (!moveService) return;
    const serviceId = moveService.id;

    let updated: Project[];

    if (targetProjectName !== null) {
      // Case-insensitive match: treat as moving to an existing project
      const existingProject = $projects.find(
        (p) => p.name.toLowerCase() === targetProjectName.toLowerCase()
      );
      if (existingProject) {
        const canonicalName = existingProject.name;
        updated = $projects.map((p) => {
          if (p.name === canonicalName) {
            return { ...p, serviceIds: [...p.serviceIds.filter((id) => id !== serviceId), serviceId] };
          }
          return { ...p, serviceIds: p.serviceIds.filter((id) => id !== serviceId) };
        });
      } else {
        // Genuinely new project
        updated = [
          ...$projects.map((p) => ({ ...p, serviceIds: p.serviceIds.filter((id) => id !== serviceId) })),
          { name: targetProjectName, serviceIds: [serviceId] },
        ];
      }
    } else {
      // Move to "Other Services": remove from all projects
      updated = $projects.map((p) => ({
        ...p,
        serviceIds: p.serviceIds.filter((id) => id !== serviceId),
      }));
    }

    try {
      await saveProjects({ projects: updated });
      projects.set(updated);
      moveService = null;
      moveServiceCurrentProject = null;
    } catch (e) {
      error = extractErrorMessage(e);
    }
  }

  function closeMoveModal() {
    moveService = null;
    moveServiceCurrentProject = null;
  }

  // ── Inline project rename ────────────────────────────────────────────────────

  function startRename(project: Project) {
    renamingProject = project.name;
    renameValue = project.name;
    openHeaderMenu = null;
    setTimeout(() => renameInput?.focus(), 0);
  }

  async function commitRename() {
    if (!renamingProject) return;
    const trimmed = renameValue.trim();
    if (!trimmed || trimmed === renamingProject) {
      renamingProject = null;
      return;
    }
    const updated = $projects.map((p) =>
      p.name === renamingProject ? { ...p, name: trimmed } : p
    );
    try {
      await saveProjects({ projects: updated });
      projects.set(updated);
    } catch (e) {
      error = extractErrorMessage(e);
    } finally {
      renamingProject = null;
    }
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") commitRename();
    if (e.key === "Escape") renamingProject = null;
  }

  // ── Project header overflow menu ─────────────────────────────────────────────

  function toggleHeaderMenu(projectName: string, e: MouseEvent) {
    e.stopPropagation();
    openHeaderMenu = openHeaderMenu === projectName ? null : projectName;
  }

  function closeAllMenus() {
    openHeaderMenu = null;
  }

  // ── Project delete ────────────────────────────────────────────────────────────

  function requestDelete(projectName: string) {
    openHeaderMenu = null;
    deletingProject = projectName;
  }

  async function confirmDelete() {
    if (!deletingProject) return;
    const updated = $projects.filter((p) => p.name !== deletingProject);
    try {
      await saveProjects({ projects: updated });
      projects.set(updated);
      deletingProject = null;
    } catch (e) {
      error = extractErrorMessage(e);
    }
  }

  onMount(() => {
    load();
    refreshInterval = setInterval(load, 5000);
    return () => {
      if (refreshInterval !== undefined) clearInterval(refreshInterval);
    };
  });
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="projects-layout" on:click={closeAllMenus}>
  <div class="main-content">
    {#if loading}
      <div class="empty-state">Loading…</div>
    {:else if error}
      <div class="empty-state error">
        <p>Could not connect to Canopus daemon.</p>
        <p class="error-detail">{error}</p>
        <button class="btn-retry" on:click={load}>Retry</button>
      </div>
    {:else}
      <div class="projects-list">
        {#each $projects as project}
          {@const svcList = getProjectServices(project)}
          <section class="project-section">
            <div class="project-header">
              {#if renamingProject === project.name}
                <input
                  class="rename-input"
                  type="text"
                  bind:value={renameValue}
                  bind:this={renameInput}
                  on:blur={commitRename}
                  on:keydown={handleRenameKeydown}
                />
              {:else}
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <h2
                  class="project-name"
                  on:dblclick={() => startRename(project)}
                  title="Double-click to rename"
                >
                  {project.name}
                </h2>
              {/if}

              <div class="project-header-actions">
                <div class="overflow-wrap">
                  <button
                    class="btn-overflow"
                    on:click={(e) => toggleHeaderMenu(project.name, e)}
                    aria-label="Project options"
                  >⋯</button>
                  {#if openHeaderMenu === project.name}
                    <div class="overflow-menu">
                      <button class="overflow-item" on:click={() => startRename(project)}>
                        Rename
                      </button>
                      <button class="overflow-item overflow-item-danger" on:click={() => requestDelete(project.name)}>
                        Delete project
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            </div>

            {#if svcList.length === 0}
              <p class="no-services">No services assigned.</p>
            {:else}
              <div class="service-grid">
                {#each svcList as service}
                  <ServiceCard
                    {service}
                    onRefresh={load}
                    projectName={project.name}
                    onMoveRequest={handleMoveRequest}
                    onMenuOpen={closeAllMenus}
                  />
                {/each}
              </div>
            {/if}
          </section>
        {/each}

        {#if ungrouped.length > 0}
          <section class="project-section">
            <div class="project-header">
              <h2 class="project-name ungrouped">Other Services</h2>
            </div>
            <div class="service-grid">
              {#each ungrouped as service}
                <ServiceCard
                  {service}
                  onRefresh={load}
                  projectName={null}
                  onMoveRequest={handleMoveRequest}
                  onMenuOpen={closeAllMenus}
                />
              {/each}
            </div>
          </section>
        {/if}

        {#if $projects.length === 0 && $services.length === 0}
          <div class="empty-state">
            <p>No services found.</p>
            <p class="hint">Start the Canopus daemon and register some services.</p>
          </div>
        {/if}

        <!-- Add project button -->
        {#if showAddProject}
          <div class="add-project-form">
            <input
              class="project-input"
              type="text"
              placeholder="Project name…"
              bind:value={newProjectName}
              on:keydown={(e) => e.key === "Enter" && addProject()}
              autofocus
            />
            <button class="btn-add" on:click={addProject}>Add</button>
            <button class="btn-cancel" on:click={() => (showAddProject = false)}>Cancel</button>
          </div>
        {:else}
          <button class="btn-new-project" on:click={() => (showAddProject = true)}>
            + New Project
          </button>
        {/if}
      </div>
    {/if}
  </div>

  {#if $logPanelServiceId}
    <div class="log-pane">
      {#key $logPanelServiceId}
        <LogViewer serviceId={$logPanelServiceId} />
      {/key}
    </div>
  {/if}
</div>

<!-- Move-to-project modal -->
{#if moveService}
  <MoveToProjectModal
    projects={$projects}
    currentProjectName={moveServiceCurrentProject}
    serviceName={moveService.name}
    onConfirm={handleMoveConfirm}
    onClose={closeMoveModal}
  />
{/if}

<!-- Delete confirmation dialog -->
{#if deletingProject}
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="overlay" on:click|self={() => (deletingProject = null)}>
    <div class="confirm-dialog" role="dialog" aria-modal="true">
      <p class="confirm-text">Delete project <strong>{deletingProject}</strong>?</p>
      <p class="confirm-hint">Services will be returned to Other Services.</p>
      <div class="confirm-actions">
        <button class="btn btn-cancel" on:click={() => (deletingProject = null)}>Cancel</button>
        <button class="btn btn-danger" on:click={confirmDelete}>Delete</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .projects-layout {
    display: flex;
    height: 100%;
    overflow: hidden;
  }

  .main-content {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    min-width: 0;
  }

  .log-pane {
    width: 420px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
  }

  .projects-list {
    display: flex;
    flex-direction: column;
    gap: 32px;
    max-width: 900px;
  }

  .project-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .project-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 24px;
  }

  .project-header-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .project-section:hover .project-header-actions {
    opacity: 1;
  }

  .project-name {
    font-size: 13px;
    font-weight: 600;
    color: #64748b;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
    cursor: default;
    user-select: none;
  }

  .project-name:hover {
    color: #94a3b8;
  }

  .project-name.ungrouped {
    color: #334155;
  }

  .rename-input {
    background: #1a1d27;
    border: 1px solid #7c3aed;
    border-radius: 5px;
    color: #e2e8f0;
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    padding: 2px 8px;
    outline: none;
    width: 200px;
  }

  .overflow-wrap {
    position: relative;
  }

  .btn-overflow {
    background: none;
    border: none;
    color: #475569;
    font-size: 14px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
    letter-spacing: 1px;
    border-radius: 4px;
    transition: color 0.1s, background 0.1s;
  }

  .btn-overflow:hover {
    color: #94a3b8;
    background: #1e2130;
  }

  .overflow-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    background: #1a1d27;
    border: 1px solid #2a2f45;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    z-index: 50;
    min-width: 160px;
    padding: 4px;
  }

  .overflow-item {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: #e2e8f0;
    font-size: 12px;
    padding: 7px 10px;
    cursor: pointer;
    border-radius: 4px;
    transition: background 0.1s;
  }

  .overflow-item:hover {
    background: #2a2f45;
  }

  .overflow-item-danger {
    color: #f87171;
  }

  .overflow-item-danger:hover {
    background: #ef444418;
  }

  .service-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }

  .no-services {
    font-size: 12px;
    color: #475569;
    margin: 0;
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

  .error-detail {
    font-family: monospace;
    font-size: 11px;
    color: #64748b;
    margin-top: 4px;
  }

  .hint {
    color: #334155;
    font-size: 11px;
    margin-top: 6px;
  }

  .btn-retry {
    margin-top: 12px;
    padding: 6px 16px;
    background: #1e2130;
    border: 1px solid #2a2f45;
    border-radius: 5px;
    color: #94a3b8;
    font-size: 12px;
    cursor: pointer;
  }

  .btn-new-project {
    align-self: flex-start;
    background: none;
    border: 1px dashed #2a2f45;
    border-radius: 6px;
    color: #475569;
    font-size: 12px;
    padding: 7px 14px;
    cursor: pointer;
    transition: border-color 0.15s, color 0.15s;
  }

  .btn-new-project:hover {
    border-color: #7c3aed;
    color: #a78bfa;
  }

  .add-project-form {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .project-input {
    background: #1a1d27;
    border: 1px solid #2a2f45;
    border-radius: 5px;
    color: #e2e8f0;
    font-size: 13px;
    padding: 6px 10px;
    outline: none;
    width: 200px;
  }

  .project-input:focus {
    border-color: #7c3aed;
  }

  .btn-add {
    padding: 6px 14px;
    background: #7c3aed;
    border: none;
    border-radius: 5px;
    color: white;
    font-size: 12px;
    cursor: pointer;
  }

  .btn-cancel {
    padding: 6px 14px;
    background: none;
    border: 1px solid #2a2f45;
    border-radius: 5px;
    color: #64748b;
    font-size: 12px;
    cursor: pointer;
  }

  /* Delete confirmation overlay */
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .confirm-dialog {
    background: #1a1d27;
    border: 1px solid #2a2f45;
    border-radius: 10px;
    padding: 24px;
    width: 320px;
    max-width: 90vw;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .confirm-text {
    font-size: 13px;
    color: #e2e8f0;
    margin: 0 0 6px;
  }

  .confirm-hint {
    font-size: 12px;
    color: #64748b;
    margin: 0 0 20px;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .btn {
    padding: 6px 16px;
    border-radius: 5px;
    font-size: 12px;
    cursor: pointer;
    border: 1px solid transparent;
    transition: background 0.15s;
  }

  .btn-danger {
    background: #ef4444;
    color: white;
    border-color: #ef4444;
  }

  .btn-danger:hover {
    background: #dc2626;
  }
</style>
