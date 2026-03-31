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
