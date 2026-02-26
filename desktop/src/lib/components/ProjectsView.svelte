<script lang="ts">
  import { onMount } from "svelte";
  import { listProjects, listServices, saveProjects } from "../api";
  import { logPanelServiceId, projects, services } from "../stores";
  import type { Project, ServiceSummary } from "../types";
  import { extractErrorMessage } from "../utils";
  import LogViewer from "./LogViewer.svelte";
  import ServiceCard from "./ServiceCard.svelte";

  let loading = true;
  let error = "";
  let showAddProject = false;
  let newProjectName = "";
  let refreshInterval: ReturnType<typeof setInterval> | undefined;
  let isLoading = false;

  async function load() {
    if (isLoading) return;
    isLoading = true;
    error = "";
    try {
      const [svcList, projConfig] = await Promise.all([listServices(), listProjects()]);
      services.set(svcList);
      projects.set(projConfig.projects);
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

  // Inline the dependency on $projects and $services so Svelte's compiler can
  // statically track them and re-run the block when either store changes.
  $: ungrouped = (() => {
    const grouped = new Set($projects.flatMap((p) => p.serviceIds));
    return $services.filter((s) => !grouped.has(s.id));
  })();

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

  onMount(() => {
    load();
    refreshInterval = setInterval(load, 5000);
    return () => {
      if (refreshInterval !== undefined) clearInterval(refreshInterval);
    };
  });

</script>

<div class="projects-layout">
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
            <h2 class="project-name">{project.name}</h2>
            {#if svcList.length === 0}
              <p class="no-services">No services assigned.</p>
            {:else}
              <div class="service-grid">
                {#each svcList as service}
                  <ServiceCard {service} onRefresh={load} />
                {/each}
              </div>
            {/if}
          </section>
        {/each}

        {#if ungrouped.length > 0}
          <section class="project-section">
            <h2 class="project-name ungrouped">Other Services</h2>
            <div class="service-grid">
              {#each ungrouped as service}
                <ServiceCard {service} onRefresh={load} />
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

  .project-name {
    font-size: 13px;
    font-weight: 600;
    color: #64748b;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
  }

  .project-name.ungrouped {
    color: #334155;
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
</style>
