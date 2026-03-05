# PR Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 13 issues found in code review of `feat/desktop-project-management`: error display architecture, bulk operation reliability, modal/dialog correctness, CSS selector injection, reserved-name gaps, Svelte reactivity, and add Rust unit tests.

**Architecture:** Split `error` (connectivity/load only → full-screen Retry view) from `opError` (operational errors → dismissible banner in normal view). Fix all operations to use `opError` and handle their own cleanup in `finally` blocks.

**Tech Stack:** Svelte 4, TypeScript, Rust/Tauri, `Promise.allSettled` for bulk ops.

---

### Task 1: Extract and test `validate_project_names` in Rust

**Files:**
- Modify: `desktop/src-tauri/src/commands/projects.rs`

No test framework to run separately — tests run via `cargo test`.

**Step 1: Extract the validation logic into a testable pure function**

In `desktop/src-tauri/src/commands/projects.rs`, extract the for-loop into a standalone function and call it from `save_projects`. Replace the existing validation block (lines 22–31):

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

Then in `save_projects`, replace the inline validation block with:

```rust
validate_project_names(&config.projects)?;
```

**Step 2: Add unit tests at the bottom of `projects.rs`**

```rust
#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::state::Project;

    fn project(name: &str) -> Project {
        Project { name: name.to_string(), service_ids: vec![] }
    }

    #[test]
    fn proj004_none_is_reserved() {
        let err = validate_project_names(&[project("__none__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("__none__"));
    }

    #[test]
    fn proj004_new_is_reserved() {
        let err = validate_project_names(&[project("__new__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
        assert!(err.message.contains("__new__"));
    }

    #[test]
    fn proj004_trimmed_spaces_detected() {
        let err = validate_project_names(&[project("  __none__  ")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
    }

    #[test]
    fn proj004_valid_name_accepted() {
        validate_project_names(&[project("my-project")]).expect("valid name must pass");
    }

    #[test]
    fn proj004_second_project_reserved() {
        let err = validate_project_names(&[project("valid"), project("__new__")]).unwrap_err();
        assert_eq!(err.code, "PROJ004");
    }
}
```

**Step 3: Check what `Project` looks like in the state module**

Run:
```bash
grep -n "struct Project" desktop/src-tauri/src/state.rs
```

Adjust the field names in the `project()` helper to match the actual struct (it may be `serviceIds` or `service_ids` depending on serde rename). Look at the existing `ProjectConfig` / `Project` definitions to get exact field names.

**Step 4: Run the tests**

```bash
cd desktop/src-tauri && cargo test -p canopus-desktop commands::projects
```

Expected: 5 tests pass.

**Step 5: Commit**

```bash
git add desktop/src-tauri/src/commands/projects.rs
git commit -m "test(desktop): add unit tests for PROJ004 reserved name validation"
```

---

### Task 2: Add two-tier error state to `ProjectsView.svelte`

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

This is the foundational change everything else builds on. Do this before any other frontend task.

**Step 1: Add the `opError` variable**

In the `<script>` block, after `let error = "";` (line 13), add:

```ts
let opError = "";
```

**Step 2: Add the op-error banner to the template**

Inside the `{:else}` block (normal view), at the very start of `<div class="projects-list">` (before the `{#each $projects}` loop, around line 346), insert:

```svelte
{#if opError}
  <div class="op-error-banner" role="alert">
    <span>{opError}</span>
    <button class="op-error-close" on:click={() => (opError = "")} aria-label="Dismiss">✕</button>
  </div>
{/if}
```

**Step 3: Add the banner styles**

In `<style>`, add:

```css
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
```

**Step 4: Verify the template compiles**

```bash
cd desktop && npm run build 2>&1 | head -40
```

Expected: No errors about `opError`.

**Step 5: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "feat(desktop): add two-tier error state - opError banner for operational errors"
```

---

### Task 3: Route all operational errors to `opError`

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

Change every `error = extractErrorMessage(e)` in operational functions to `opError = extractErrorMessage(e)`. Also change the inline validation errors.

**Step 1: Fix `addProject`**

Change (lines ~128, ~132, ~144):
```ts
// Before:
if ($projects.some(p => p.name.toLowerCase() === trimmed.toLowerCase())) {
  error = "A project with that name already exists.";
// After:
  opError = "A project with that name already exists.";
```

```ts
// Before:
if (trimmed === "__none__" || trimmed === "__new__") {
  error = "Reserved project name.";
// After:
  opError = `Reserved project name '${trimmed}' is not allowed.`;
```

```ts
// Before (in catch):
  error = extractErrorMessage(e);
// After:
  opError = extractErrorMessage(e);
```

**Step 2: Fix `startAll` and `stopAll`**

In `startAll` (around line 99–100):
```ts
// Before:
if (failCount > 0 && !error) {
  error = `${failCount} service(s) failed to start.`;
}
// After:
if (failCount > 0) {
  const failed = results
    .map((r, i) => ({ id: ids[i], result: r }))
    .filter(x => x.result.status === "rejected");
  const names = failed.map(x => $services.find(s => s.id === x.id)?.name ?? x.id);
  opError = `${failed.length} service(s) failed to start: ${names.join(", ")}`;
}
```

Apply the same pattern to `stopAll` (around line 115–116), changing "start" to "stop":
```ts
if (failCount > 0) {
  const failed = results
    .map((r, i) => ({ id: ids[i], result: r }))
    .filter(x => x.result.status === "rejected");
  const names = failed.map(x => $services.find(s => s.id === x.id)?.name ?? x.id);
  opError = `${failed.length} service(s) failed to stop: ${names.join(", ")}`;
}
```

Note: `ids` in `stopAll` comes from `runningServices(project).map(s => s.id)` — make sure the index mapping matches.

**Step 3: Fix `handleMoveConfirm`**

In the catch block (line ~195):
```ts
// Before:
  error = extractErrorMessage(e);
// After:
  opError = extractErrorMessage(e);
```

**Step 4: Fix `confirmDelete`**

In the catch block (line ~318):
```ts
// Before:
  error = extractErrorMessage(e);
// After:
  opError = extractErrorMessage(e);
```

**Step 5: Verify build**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 6: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): route all operational errors to opError banner, not connectivity error view"
```

---

### Task 4: Fix `commitRename` — keep input open on validation errors, add reserved-name check

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

**Step 1: Rewrite `commitRename`**

Replace the entire `commitRename` function (lines ~216–241) with:

```ts
async function commitRename() {
  if (!renamingProject || renameSaving) return;
  const trimmed = renameValue.trim();
  if (!trimmed || trimmed === renamingProject) {
    renamingProject = null;
    return;
  }
  // Validation: keep rename input open on failure so user can correct
  if (trimmed === "__none__" || trimmed === "__new__") {
    opError = `Reserved project name '${trimmed}' is not allowed.`;
    return;
  }
  if ($projects.some(p => p.name !== renamingProject && p.name.toLowerCase() === trimmed.toLowerCase())) {
    opError = "A project with that name already exists.";
    return;
  }
  const updated = $projects.map((p) =>
    p.name === renamingProject ? { ...p, name: trimmed } : p
  );
  renameSaving = true;
  try {
    await saveProjects({ projects: updated });
    projects.set(updated);
  } catch (e) {
    opError = extractErrorMessage(e);
  } finally {
    renameSaving = false;
    renamingProject = null;
  }
}
```

Key changes:
- Reserved-name check added
- Validation failures set `opError` and `return` WITHOUT setting `renamingProject = null` — the input stays open
- Backend errors still close the input (in `finally`) since there's nothing the user can correct in the input for a filesystem error

**Step 2: Verify build**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 3: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): keep rename input open on validation errors, add reserved name check"
```

---

### Task 5: Fix `confirmDelete` — add busy guard and always close dialog

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

**Step 1: Add `deleteLoading` variable**

In the `<script>` block, after `let deletingProject: string | null = null;` (line 39), add:

```ts
let deleteLoading = false;
```

**Step 2: Rewrite `confirmDelete`**

Replace the function (lines ~310–320):

```ts
async function confirmDelete() {
  if (!deletingProject || deleteLoading) return;
  const updated = $projects.filter((p) => p.name !== deletingProject);
  deleteLoading = true;
  try {
    await saveProjects({ projects: updated });
    projects.set(updated);
  } catch (e) {
    opError = extractErrorMessage(e);
  } finally {
    deleteLoading = false;
    deletingProject = null;
  }
}
```

**Step 3: Disable the Delete button while loading**

In the delete dialog template (around line 539), update the Delete button:

```svelte
<button class="btn btn-danger" on:click={confirmDelete} disabled={deleteLoading}>
  {deleteLoading ? "Deleting…" : "Delete"}
</button>
```

**Step 4: Verify build**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 5: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): add busy guard to delete, always close dialog in finally"
```

---

### Task 6: Fix `handleMoveConfirm` — always close modal in finally

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

**Step 1: Rewrite the try/catch/finally in `handleMoveConfirm`**

Replace lines ~189–199:

```ts
moveLoading = true;
try {
  await saveProjects({ projects: updated });
  projects.set(updated);
} catch (e) {
  opError = extractErrorMessage(e);
} finally {
  moveLoading = false;
  moveService = null;
  moveServiceCurrentProject = null;
}
```

The modal now always closes — success or failure. Errors appear in the `opError` banner.

**Step 2: Verify build**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 3: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): always close move modal in finally, show errors in opError banner"
```

---

### Task 7: Fix Svelte reactivity — inline bulk button disabled checks

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

`isBulkBusy(project.name)` called as a plain function in the template won't re-evaluate reactively when the Sets change. Inline the checks.

**Step 1: Update the "Start all" button**

Find (around line 374):
```svelte
disabled={isBulkBusy(project.name) || idleServices(project).length === 0}
```

Replace with:
```svelte
disabled={bulkStarting.has(project.name) || bulkStopping.has(project.name) || idleServices(project).length === 0}
```

**Step 2: Update the "Stop all" button**

Find (around line 385):
```svelte
disabled={isBulkBusy(project.name) || runningServices(project).length === 0}
```

Replace with:
```svelte
disabled={bulkStarting.has(project.name) || bulkStopping.has(project.name) || runningServices(project).length === 0}
```

**Step 3: Remove `isBulkBusy` function** (it's now unused)

Delete lines ~41–43:
```ts
function isBulkBusy(projectName: string): boolean {
  return bulkStarting.has(projectName) || bulkStopping.has(projectName);
}
```

**Step 4: Verify build with no unused variable warnings**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 5: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): inline bulk button disabled checks for correct Svelte reactivity"
```

---

### Task 8: Fix CSS selector injection — use `bind:this` map for header menus

**Files:**
- Modify: `desktop/src/lib/components/ProjectsView.svelte`

**Step 1: Add menu element map variable**

In the `<script>` block, after `let openHeaderMenu: string | null = null;`:

```ts
let headerMenuEls: Record<string, HTMLElement> = {};
```

**Step 2: Bind the menu element in the template**

Find the overflow-menu div (around line 410):
```svelte
<div
  class="overflow-menu"
  data-project={project.name}
  on:keydown={handleMenuKeydown}
  role="menu"
  tabindex="-1"
>
```

Replace with:
```svelte
<div
  class="overflow-menu"
  bind:this={headerMenuEls[project.name]}
  on:keydown={handleMenuKeydown}
  role="menu"
  tabindex="-1"
>
```

(Remove the `data-project` attribute — it's no longer needed for querying.)

**Step 3: Update `toggleHeaderMenu` to use the map**

Replace the `setTimeout` block inside `toggleHeaderMenu` (lines ~254–260):

```ts
if (openHeaderMenu) {
  setTimeout(() => {
    const menu = headerMenuEls[projectName];
    const firstItem = menu?.querySelector('.overflow-item') as HTMLElement;
    if (firstItem) firstItem.focus();
  }, 0);
}
```

**Step 4: Verify build**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 5: Commit**

```bash
git add desktop/src/lib/components/ProjectsView.svelte
git commit -m "fix(desktop): replace CSS selector injection with bind:this map for header menus"
```

---

### Task 9: Fix `MoveToProjectModal` — reserved-name check and Enter key

**Files:**
- Modify: `desktop/src/lib/components/MoveToProjectModal.svelte`

**Step 1: Add reserved-name check in `confirm()`**

In the `confirm()` function, after the empty-name check and before the duplicate check (around line 36):

```ts
function confirm() {
  if (loading) return;
  if (selected === NEW) {
    const name = newProjectName.trim();
    if (!name) return;
    if (name === "__none__" || name === "__new__") {
      newProjectError = "Reserved project name.";
      return;
    }
    if (projects.some(p => p.name.toLowerCase() === name.toLowerCase())) {
      newProjectError = "A project with that name already exists.";
      return;
    }
    onConfirm(name);
  } else if (selected === NONE) {
    onConfirm(null);
  } else {
    onConfirm(selected as string);
  }
}
```

**Step 2: Fix the Enter key handler to not fire on the new-project text input**

Replace `handleKeydown` (lines ~49–52):

```ts
function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") onClose();
  // Don't intercept Enter from the new-project text input — it handles its own submit
  if (e.key === "Enter" && !(e.target instanceof HTMLInputElement)) confirm();
}
```

**Step 3: Add `on:keydown` to the new-project text input**

In the template, find the new-project text input (around line 88) and add a keydown handler:

```svelte
<input
  class="new-project-input"
  type="text"
  placeholder="New project name…"
  bind:value={newProjectName}
  bind:this={newProjectInput}
  on:input={() => (newProjectError = "")}
  on:click|stopPropagation={() => { selected = NEW; }}
  on:keydown={(e) => { if (e.key === "Enter") { e.stopPropagation(); confirm(); } }}
  disabled={loading}
/>
```

**Step 4: Verify build**

```bash
cd desktop && npm run build 2>&1 | head -40
```

**Step 5: Commit**

```bash
git add desktop/src/lib/components/MoveToProjectModal.svelte
git commit -m "fix(desktop): add reserved-name check and fix Enter key in MoveToProjectModal"
```

---

### Task 10: Final verification

**Step 1: Full build**

```bash
cd desktop && npm run build
```

Expected: Clean build, no TypeScript or Svelte errors.

**Step 2: Run Rust tests**

```bash
cd desktop/src-tauri && cargo test
```

Expected: All tests pass including the 5 new `proj004_*` tests.

**Step 3: Run linting**

```bash
cd /Users/chanwaichan/workspace/canopus && just lint
```

Expected: No warnings.

**Step 4: Final commit if any lint fixes were needed**

```bash
git add -p
git commit -m "fix(desktop): address lint warnings from PR review fixes"
```

**Step 5: Summary commit message for the branch**

The branch now has these logical commits:
1. `test(desktop): add unit tests for PROJ004 reserved name validation`
2. `feat(desktop): add two-tier error state - opError banner for operational errors`
3. `fix(desktop): route all operational errors to opError banner, not connectivity error view`
4. `fix(desktop): keep rename input open on validation errors, add reserved name check`
5. `fix(desktop): add busy guard to delete, always close dialog in finally`
6. `fix(desktop): always close move modal in finally, show errors in opError banner`
7. `fix(desktop): inline bulk button disabled checks for correct Svelte reactivity`
8. `fix(desktop): replace CSS selector injection with bind:this map for header menus`
9. `fix(desktop): add reserved-name check and fix Enter key in MoveToProjectModal`

---

## Notes

- The `Project` struct field names in Rust tests (Task 1) must match the actual struct definition in `desktop/src-tauri/src/state.rs`. Check before writing the test helper.
- Svelte's `bind:this` on elements inside `{#if}` blocks is set when the element mounts and cleared (set to `undefined`) when it unmounts. `headerMenuEls[project.name]` will be `undefined` when the menu is closed — the `?.` optional chaining in `toggleHeaderMenu` handles this safely.
- The `opError` banner sits inside the `{:else}` (normal view) block so it is only visible when the project list is visible. It cannot conflict with the full-screen error view.
