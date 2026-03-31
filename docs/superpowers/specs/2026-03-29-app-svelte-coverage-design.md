# App.svelte Unit Coverage Increase Design

## Problem

`desktop/src/App.svelte` contains meaningful mount-time behavior, but the desktop package currently has no JS unit-test setup. That leaves the file's listener registration, unread-count seeding, cancellation handling, cleanup, and view switching untested. The goal is to add the minimum desktop-side test infrastructure needed to improve `App.svelte` unit coverage by at least 10% without broadening scope to unrelated desktop components.

## Proposed Approach

Add a small desktop-only unit test stack based on `vitest`, `@testing-library/svelte`, and `jsdom`, then write focused behavior tests for `desktop/src/App.svelte`.

The tests will isolate `App.svelte` by:

- mocking `./lib/api` so Tauri-backed calls are never exercised in unit tests
- stubbing `Sidebar`, `ProjectsView`, and `InboxView` so the tests stay centered on `App.svelte` orchestration logic
- controlling exported stores directly to drive the `projects` vs `inbox` branch

This keeps the implementation small while exercising the file's real logic instead of relying on snapshots or full-component integration tests.

## Alternatives Considered

### 1. Direct `App.svelte` behavior tests

This is the selected approach. It offers the best balance of coverage gain, confidence, and implementation cost.

### 2. Render-only smoke tests

This would add less infrastructure, but it would likely miss most of the `onMount` logic and probably would not reach the requested coverage improvement.

### 3. Integrated tests with real child components

This would be more realistic, but it would pull unrelated component behavior into the test surface, increase brittleness, and require more setup than this file warrants.

## Scope

### In scope

- adding the minimum test dependencies and scripts required to run desktop unit tests
- adding Vite/Vitest configuration needed for Svelte component tests
- adding focused tests for `desktop/src/App.svelte`
- measuring coverage for the new desktop tests

### Out of scope

- adding tests for `Sidebar`, `ProjectsView`, `InboxView`, or other desktop components
- refactoring `App.svelte` unless a tiny testability adjustment is required
- changing Rust-side coverage or repository-wide coverage workflows

## Design Details

### Test infrastructure

Update the desktop package so it can run Svelte unit tests locally and in CI-style validation:

- add `vitest`, `@testing-library/svelte`, `jsdom`, and any required companion packages to `desktop/package.json`
- add a `test` script for running Vitest
- add a `test:coverage` script so the coverage increase can be measured explicitly
- extend the existing Vite setup, or add a closely related test config, to use a `jsdom` environment and Svelte preprocessing correctly

The infrastructure should remain desktop-local and should not alter Rust workspace commands.

### Test file layout

Create a new test file adjacent to the component:

- `desktop/src/App.test.ts`

Keeping the test next to the component makes the scope obvious and avoids inventing a parallel test layout for a single targeted addition.

### Mocking strategy

Use module mocks for `./lib/api` and lightweight component stubs for child components.

The mock API surface needs to cover:

- `onLogUpdate`
- `listInbox`

The tests should also interact with these existing stores directly:

- `activeView`
- `inboxUnreadCount`

That preserves the current component contract and avoids introducing test-only hooks.

## Required Test Scenarios

### 1. Default render path

Verify that `ProjectsView` renders when `activeView` is left at its default value.

### 2. Alternate render path

Set `activeView` to `"inbox"` and verify that `InboxView` renders instead of `ProjectsView`.

### 3. Successful mount flow

Verify that:

- `onLogUpdate` is called with `appendLog`
- `listInbox` is called with `{ status: "unread" }`
- `inboxUnreadCount` is updated to match the returned unread item count

### 4. Unmount cleanup

Verify that the unlisten function returned by `onLogUpdate` is called when the component unmounts normally.

### 5. Cancellation race

Cover the case where the component unmounts before the asynchronous `onLogUpdate` promise resolves. The test should verify that the eventually returned unlisten function is invoked immediately instead of being retained.

### 6. Listener registration failure

Make `onLogUpdate` reject and verify that the component logs the failure without throwing.

### 7. Initial unread fetch failure

Make `listInbox` reject and verify that the component logs the failure without throwing.

## Validation Plan

After implementation:

1. run the new desktop unit tests
2. run desktop coverage and confirm the targeted increase for `App.svelte`
3. run the desktop build to ensure the test configuration did not break the package build

Expected validation commands:

- `cd desktop && npm run test -- --run`
- `cd desktop && npm run test:coverage -- --run`
- `cd desktop && npm run build`

## Risks and Mitigations

### Risk: Vite test configuration interferes with the desktop build

Mitigation: keep the test configuration minimal and aligned with the existing `vite.config.ts`, then verify with a normal build.

### Risk: Tests become brittle by depending on child component internals

Mitigation: stub child components and assert only `App.svelte` responsibilities.

### Risk: Async race tests become flaky

Mitigation: control promise resolution explicitly in the test and wait for observable outcomes instead of relying on fixed sleeps.

## Success Criteria

- the desktop package gains a runnable unit-test workflow
- `desktop/src/App.svelte` has focused unit tests covering both success and failure paths
- measured coverage for the target file increases by at least 10%
- the desktop build still succeeds after the test setup is added
