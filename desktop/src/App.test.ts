import { render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import App from "./App.svelte";
import { appendLog, activeView, inboxUnreadCount } from "./lib/stores";

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

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    activeView.set("projects");
    inboxUnreadCount.set(0);
    onLogUpdate.mockResolvedValue(vi.fn());
    listInbox.mockResolvedValue([]);
  });

  it("renders ProjectsView by default", async () => {
    render(App);
    expect(await screen.findByTestId("projects-view-stub")).toBeInTheDocument();
  });

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
  expect(get(inboxUnreadCount)).toBe(0);
});
});
