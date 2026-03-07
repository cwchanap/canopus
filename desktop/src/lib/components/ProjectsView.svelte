<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { listProjects, listServices, saveProjects, startService, stopService } from "../api";
  import { logPanelServiceId, projects, services } from "../stores";
  import type { Project, ServiceSummary } from "../types";
  import { extractErrorMessage, isReservedProjectName } from "../utils";
  import LogViewer from "./LogViewer.svelte";
  import MoveToProjectModal from "./MoveToProjectModal.svelte";
  import ServiceCard from "./ServiceCard.svelte";
  import "./overflow.css";

  let loading = true;
  let error = "";
  let opError = "";
  let showAddProject = false;
  let newProjectName = "";
  let newProjectInput: HTMLInputElement;
  let refreshInterval: ReturnType<typeof setInterval> | undefined;
  let isLoading = false;
  let bulkStarting = new Set<string>();
  let bulkStopping = new Set<string>();

  // Bump this to force ServiceCard overflow menus closed.
  let serviceMenuCloseSignal = 0;

  // Move-to-project modal state
  let moveService: ServiceSummary | null = null;
  let moveServiceCurrentProject: string | null = null;
  let moveLoading = false;

  // Inline rename state
  let renamingProject: string | null = null;
  let renameValue = "";
  let renameInput: HTMLInputElement;
  let renameSaving = false;

  // Project header overflow menu
  let openHeaderMenu: string | null = null;
  let headerMenuEls: Record<string, HTMLElement> = {};

  // Delete confirmation
  let deletingProjectId: string | null = null;
  let deletingProject: Project | null = null;
  let deleteLoading = false;
  let projectSaveQueue: Promise<void> = Promise.resolve();

  function normalizeProjectName(name: string): string {
    return name.trim().toLowerCase();
  }

  function getProjectId(project: Pick<Project, "name">): string {
    return normalizeProjectName(project.name);
  }

  function createProjectCommandError(message: string) {
    return {
      code: "PROJ004",
      message,
    };
  }

  async function mutateProjects(
    updater: (currentProjects: Project[]) => Project[]
  ): Promise<Project[]> {
    let updatedProjects: Project[] = [];
    const run = projectSaveQueue.catch(() => undefined).then(async () => {
      const currentProjects = get(projects);
      updatedProjects = updater(currentProjects);
      await saveProjects({ projects: updatedProjects });
      projects.set(updatedProjects);
    });
    projectSaveQueue = run.then(() => undefined, () => undefined);
    await run;
    return updatedProjects;
  }

  $: deletingProject = deletingProjectId
    ? $projects.find((project) => getProjectId(project) === deletingProjectId) ?? null
    : null;

  function closeServiceMenus() {
    serviceMenuCloseSignal += 1;
  }

  function openAddProject() {
    showAddProject = true;
    setTimeout(() => newProjectInput?.focus(), 0);
  }

  $: ungrouped = (() => {
    const grouped = new Set($projects.flatMap((p) => p.serviceIds));
    return $services.filter((s) => !grouped.has(s.id));
  })();

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

  function idleServices(project: Project): ServiceSummary[] {
    return getProjectServices(project).filter((s) => s.state === "idle");
  }

  function runningServices(project: Project): ServiceSummary[] {
    return getProjectServices(project).filter(
      (s) => s.state === "ready" || s.state === "starting" || s.state === "spawning"
    );
  }

  async function startAll(project: Project) {
    const idle = idleServices(project);
    const ids = idle.map((s) => s.id);
    const namesById = new Map(idle.map((service) => [service.id, service.name]));
    if (ids.length === 0 || bulkStarting.has(project.name) || bulkStopping.has(project.name)) return;
    opError = "";
    bulkStarting = new Set([...bulkStarting, project.name]);
    try {
      const results = await Promise.allSettled(ids.map((id) => startService(id)));
      await load();
      const failed = results
        .map((r, i) => ({ id: ids[i], result: r }))
        .filter(x => x.result.status === "rejected");
      if (failed.length > 0) {
        const names = failed.map(x => namesById.get(x.id) ?? $services.find(s => s.id === x.id)?.name ?? x.id);
        opError = `${failed.length} service(s) failed to start: ${names.join(", ")}`;
      }
    } finally {
      bulkStarting = new Set([...bulkStarting].filter((n) => n !== project.name));
    }
  }

  async function stopAll(project: Project) {
    const running = runningServices(project);
    const ids = running.map((s) => s.id);
    const namesById = new Map(running.map((service) => [service.id, service.name]));
    if (ids.length === 0 || bulkStarting.has(project.name) || bulkStopping.has(project.name)) return;
    opError = "";
    bulkStopping = new Set([...bulkStopping, project.name]);
    try {
      const results = await Promise.allSettled(ids.map((id) => stopService(id)));
      await load();
      const failed = results
        .map((r, i) => ({ id: ids[i], result: r }))
        .filter(x => x.result.status === "rejected");
      if (failed.length > 0) {
        const names = failed.map(x => namesById.get(x.id) ?? $services.find(s => s.id === x.id)?.name ?? x.id);
        opError = `${failed.length} service(s) failed to stop: ${names.join(", ")}`;
      }
    } finally {
      bulkStopping = new Set([...bulkStopping].filter((n) => n !== project.name));
    }
  }

  async function addProject() {
    const trimmed = newProjectName.trim();
    if (!trimmed) return;

    opError = "";
    try {
      await mutateProjects((currentProjects) => {
        if (currentProjects.some((project) => normalizeProjectName(project.name) === normalizeProjectName(trimmed))) {
          throw createProjectCommandError("A project with that name already exists.");
        }
        if (isReservedProjectName(trimmed)) {
          throw createProjectCommandError(`Reserved project name '${trimmed}' is not allowed.`);
        }
        return [...currentProjects, { name: trimmed, serviceIds: [] }];
      });
      newProjectName = "";
      showAddProject = false;
    } catch (e) {
      console.error("Failed to save project:", e);
      opError = extractErrorMessage(e);
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

    opError = "";
    moveLoading = true;
    try {
      await mutateProjects((currentProjects) => {
        if (targetProjectName !== null) {
          const targetId = normalizeProjectName(targetProjectName);
          const existingProject = currentProjects.find((project) => getProjectId(project) === targetId);
          if (existingProject) {
            const canonicalName = existingProject.name;
            return currentProjects.map((project) => {
              if (project.name === canonicalName) {
                return { ...project, serviceIds: [...project.serviceIds.filter((id) => id !== serviceId), serviceId] };
              }
              return { ...project, serviceIds: project.serviceIds.filter((id) => id !== serviceId) };
            });
          }
          return [
            ...currentProjects.map((project) => ({ ...project, serviceIds: project.serviceIds.filter((id) => id !== serviceId) })),
            { name: targetProjectName, serviceIds: [serviceId] },
          ];
        }
        return currentProjects.map((project) => ({
          ...project,
          serviceIds: project.serviceIds.filter((id) => id !== serviceId),
        }));
      });
    } catch (e) {
      opError = extractErrorMessage(e);
    } finally {
      moveLoading = false;
      moveService = null;
      moveServiceCurrentProject = null;
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
    if (!renamingProject || renameSaving) return;
    const trimmed = renameValue.trim();
    if (!trimmed || trimmed === renamingProject) {
      opError = "";
      renamingProject = null;
      return;
    }
    const renamingProjectId = normalizeProjectName(renamingProject);
    renameSaving = true;
    try {
      await mutateProjects((currentProjects) => {
        if (isReservedProjectName(trimmed)) {
          throw createProjectCommandError(`Reserved project name '${trimmed}' is not allowed.`);
        }
        if (currentProjects.some((project) => getProjectId(project) !== renamingProjectId && normalizeProjectName(project.name) === normalizeProjectName(trimmed))) {
          throw createProjectCommandError("A project with that name already exists.");
        }
        if (!currentProjects.some((project) => getProjectId(project) === renamingProjectId)) {
          throw new Error("Project no longer exists.");
        }
        return currentProjects.map((project) =>
          getProjectId(project) === renamingProjectId ? { ...project, name: trimmed } : project
        );
      });
    } catch (e) {
      opError = extractErrorMessage(e);
    } finally {
      renameSaving = false;
      renamingProject = null;
    }
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") commitRename();
    if (e.key === "Escape") { opError = ""; renamingProject = null; }
  }

  // ── Project header overflow menu ─────────────────────────────────────────────

  function toggleHeaderMenu(projectName: string, e: MouseEvent | KeyboardEvent) {
    e.stopPropagation();
    closeServiceMenus();
    openHeaderMenu = openHeaderMenu === projectName ? null : projectName;
    if (openHeaderMenu) {
      setTimeout(() => {
        const menu = headerMenuEls[projectName];
        const firstItem = menu?.querySelector('.overflow-item') as HTMLElement;
        if (firstItem) firstItem.focus();
      }, 0);
    }
  }

  function handleMenuKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      openHeaderMenu = null;
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
        next.focus();
      } else {
        const prev = items[index - 1] || items[items.length - 1];
        prev.focus();
      }
    }
  }

  function closeAllMenus() {
    openHeaderMenu = null;
    closeServiceMenus();
  }

  function closeHeaderMenuOnly() {
    openHeaderMenu = null;
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (deletingProjectId !== null) {
      deletingProjectId = null;
    }
  }

  // ── Project delete ────────────────────────────────────────────────────────────

  function requestDelete(project: Project) {
    openHeaderMenu = null;
    deletingProjectId = getProjectId(project);
  }

  async function confirmDelete() {
    if (deletingProjectId === null || deleteLoading) return;
    opError = "";
    deleteLoading = true;
    try {
      await mutateProjects((currentProjects) => {
        const deletingIndex = currentProjects.findIndex((project) => getProjectId(project) === deletingProjectId);
        if (deletingIndex === -1) {
          throw new Error("Project no longer exists.");
        }
        return currentProjects.filter((_, index) => index !== deletingIndex);
      });
    } catch (e) {
      opError = extractErrorMessage(e);
    } finally {
      deleteLoading = false;
      deletingProjectId = null;
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

<svelte:window on:keydown={handleGlobalKeydown} />

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
        {#if opError}
          <div class="op-error-banner" role="alert">
            <span>{opError}</span>
            <button class="op-error-close" on:click={() => (opError = "")} aria-label="Dismiss">✕</button>
          </div>
        {/if}
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
                <button
                  class="btn-bulk"
                  disabled={bulkStarting.has(project.name) || bulkStopping.has(project.name) || idleServices(project).length === 0}
                  on:click|stopPropagation={() => startAll(project)}
                  title="Start all idle services in this project"
                  aria-label={bulkStarting.has(project.name) ? "Starting services…" : "Start all idle services"}
                  aria-busy={bulkStarting.has(project.name)}
                >
                  {bulkStarting.has(project.name) ? "…" : "Start all"}
                </button>

                <button
                  class="btn-bulk btn-bulk-stop"
                  disabled={bulkStarting.has(project.name) || bulkStopping.has(project.name) || runningServices(project).length === 0}
                  on:click|stopPropagation={() => stopAll(project)}
                  title="Stop all running services in this project"
                  aria-label={bulkStopping.has(project.name) ? "Stopping services…" : "Stop all running services"}
                  aria-busy={bulkStopping.has(project.name)}
                >
                  {bulkStopping.has(project.name) ? "…" : "Stop all"}
                </button>

                <div class="overflow-wrap">
                  <button
                    class="btn-overflow"
                    on:click={(e) => toggleHeaderMenu(project.name, e)}
                    on:keydown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        toggleHeaderMenu(project.name, e);
                      }
                    }}
                    aria-label="Project options"
                    aria-haspopup="menu"
                    aria-expanded={openHeaderMenu === project.name}
                  >⋯</button>
                  {#if openHeaderMenu === project.name}
                    <!-- svelte-ignore a11y-no-static-element-interactions -->
                    <div
                      class="overflow-menu"
                      bind:this={headerMenuEls[project.name]}
                      on:keydown={handleMenuKeydown}
                      role="menu"
                      tabindex="-1"
                    >
                      <button 
                        class="overflow-item" 
                        on:click={() => startRename(project)}
                        role="menuitem"
                        tabindex="-1"
                      >
                        Rename
                      </button>
                      <button
                        class="overflow-item overflow-item-danger"
                        on:click={() => requestDelete(project)}
                        role="menuitem"
                        tabindex="-1"
                      >
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
                    onMenuOpen={closeHeaderMenuOnly}
                    closeSignal={serviceMenuCloseSignal}
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
                  onMenuOpen={closeHeaderMenuOnly}
                  closeSignal={serviceMenuCloseSignal}
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
              bind:this={newProjectInput}
              bind:value={newProjectName}
              on:keydown={(e) => e.key === "Enter" && addProject()}
            />
            <button class="btn-add" on:click={addProject}>Add</button>
            <button class="btn-cancel" on:click={() => (showAddProject = false)}>Cancel</button>
          </div>
        {:else}
          <button class="btn-new-project" on:click={openAddProject}>
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
    loading={moveLoading}
  />
{/if}

<!-- Delete confirmation dialog -->
{#if deletingProject}
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="overlay" on:click|self={() => (deletingProjectId = null)}>
    <div class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="delete-project-dialog-title">
      <p id="delete-project-dialog-title" class="confirm-text">Delete project <strong>{deletingProject.name}</strong>?</p>
      <p class="confirm-hint">Services will be returned to Other Services.</p>
      <div class="confirm-actions">
        <button class="btn btn-cancel" on:click={() => (deletingProjectId = null)}>Cancel</button>
        <button class="btn btn-danger" on:click={confirmDelete} disabled={deleteLoading}>
          {deleteLoading ? "Deleting…" : "Delete"}
        </button>
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

  .project-section:focus-within .project-header-actions {
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

  .btn-bulk {
    padding: 2px 8px;
    background: none;
    border: 1px solid #2a2f45;
    border-radius: 4px;
    color: #64748b;
    font-size: 11px;
    cursor: pointer;
    transition: border-color 0.15s, color 0.15s, background 0.15s;
    white-space: nowrap;
  }

  .btn-bulk:hover:not(:disabled) {
    border-color: #7c3aed;
    color: #a78bfa;
    background: #7c3aed18;
  }

  .btn-bulk:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .btn-bulk-stop:hover:not(:disabled) {
    border-color: #ef4444;
    color: #f87171;
    background: #ef444418;
  }

  .overflow-menu {
    left: 0;
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

  .op-error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: #2d1b1b;
    border: 1px solid #7f1d1d;
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 12px;
    color: #fca5a5;
    margin-bottom: 8px;
  }

  .op-error-close {
    background: none;
    border: none;
    color: #f87171;
    font-size: 12px;
    cursor: pointer;
    flex-shrink: 0;
    padding: 0 2px;
    line-height: 1;
  }

  .op-error-close:hover {
    color: #fca5a5;
  }
</style>
