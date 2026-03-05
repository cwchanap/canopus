# Design: PR Review Fixes for feat/desktop-project-management

**Date:** 2026-03-04
**Branch:** feat/desktop-project-management
**Status:** Approved

## Problem

The PR review identified 13 issues across two categories:

1. **Error display architecture** — operational errors (rename, move, delete, bulk start/stop) incorrectly use the global `error` variable, which triggers a full-screen "Could not connect to Canopus daemon." view and wipes the normal project list. Bulk errors are also silently erased by the periodic 5-second `load()` refresh.

2. **Targeted bugs** — CSS selector injection, missing reserved-name validation in rename/move paths, Svelte reactivity issue with `isBulkBusy`, delete dialog missing busy guard, move modal staying open on error, `commitRename` discarding user input on validation failure, `MoveToProjectModal` Enter key firing on whitespace, and no Rust unit tests for the PROJ004 validation.

## Approach: Two-Tier Error State

Keep `error` for connectivity/load failures (full-screen Retry view). Add `opError` for all operational errors, displayed as a dismissible banner inside the normal view.

This is the minimum change that fixes the core bug without over-engineering. The two tiers map naturally to the existing structure.

## Design

### 1. Error Display Architecture (`ProjectsView.svelte`)

- **`error: string`** — connectivity/load failures only. `load()` sets and clears this. Triggers full-screen "Could not connect to Canopus daemon." with Retry button. Unchanged behavior.
- **`opError: string`** — all operational errors. Shown as a small dismissible banner (✕ button) at the top of the project list, inside the `{:else}` block. Never replaces the view.
- `load()` no longer touches `opError`. Operational errors persist until the user dismisses them or a successful subsequent operation clears them.

Banner placement: above all `{#each $projects}` sections, below the `<div class="projects-list">` opening tag.

### 2. Bulk Operation Fixes

**Issue 1 — Ephemeral errors:** Bulk errors → `opError`. Since `load()` never clears `opError`, these persist until dismissed.

**Issue 2 — Failure detail:** Collect rejected results with service names:
```ts
const failed = results
  .map((r, i) => ({ id: ids[i], result: r }))
  .filter(x => x.result.status === "rejected");
if (failed.length > 0) {
  const names = failed.map(x => $services.find(s => s.id === x.id)?.name ?? x.id);
  opError = `${failed.length} service(s) failed to start: ${names.join(", ")}`;
}
```

**Issue 7 — `!error` guard:** Drop the `&& !error` guard entirely. `opError` and `error` are now separate so there's no overwrite risk. Always report failures.

**Issue 9 — Svelte reactivity:** Replace `isBulkBusy(project.name)` calls in template with direct inline Set reads:
```svelte
disabled={bulkStarting.has(project.name) || bulkStopping.has(project.name) || idleServices(project).length === 0}
```
(Same pattern for Stop all button.)

### 3. Modal and Dialog Fixes

**Issue 5 — Delete dialog:**
- Add `let deleteLoading = false` busy flag; guard `confirmDelete` against double-clicks.
- Move `deletingProject = null` into `finally` so dialog always closes (error goes to `opError`).

**Issue 6 — Move modal:**
- In `handleMoveConfirm` catch: set `opError`, then close modal via `moveService = null` in `finally`.

**Issues 3/10 — `commitRename`:**
- On duplicate-name or reserved-name validation failure: set `opError`, do NOT set `renamingProject = null`. The rename input stays open so the user can correct the name.
- Move `renamingProject = null` into `finally` so it closes on both success and backend error.
- Add reserved-name check (matching `addProject`):
  ```ts
  if (trimmed === "__none__" || trimmed === "__new__") {
    opError = `Reserved project name '${trimmed}' is not allowed.`;
    return; // keep rename input open
  }
  ```

**Issue 11 — `MoveToProjectModal` Enter key:**
- In the new-project text input, add `on:keydown` that calls `confirm()` and `e.stopPropagation()`, preventing the window-level handler from firing a second time.
- The window-level `handleKeydown` guard: only call `confirm()` on Enter if `selected !== NEW` (or if the new-project input is empty/hidden).

**Issue 8 — Reserved name in modal:**
- In `MoveToProjectModal.confirm()` when `selected === NEW`:
  ```ts
  if (name === "__none__" || name === "__new__") {
    newProjectError = "Reserved project name.";
    return;
  }
  ```

### 4. CSS Selector Fix (`ProjectsView.svelte`)

**Issue 4 — CSS selector injection:**
Replace `document.querySelector(`.overflow-menu[data-project="${projectName}"]`)` with a `Map<string, HTMLElement>` approach using `bind:this`:

```svelte
{#if openHeaderMenu === project.name}
  <div
    class="overflow-menu"
    bind:this={headerMenuEls[project.name]}
    ...
  >
```

Then `toggleHeaderMenu` reads `headerMenuEls[projectName]` directly instead of calling `document.querySelector`.

### 5. Rust Unit Tests (`projects.rs`)

Extract validation into a pure helper and test it:

```rust
fn validate_project_names(projects: &[crate::state::Project]) -> Result<(), CommandError> {
    for project in projects {
        let name = project.name.trim();
        if name == "__none__" || name == "__new__" {
            return Err(CommandError {
                code: "PROJ004",
                message: format!("Reserved project name '{name}' is not allowed"),
            });
        }
    }
    Ok(())
}
```

Tests (following the pattern in `commands/mod.rs`):
- `proj004_none_is_reserved` — name `"__none__"` → Err with code `"PROJ004"`
- `proj004_new_is_reserved` — name `"__new__"` → Err with code `"PROJ004"`
- `proj004_trimmed_spaces_detected` — name `"  __none__  "` → Err (verifies `.trim()`)
- `proj004_valid_name_accepted` — name `"my-project"` → Ok
- `proj004_second_project_reserved` — two projects, first valid, second `"__new__"` → Err

### 6. Out of Scope

- `ServiceCard` ARIA attributes (minor, follow-up PR)
- Competing Escape handlers (functional, low severity)
- `load()` partial state with `allSettled` (pre-existing)
- Case-sensitivity asymmetry for reserved names (intentional behavior)

## Files Changed

| File | Changes |
|------|---------|
| `desktop/src/lib/components/ProjectsView.svelte` | Two-tier error state, bulk fixes, dialog/modal fixes, CSS selector fix, rename fixes |
| `desktop/src/lib/components/MoveToProjectModal.svelte` | Reserved name check, Enter key fix |
| `desktop/src-tauri/src/commands/projects.rs` | Extract `validate_project_names`, add unit tests |
