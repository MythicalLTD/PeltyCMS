// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, useLocation } from "react-router";
import { AdminLiveChatPage } from "./LiveChat";

vi.mock("@/lib/auth", () => ({ useMe: () => ({ data: { permissions: ["tickets.read"], csrf_token: "staff-csrf" } }) }));
vi.mock("@/lib/addons/plugins", () => ({ usePluginRenderSlots: () => [] }));
vi.mock("@/lib/i18n", () => ({ useI18n: () => ({ t: (key: string) => key, tCode: (key: string) => key }) }));
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
function Location() { const loc = useLocation(); return <output data-testid="url">{loc.search}</output>; }

it("renders a read-only selected open chat transcript without write controls or staff sockets", async () => {
  const socket = vi.fn<typeof WebSocket>();
  vi.stubGlobal("WebSocket", socket);
  const fetcher = vi.fn<(url: string, _init?: RequestInit) => Promise<Response>>(async (url: string, _init?: RequestInit) => {
    if (url.includes("/messages?")) return Response.json({ status: "open", messages: [
      { id: 1, author: "visitor", staff_name: null, body: "Please check my service", created_at: "2026-01-01T00:00:00Z" },
      { id: 2, author: "staff", staff_name: "Support", body: "We are investigating", created_at: "2026-01-01T00:01:00Z" },
    ] });
    return Response.json({ unread: 0, sessions: [{ id: "read-only", name: "Customer", email: "customer@example.test", status: "open", staff_unread: 0, ticket_id: null, assigned_to: null }] });
  });
  vi.stubGlobal("fetch", fetcher);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={["/admin/live-chat?session=read-only&status=open"]}><AdminLiveChatPage /></MemoryRouter></QueryClientProvider>);
  await screen.findByText("Please check my service");
  expect(screen.getByText("We are investigating")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "chat.send" })).toBeNull();
  expect(screen.queryByRole("button", { name: "chat.admin.close" })).toBeNull();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.queryByRole("combobox")).toBeNull();
  expect(screen.queryByText("chat.assignment")).toBeNull();
  expect(screen.queryByText("chat.convert")).toBeNull();
  expect(screen.queryByText("chat.settings")).toBeNull();
  expect(screen.queryByText("chat.closed")).toBeNull();
  expect(socket).not.toHaveBeenCalled();
  expect(fetcher.mock.calls.some(([url]) => url.includes("/admin/chats/read-only/messages?after=0"))).toBe(true);
  expect(fetcher.mock.calls.every(([url, init]) => init?.method === "GET" && (url.endsWith("/admin/chats?status=open") || url.includes("/admin/chats/read-only/messages?")))).toBe(true);
});

it("explicitly reports a closed deep link outside the loaded 200 conversations only after the inbox is ready", async () => {
  let resolve!: (response: Response) => void;
  const fetcher = vi.fn<(_url: string) => Promise<Response>>((_url: string) => new Promise<Response>(r => { resolve = r; }));
  vi.stubGlobal("fetch", fetcher);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={["/admin/live-chat?session=older-transcript&status=closed"]}><AdminLiveChatPage /></MemoryRouter></QueryClientProvider>);
  expect(screen.getByRole("status").textContent).toBe("workspace.loading");
  expect(screen.queryByText("workspace.chat_unavailable")).toBeNull();
  resolve(Response.json({ unread: 0, sessions: Array.from({ length: 200 }, (_, i) => ({ id: `recent-${i}`, name: `Recent ${i}`, email: `${i}@example.test`, status: "closed", staff_unread: 0 })) }));
  await screen.findByText("workspace.chat_unavailable");
  expect(screen.queryByText("chat.admin.pick")).toBeNull();
  expect(fetcher).toHaveBeenCalledTimes(1);
  expect(fetcher.mock.calls[0]![0]).toContain("status=closed");
  expect(screen.queryByRole("textbox")).toBeNull();
});

it("shows a retryable inbox error instead of claiming a selected conversation is unavailable", async () => {
  const fetcher = vi.fn<() => Promise<Response>>().mockResolvedValueOnce(Response.json({}, { status: 500 })).mockResolvedValueOnce(Response.json({ unread: 0, sessions: [] }));
  vi.stubGlobal("fetch", fetcher);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={["/admin/live-chat?session=older-transcript&status=closed"]}><AdminLiveChatPage /></MemoryRouter></QueryClientProvider>);
  await screen.findByRole("alert");
  expect(screen.getByText("workspace.chat_load_error")).toBeTruthy();
  expect(screen.queryByText("workspace.chat_unavailable")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "workspace.retry" }));
  await screen.findByText("workspace.chat_unavailable");
  expect(fetcher).toHaveBeenCalledTimes(2);
});

it("opens a closed conversation deep link and preserves unrelated query parameters when selecting", async () => {
  const fetcher = vi.fn<(url: string) => Promise<Response>>(async (url: string) => {
    if (url.includes("/messages?")) return Response.json({ status: "closed", messages: [] });
    return Response.json({ unread: 0, sessions: ["one", "two"].map(id => ({ id, name: `Customer ${id}`, email: `${id}@example.test`, status: "closed", staff_unread: 0, last_body: `Preview ${id}` })) });
  });
  vi.stubGlobal("fetch", fetcher);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={["/admin/live-chat?session=one&status=closed&core=1"]}><Location /><AdminLiveChatPage /></MemoryRouter></QueryClientProvider>);
  await screen.findByText("one@example.test");
  expect(fetcher.mock.calls.some(([u]) => u.endsWith("/admin/chats?status=closed"))).toBe(true);
  await waitFor(() => expect(fetcher.mock.calls.some(([u]) => u.includes("/admin/chats/one/messages?"))).toBe(true));
  fireEvent.click(screen.getByRole("button", { name: /Customer two/ }));
  await screen.findByText("two@example.test");
  expect(screen.getByTestId("url").textContent).toBe("?session=two&status=closed&core=1");
  const open = screen.getByRole("tab", { name: "chat.admin.open" });
  fireEvent.mouseDown(open, { button: 0, ctrlKey: false });
  await waitFor(() => expect(screen.getByTestId("url").textContent).toBe("?status=open&core=1"));
});
