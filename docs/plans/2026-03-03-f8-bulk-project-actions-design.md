# F8 — Bulk Project Actions Design

**Date:** 2026-03-03
**Status:** Approved
**Priority:** P1

---

## Problem

A developer with a project containing multiple services must click Stop (or Start) on each card individually to bring an environment up or down. Bulk "Start all" and "Stop all" buttons at the project header level are the standard affordance for grouped service managers.

## Scope

Frontend-only change to `ProjectsView.svelte`. No new Tauri commands required.

---

## Design

### Layout

Bulk action buttons are added inline to the existing project header row, between the project name and the `⋯` overflow menu:

```
[Project Name]  [Start all]  [Stop all]  [⋯]
```

Buttons are always visible (not hover-revealed) so the affordance is obvious.

### Button Enabled State

| Button     | Enabled when…                                                      |
|------------|--------------------------------------------------------------------|
| Start all  | At least one service in the project is in `idle` state             |
| Stop all   | At least one service is in `ready`, `starting`, or `spawning` state |

Services in `stopping` state are excluded from "Stop all" (already stopping).

### In-Flight Behavior

- A `Set<string>` named `bulkLoading` tracks project names with an active bulk operation.
- While a project is in `bulkLoading`: both buttons are disabled; the triggered button shows `…` as its label.
- `Promise.allSettled` is used (not `Promise.all`) so a single service failure does not abort the rest.
- On settle: project removed from `bulkLoading`, `load()` called to refresh service state.
- Errors (any rejected promise) are surfaced via the existing `error` string banner.

### Implementation

**File:** `desktop/src/lib/components/ProjectsView.svelte`

1. Add `let bulkLoading = new Set<string>();` to component state.
2. Add `startAll(project)` handler: collect idle service IDs → `Promise.allSettled(ids.map(id => startService(id)))` → remove from `bulkLoading` → `load()`.
3. Add `stopAll(project)` handler: collect running service IDs → same pattern.
4. In the project header template, add the two buttons with `disabled` bindings and `{#if bulkLoading.has(project.name)}` for the `…` label.
5. Add CSS for `.btn-bulk` (small, ghost-style, consistent with existing button hierarchy).

---

## Out of Scope

- Per-card loading state during bulk operations (will become natural once F2 real-time events land).
- Bulk actions for "Other Services" section (ungrouped services have no project context).
- Confirmation dialogs before bulk stop (omitted for speed; can be added later).
