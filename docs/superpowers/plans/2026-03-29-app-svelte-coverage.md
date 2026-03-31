# App.svelte Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a minimal desktop-side unit test workflow and focused `desktop/src/App.svelte` tests that improve `App.svelte` line coverage by at least 10%.

**Architecture:** Keep production behavior unchanged and isolate `App.svelte` in tests by mocking `./lib/api`, stubbing the three child components it renders, and driving the existing Svelte stores directly. Reuse the existing Vite config instead of introducing a parallel toolchain so testing stays local to `desktop/` and does not change Rust workspace commands.

**Tech Stack:** Svelte 5, Vite 5, Vitest, `@testing-library/svelte`, `@testing-library/jest-dom`, `jsdom`, npm

---

## File Map

- Modify: `desktop/package.json` — add the desktop test scripts and dev dependencies.
- Modify: `desktop/package-lock.json` — lock the newly installed test packages.
- Modify: `desktop/vite.config.ts` — add Vitest config on top of the existing Vite/Svelte setup.
- Create: `desktop/src/vitest.setup.ts` — install Testing Library cleanup and DOM matchers.
- Create: `desktop/src/test/stubs/SidebarStub.svelte` — lightweight stand-in for `Sidebar.svelte`.
- Create: `desktop/src/test/stubs/ProjectsViewStub.svelte` — lightweight stand-in for `ProjectsView.svelte`.
- Create: `desktop/src/test/stubs/InboxViewStub.svelte` — lightweight stand-in for `InboxView.svelte`.
- Create: `desktop/src/App.test.ts` — focused unit tests for render branching, mount flow, cleanup, race handling, and error logging.
- Modify only if required by failing tests: `desktop/src/App.svelte` — reserve for a tiny testability fix if the existing behavior cannot be observed safely from tests. Do not refactor this file speculatively.

## Coverage Target

Use **line coverage for `desktop/src/App.svelte`** as the success metric. Because the file currently has no desktop-side unit tests, treat the starting point as effectively unmeasured/untested and require the final `vitest --coverage` report to show at least a 10 percentage-point improvement for that file.

## Task 1: Stand Up the Desktop Test Harness

**Files:**
- Modify: `desktop/package.json`
- Modify: `desktop/package-lock.json`
- Modify: `desktop/vite.config.ts`
- Create: `desktop/src/vitest.setup.ts`
- Create: `desktop/src/test/stubs/SidebarStub.svelte`
- Create: `desktop/src/test/stubs/ProjectsViewStub.svelte`
- Create: `desktop/src/test/stubs/InboxViewStub.svelte`
- Create: `desktop/src/App.test.ts`

- [ ] **Step 1: Write the first failing test file**

Create `desktop/src/App.test.ts` with one intentionally failing render-path test before any test tooling is added:

```ts
import { render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App.svelte";
import { activeView, inboxUnreadCount } from "./lib/stores";

const { onLogUpdate, listInbox } = vi.hoisted(() => ({
  onLogUpdate: vi.fn(),
  listInbox: vi.fn(),
}));

vi.mock("./lib/api", () => ({
  onLogUpdate,
  listInbox,
}));

vi.mock("./lib/components/Sidebar.svelte", async () => {
  const stub = await import("./test/stubs/SidebarStub.svelte");
  return { default: stub.default };
});

vi.mock("./lib/components/ProjectsView.svelte", async () => {
  const stub = await import("./test/stubs/ProjectsViewStub.svelte");
  return { default: stub.default };
});

vi.mock("./lib/components/InboxView.svelte", async () => {
  const stub = await import("./test/stubs/InboxViewStub.svelte");
  return { default: stub.default };
});

beforeEach(() => {
  vi.clearAllMocks();
  activeView.set("projects");
  inboxUnreadCount.set(0);
  onLogUpdate.mockResolvedValue(vi.fn());
  listInbox.mockResolvedValue([]);
});

describe("App", () => {
  it("renders ProjectsView by default", async () => {
    render(App);
    expect(await screen.findByTestId("projects-view-stub")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test and confirm the harness does not exist yet**

Run:

```bash
cd desktop && npm run test -- --run src/App.test.ts
```

Expected: fail immediately because `package.json` does not yet define a `test` script and the required packages are not installed.

- [ ] **Step 3: Add the minimal test dependencies and scripts**

Update `desktop/package.json` so the scripts and dev dependencies include:

```json
{
  "scripts": {
    "test": "vitest",
    "test:coverage": "vitest run --coverage"
  },
  "devDependencies": {
    "@testing-library/jest-dom": "...",
    "@testing-library/svelte": "^5",
    "@vitest/coverage-v8": "...",
    "jsdom": "...",
    "vitest": "..."
  }
}
```

Install them and refresh the lockfile:

```bash
cd desktop && npm install --save-dev vitest @vitest/coverage-v8 @testing-library/svelte@^5 @testing-library/jest-dom jsdom
```

- [ ] **Step 4: Extend the existing Vite config for Vitest**

Modify `desktop/vite.config.ts` to use `defineConfig` from `vitest/config` and add a `test` block:

```ts
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [sveltekit()],
  // existing server/build settings stay unchanged
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/vitest.setup.ts"],
    include: ["src/**/*.test.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      all: true,
      include: ["src/App.svelte"],
    },
  },
});
```

Create `desktop/src/vitest.setup.ts`:

```ts
import { cleanup } from "@testing-library/svelte";
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";

afterEach(() => {
  cleanup();
});
```

Create the three stub components with stable test IDs:

```svelte
<!-- Example: desktop/src/test/stubs/ProjectsViewStub.svelte -->
<div data-testid="projects-view-stub">ProjectsView stub</div>
```

Use the same pattern for `SidebarStub.svelte` and `InboxViewStub.svelte`.

- [ ] **Step 5: Re-run the first test and make sure it passes**

Run:

```bash
cd desktop && npm run test -- --run src/App.test.ts -t "renders ProjectsView by default"
```

Expected: PASS.

- [ ] **Step 6: Commit the harness**

Run:

```bash
git add desktop/package.json desktop/package-lock.json desktop/vite.config.ts desktop/src/vitest.setup.ts desktop/src/test/stubs desktop/src/App.test.ts
git commit -m "test: add desktop App.svelte test harness"
```

## Task 2: Cover the Happy Path and View Switching

**Files:**
- Modify: `desktop/src/App.test.ts`

- [ ] **Step 1: Add failing tests for the main success path**

Extend `desktop/src/App.test.ts` with:

```ts
import { get } from "svelte/store";
import { appendLog, activeView, inboxUnreadCount } from "./lib/stores";

it("renders InboxView when activeView is inbox", async () => {
  activeView.set("inbox");
  render(App);
  expect(await screen.findByTestId("inbox-view-stub")).toBeInTheDocument();
});

it("registers log updates and seeds the unread count on mount", async () => {
  const unlisten = vi.fn();
  onLogUpdate.mockResolvedValue(unlisten);
  listInbox.mockResolvedValue([{ id: "1" }, { id: "2" }]);

  render(App);

  await waitFor(() => {
    expect(onLogUpdate).toHaveBeenCalledWith(appendLog);
    expect(listInbox).toHaveBeenCalledWith({ status: "unread" });
    expect(get(inboxUnreadCount)).toBe(2);
  });
});

it("calls the unlisten function on normal unmount", async () => {
  const unlisten = vi.fn();
  onLogUpdate.mockResolvedValue(unlisten);

  const { unmount } = render(App);

  await waitFor(() => {
    expect(listInbox).toHaveBeenCalledWith({ status: "unread" });
  });

  unmount();
  expect(unlisten).toHaveBeenCalledTimes(1);
});
```

- [ ] **Step 2: Run only the new tests and verify the failure mode**

Run:

```bash
cd desktop && npm run test -- --run src/App.test.ts -t "registers log updates and seeds the unread count on mount|renders InboxView when activeView is inbox|calls the unlisten function on normal unmount"
```

Expected: at least one new assertion fails before the mocks/setup are complete.

- [ ] **Step 3: Finish the minimal test implementation**

Update `desktop/src/App.test.ts` so it includes the missing imports (`waitFor`, `get`, `appendLog`), keeps store state isolated in `beforeEach`, and leaves `App.svelte` untouched unless a genuine observability gap appears.

Use this `beforeEach` shape:

```ts
beforeEach(() => {
  vi.clearAllMocks();
  activeView.set("projects");
  inboxUnreadCount.set(0);
  onLogUpdate.mockResolvedValue(vi.fn());
  listInbox.mockResolvedValue([]);
});
```

- [ ] **Step 4: Re-run the targeted tests and make sure they pass**

Run:

```bash
cd desktop && npm run test -- --run src/App.test.ts -t "registers log updates and seeds the unread count on mount|renders InboxView when activeView is inbox|calls the unlisten function on normal unmount"
```

Expected: PASS for all three tests.

- [ ] **Step 5: Commit the happy-path coverage**

Run:

```bash
git add desktop/src/App.test.ts
git commit -m "test: cover App.svelte happy paths"
```

## Task 3: Cover Cancellation and Error Paths

**Files:**
- Modify: `desktop/src/App.test.ts`
- Modify only if required: `desktop/src/App.svelte`

- [ ] **Step 1: Add failing tests for the race and error cases**

Add a local deferred helper and three tests:

```ts
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

it("calls a late unlisten function if the component unmounts before onLogUpdate resolves", async () => {
  const gate = deferred<() => void>();
  onLogUpdate.mockReturnValue(gate.promise);
  listInbox.mockResolvedValue([]);

  const { unmount } = render(App);
  unmount();

  const lateUnlisten = vi.fn();
  gate.resolve(lateUnlisten);

  await waitFor(() => {
    expect(lateUnlisten).toHaveBeenCalledTimes(1);
  });
});

it("logs listener registration failures without crashing", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  onLogUpdate.mockRejectedValue(new Error("listener failed"));
  listInbox.mockResolvedValue([]);

  render(App);

  await waitFor(() => {
    expect(error).toHaveBeenCalledWith(
      "Failed to register log-update listener:",
      expect.any(Error),
    );
  });

  error.mockRestore();
});

it("logs unread-count fetch failures without crashing", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  onLogUpdate.mockResolvedValue(vi.fn());
  listInbox.mockRejectedValue(new Error("fetch failed"));

  render(App);

  await waitFor(() => {
    expect(error).toHaveBeenCalledWith(
      "Failed to fetch initial unread count:",
      expect.any(Error),
    );
  });

  error.mockRestore();
});
```

- [ ] **Step 2: Run the new failure-path tests and confirm at least one fails first**

Run:

```bash
cd desktop && npm run test -- --run src/App.test.ts -t "late unlisten|listener registration failures|unread-count fetch failures"
```

Expected: FAIL until the deferred helper, console spies, and async timing are wired correctly.

- [ ] **Step 3: Make the tests deterministic with the smallest possible change**

Keep the fix inside `desktop/src/App.test.ts` if possible:

- use the local `deferred()` helper instead of timers
- restore `console.error` in every test that spies on it
- prefer `waitFor()` over sleeps

Only touch `desktop/src/App.svelte` if the cancellation path cannot be observed without a tiny production-safe change. If that happens, keep the change limited to observability and do not refactor the component.

- [ ] **Step 4: Re-run the failure-path tests and make sure they pass**

Run:

```bash
cd desktop && npm run test -- --run src/App.test.ts -t "late unlisten|listener registration failures|unread-count fetch failures"
```

Expected: PASS.

- [ ] **Step 5: Commit the edge-case coverage**

Run:

```bash
git add desktop/src/App.test.ts desktop/src/App.svelte
git commit -m "test: cover App.svelte edge cases"
```

If `desktop/src/App.svelte` was not modified, omit it from `git add`.

## Task 4: Validate Coverage and Build Stability

**Files:**
- No new files expected

- [ ] **Step 1: Run the full desktop unit test suite**

Run:

```bash
cd desktop && npm run test -- --run
```

Expected: PASS for the complete `App.test.ts` suite.

- [ ] **Step 2: Run coverage and confirm the target metric**

Run:

```bash
cd desktop && npm run test:coverage -- --run
```

Expected:

- the report includes `desktop/src/App.svelte`
- line coverage for `App.svelte` improves by at least 10 percentage points
- no unexpected uncovered branches suggest missing tests from the approved spec

- [ ] **Step 3: Run the desktop build**

Run:

```bash
cd desktop && npm run build
```

Expected: PASS.

- [ ] **Step 4: Do a final git review**

Run:

```bash
git --no-pager status --short
git --no-pager diff -- desktop/package.json desktop/vite.config.ts desktop/src/vitest.setup.ts desktop/src/App.test.ts desktop/src/App.svelte
```

Expected: only the intended desktop test-harness and `App.svelte` test changes are present.

- [ ] **Step 5: Only commit if validation required a follow-up fix**

If Step 4 reveals a real problem that needs one more code change, make the smallest fix, then run:

```bash
git add desktop/package.json desktop/package-lock.json desktop/vite.config.ts desktop/src/vitest.setup.ts desktop/src/test/stubs desktop/src/App.test.ts desktop/src/App.svelte
git commit -m "test: finalize App.svelte coverage validation"
```

If validation passes without additional edits, do not create an empty commit; Tasks 1-3 already capture the implementation history.

## Notes for the Implementer

- Do not widen scope into `ProjectsView.svelte`, `InboxView.svelte`, or `Sidebar.svelte`; the approved design is to stub them.
- Reset `activeView` and `inboxUnreadCount` before every test to prevent order-dependent failures.
- Keep the Vitest config inside `desktop/vite.config.ts`; do not create a parallel `vitest.config.ts` unless the shared config approach proves impossible.
- Prefer direct assertions on `appendLog`, `listInbox`, store state, and unlisten cleanup over snapshots.
- If the first attempt at the cancellation test is flaky, fix the test harness before considering any production change.
