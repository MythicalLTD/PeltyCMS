// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { WorkspaceActivity } from "./WorkspaceActivity";
import { WorkspaceNotes } from "./WorkspaceNotes";
import { activityKey, workspaceKey, type ActivityItem, type WorkspaceAccess } from "@/lib/admin/customerWorkspace";

vi.mock("@/lib/i18n", async () => {
  const { default: messages } = await import("../../../../locales/en.json");
  const translate = (key: string) => (messages as Record<string, string>)[key] ?? key;
  return { useI18n: () => ({ lang: "en", t: translate, tCode: translate }) };
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const access: WorkspaceAccess = { finance: true, services: true, support: true, operations: true, audit: true, manage_customer: true };
const event = (id: string, kind: ActivityItem["kind"] = "note", title = "Customer note"): ActivityItem => ({ id: `${kind}:${id}`, kind, title, status: null, occurred_at: "2026-01-01T00:00:00Z", href: "/admin/customers/c1" });
function mount(element: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } } });
  const view = render(<QueryClientProvider client={client}><MemoryRouter>{element}</MemoryRouter></QueryClientProvider>);
  return { client, ...view };
}

describe("workspace activity interactions", () => {
  it("filters and paginates with the exact cursor, replacing pages on filter change", async () => {
    const cursor = '{"key":"note:a+b&%","kind":"all"}';
    const fetcher = vi.fn(async (url: string) => {
      const params = new URL(url, "https://example.test").searchParams;
      if (params.get("kind") === "invoice") return Response.json({ items: [event("i", "invoice", "INV-123")], next_cursor: null });
      if (params.has("before")) return Response.json({ items: [event("second")], next_cursor: null });
      return Response.json({ items: [event("first")], next_cursor: cursor });
    });
    vi.stubGlobal("fetch", fetcher);
    mount(<WorkspaceActivity customerId="c1" access={access} />);
    expect(screen.getByText("Records are listed by creation time with their current status. This is not a complete history of status changes.")).toBeTruthy();
    await screen.findByRole("link", { name: "Customer note" });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => expect(screen.getAllByRole("link", { name: "Customer note" })).toHaveLength(2));
    expect(new URL(fetcher.mock.calls[1]![0], "https://example.test").searchParams.get("before")).toBe(cursor);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "invoice" } });
    await screen.findByRole("link", { name: "INV-123" });
    expect(screen.queryByRole("link", { name: "Customer note" })).toBeNull();
    expect(screen.getByRole("link", { name: "INV-123" }).getAttribute("href")).toBe("/admin/invoices/i");
    const lastUrl = new URL(fetcher.mock.calls.at(-1)![0], "https://example.test");
    expect(lastUrl.searchParams.get("kind")).toBe("invoice");
    expect(lastUrl.searchParams.has("before")).toBe(false);
  });
  it("retries initial errors and failed pagination without discarding earlier events", async () => {
    let calls = 0;
    const fetcher = vi.fn(async (url: string) => {
      calls++;
      if (calls === 1 || calls === 3) return Response.json({}, { status: 500 });
      if (url.includes("before=")) return Response.json({ items: [event("second", "email", "Email delivery")], next_cursor: null });
      return Response.json({ items: [event("first")], next_cursor: "opaque" });
    });
    vi.stubGlobal("fetch", fetcher);
    mount(<WorkspaceActivity customerId="c1" access={access} />);
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByRole("link", { name: "Customer note" });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await screen.findByRole("alert");
    expect(screen.getByRole("link", { name: "Customer note" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByRole("link", { name: "Email delivery" });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(fetcher.mock.calls[3]![0]).toContain("before=opaque");
  });
  it("never exposes restricted filters and resets pagination when the customer changes", async () => {
    const fetcher = vi.fn(async (_url: string) => Response.json({ items: [event("one")], next_cursor: "cursor" }));
    vi.stubGlobal("fetch", fetcher);
    const restricted = { ...access, finance: false, services: false, support: false, operations: false, audit: false };
    const { client, rerender } = mount(<WorkspaceActivity key="c1" customerId="c1" access={restricted} />);
    await screen.findByRole("link", { name: "Customer note" });
    expect(screen.queryByRole("option", { name: "Invoice" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
    rerender(<QueryClientProvider client={client}><MemoryRouter><WorkspaceActivity key="c2" customerId="c2" access={restricted} /></MemoryRouter></QueryClientProvider>);
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(3));
    expect(fetcher.mock.calls.at(-1)?.[0]).toContain("/customers/c2/activity?kind=all&limit=25");
    expect(fetcher.mock.calls.at(-1)?.[0]).not.toContain("before=");
  });
});

describe("inline workspace notes", () => {
  it("validates input, trims the saved note and invalidates workspace plus all activity filters", async () => {
    const fetcher = vi.fn(async () => Response.json({ id: "n" }));
    vi.stubGlobal("fetch", fetcher);
    const { client } = mount(<WorkspaceNotes customerId="c1" />);
    client.setQueryData(workspaceKey("c1"), {});
    client.setQueryData([...activityKey("c1"), "all"], {});
    client.setQueryData([...activityKey("c1"), "note"], {});
    client.setQueryData(workspaceKey("c2"), {});
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByRole("alert");
    expect(fetcher).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Note"), { target: { value: "a".repeat(10_001) } });
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    expect(fetcher).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Note"), { target: { value: "  Call back tomorrow  " } });
    fireEvent.click(screen.getByLabelText("Pin this note"));
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByRole("status");
    const [url, init] = fetcher.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("/api/v1/admin/customers/c1/notes");
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({ body: "Call back tomorrow", sticky: true });
    expect((screen.getByLabelText("Note") as HTMLTextAreaElement).value).toBe("");
    expect(client.getQueryState(workspaceKey("c1"))?.isInvalidated).toBe(true);
    expect(client.getQueryState([...activityKey("c1"), "all"])?.isInvalidated).toBe(true);
    expect(client.getQueryState([...activityKey("c1"), "note"])?.isInvalidated).toBe(true);
    expect(client.getQueryState(workspaceKey("c2"))?.isInvalidated).toBe(false);
  });
  it("keeps the draft after save failure and allows retry", async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(Response.json({}, { status: 500 })).mockResolvedValueOnce(Response.json({ id: "n" }));
    vi.stubGlobal("fetch", fetcher);
    mount(<WorkspaceNotes customerId="c1" />);
    fireEvent.change(screen.getByLabelText("Note"), { target: { value: "Keep this draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByRole("alert");
    expect((screen.getByLabelText("Note") as HTMLTextAreaElement).value).toBe("Keep this draft");
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByText("Note saved.");
    expect(fetcher).toHaveBeenCalledTimes(2);
  });
});
