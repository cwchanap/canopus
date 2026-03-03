# F8 Bulk Project Actions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add "Start all" and "Stop all" buttons to each project section header so developers can bring an entire project environment up or down with one click.

**Architecture:** Pure frontend change to `ProjectsView.svelte`. A `Set<string>` tracks which project names have an in-flight bulk operation. The bulk handlers fire concurrent `startService`/`stopService` calls via `Promise.allSettled`, then trigger a refresh. No new Tauri commands needed.

**Tech Stack:** Svelte 4, TypeScript, existing `startService`/`stopService` from `api.ts`.

---

### Task 1: Add bulk state and handlers to ProjectsView

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

**Step 1: Add `bulkLoading` state after the existing state declarations**

In the `<script>` block, after the line `let isLoading = false;`, add:

```ts
// Tracks project names with an active bulk start/stop operation
let bulkLoading = new Set<string>();
```

**Step 2: Add helper to compute startable / stoppable service lists**

After the `ungrouped` reactive statement, add:

```ts
function idleServices(project: Project): ServiceSummary[] {
  return getProjectServices(project).filter((s) => s.state === "idle");
}

function runningServices(project: Project): ServiceSummary[] {
  return getProjectServices(project).filter(
    (s) => s.state === "ready" || s.state === "starting" || s.state === "spawning"
  );
}
```

**Step 3: Add `startAll` and `stopAll` handlers**

After the `runningServices` helper:

```ts
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
```

Note: `bulkLoading = new Set(...)` reassignment (instead of `.add()`/`.delete()`) is required for Svelte's reactivity to detect the change.

**Step 4: Verify the script block looks correct**

The top of the `<script>` should now have all state variables and the bottom should have `startAll`/`stopAll` defined before `onMount`.

**Step 5: Commit (script changes only — template unchanged yet)**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "feat(desktop): add bulk start/stop handlers for F8"
```

---

### Task 2: Add bulk action buttons to the project header template

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

**Step 1: Locate the project header in the template**

Find the `<div class="project-header">` block — it currently contains the rename input/heading and the `<div class="project-header-actions">` with the `⋯` menu.

**Step 2: Add bulk buttons inside `.project-header-actions`**

Replace the existing `.project-header-actions` div:

```svelte
<div class="project-header-actions">
  <div class="overflow-wrap">
    ...
  </div>
</div>
```

with:

```svelte
<div class="project-header-actions">
  {@const isBulkLoading = bulkLoading.has(project.name)}
  {@const canStartAll = idleServices(project).length > 0}
  {@const canStopAll = runningServices(project).length > 0}

  <button
    class="btn-bulk"
    disabled={isBulkLoading || !canStartAll}
    on:click|stopPropagation={() => startAll(project)}
    title="Start all idle services in this project"
  >
    {isBulkLoading && canStartAll && !canStopAll ? "…" : "Start all"}
  </button>

  <button
    class="btn-bulk btn-bulk-stop"
    disabled={isBulkLoading || !canStopAll}
    on:click|stopPropagation={() => stopAll(project)}
    title="Stop all running services in this project"
  >
    {isBulkLoading && canStopAll ? "…" : "Stop all"}
  </button>

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
```

Note: `{@const}` tags must be direct children of each block — place them inside the `{#each $projects as project}` block, not inside the `.project-header-actions` div. If the Svelte version (4.x) does not support `{@const}` inside component markup, use `$: ` derived variables or inline the expressions directly on the `disabled` attributes.

**Step 3: Run the dev server to visually verify**

```bash
cd desktop && npm run dev
```

Check that:
- Projects with at least one `idle` service show an enabled "Start all" button
- Projects with running services show an enabled "Stop all" button
- Both buttons are disabled when no qualifying services exist
- Clicking fires the action (check browser console or daemon logs)

**Step 4: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "feat(desktop): add Start all / Stop all buttons to project header"
```

---

### Task 3: Style the bulk action buttons

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte` (the `<style>` block)

**Step 1: Add `.btn-bulk` styles**

In the `<style>` block, after the existing `.btn-overflow` rule, add:

```css
.btn-bulk {
  background: none;
  border: 1px solid #2a2f45;
  border-radius: 5px;
  color: #64748b;
  font-size: 11px;
  padding: 3px 10px;
  cursor: pointer;
  transition: border-color 0.15s, color 0.15s;
  white-space: nowrap;
}

.btn-bulk:hover:not(:disabled) {
  border-color: #7c3aed;
  color: #a78bfa;
}

.btn-bulk:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.btn-bulk-stop:hover:not(:disabled) {
  border-color: #ef4444;
  color: #f87171;
}
```

**Step 2: Verify visually**

Reload the dev server. Check that:
- "Start all" button hovers purple (matching the primary action color)
- "Stop all" button hovers red (matching the danger color)
- Disabled buttons are visibly dimmed

**Step 3: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "style(desktop): add bulk action button styles"
```

---

### Task 4: Svelte reactivity fix for `{@const}` compatibility

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

Svelte 4 supports `{@const}` inside `{#each}` blocks but NOT as a direct child of a regular HTML element like `<div>`. If the build produces an error like "'{@const}' tags cannot be a child of...", replace the `{@const}` tags in Task 2 with inline expressions:

```svelte
<div class="project-header-actions">
  <button
    class="btn-bulk"
    disabled={bulkLoading.has(project.name) || idleServices(project).length === 0}
    on:click|stopPropagation={() => startAll(project)}
    title="Start all idle services in this project"
  >
    {bulkLoading.has(project.name) && idleServices(project).length > 0 && runningServices(project).length === 0 ? "…" : "Start all"}
  </button>

  <button
    class="btn-bulk btn-bulk-stop"
    disabled={bulkLoading.has(project.name) || runningServices(project).length === 0}
    on:click|stopPropagation={() => stopAll(project)}
    title="Stop all running services in this project"
  >
    {bulkLoading.has(project.name) && runningServices(project).length > 0 ? "…" : "Stop all"}
  </button>
  ...
```

Run `npm run build` to confirm no errors.

**Step 5: Commit if changes were needed**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): inline expressions for Svelte 4 compatibility"
```

---

### Task 5: Final build verification

**Step 1: Clean build**

```bash
cd desktop && npm run build
```

Expected: `✓ built` with no errors. One pre-existing `autofocus` accessibility warning is normal.

**Step 2: Tauri build check (optional, slower)**

```bash
cargo tauri build --debug 2>&1 | tail -5
```

Expected: `Finished` with no errors.

**Step 3: Final commit (if any loose changes remain)**

```bash
git status
# If clean, nothing to do. If there are changes:
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "chore(desktop): finalize F8 bulk project actions"
```
