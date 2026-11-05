// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { Shell } from "@/layouts/Shell";
import { PageHeader } from "@/components/PageHeader";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { addFilter, emitHost, on, resetBus } from "@/lib/addons/addon-events";

const frame = { src: "/api/v1/addons/demo/assets/ui.html", height: 200 };
const UI = {
  items: [],
  pages: [{ plugin: "demo", id: "panel", area: "admin", title: "plugin.demo.panel", nav: true, data: {}, body: null, frame, bridge: ["/api/v1/auth/me"] }],
  slots: [
    { plugin: "demo", slot: "admin.layout.banner", data: {}, body: { type: "alert", text: "addon banner" } },
    { plugin: "demo", slot: "admin.invoices.toolbar", data: {}, body: { type: "badge", text: "addon toolbar" } },
    { plugin: "demo", slot: "admin.*.after", data: {}, body: { type: "text", text: "addon after" } },
    { plugin: "demo", slot: "admin.services.after", data: {}, body: { type: "text", text: "other page" } },
    { plugin: "demo", slot: "admin.invoices.row-actions", data: {}, body: { type: "text", text: "row {{ctx.row_id}}" } },
    { plugin: "demo", slot: "admin.invoices.before", data: {}, body: null, frame: { src: "/api/v1/addons/demo/assets/w.html", height: 90 }, bridge: [] },
    { plugin: "demo", slot: "admin.invoices.before:list", data: {}, body: { type: "text", text: "above list" } },
    { plugin: "demo", slot: "admin.invoices.after:list", data: {}, body: { type: "text", text: "below list" } },
    { plugin: "demo", slot: "admin.invoices.replace:summary", data: {}, body: { type: "text", text: "addon summary" } },
  ],
  overrides: { pages: [{ plugin: "demo", route: "admin.services", page: "panel" }], hide_nav: [] as Array<{ plugin: string; path: string }> },
};
const calls: { url: string; init?: RequestInit }[] = [];

beforeEach(() => {
  calls.length = 0;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    const body = url.endsWith("/auth/me")
      ? { id: "u", email: "ana@example.com", display_name: "Ana", locale: null, roles: ["admin"], permissions: ["admin.access"], csrf_token: "c" }
      : url.endsWith("/plugins/ui") ? UI
      : url.endsWith("/ui-token") ? { token: "tok-1", expires_in: 300 }
      : url.endsWith("/i18n") ? { languages: [{ code: "en", name: "English" }] } : { ok: true };
    return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  }));
  vi.stubGlobal("matchMedia", (q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} }));
  localStorage.clear();
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); resetBus(); UI.overrides.hide_nav = []; });

function Invoices() {
  return (
    <div>
      <PageHeader title="Invoices" />
      <Replaceable section="list"><ResponsiveTable rows={[{ id: 7 }]} columns={[{ header: "Id", cell: (r: { id: number }) => String(r.id) }]} rowKey={(r) => r.id} /></Replaceable>
      <Replaceable section="summary"><p>core summary</p></Replaceable>
    </div>
  );
}

function wrap(path: string) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}><I18nProvider><MemoryRouter initialEntries={[path]}>
      <Routes><Route element={<Shell scope="admin" section="nav.admin" slot="admin.nav" />}>
        <Route path="/admin/invoices" element={<Invoices />} />
        <Route path="/admin/services" element={<p>core services</p>} />
      </Route></Routes>
    </MemoryRouter></I18nProvider></QueryClientProvider>,
  );
}

/** First render lazy-loads the admin layout and page chunks; under a busy
 * machine that cold import can exceed the 1 s default. */
const SLOW = { timeout: 10_000 };

describe("addon slots on every page", { timeout: 20_000 }, () => {
  it("fills layout, route and global slots, toolbar and row actions", async () => {
    wrap("/admin/invoices");
    expect(await screen.findByText("addon banner", undefined, SLOW)).toBeTruthy();
    expect(await screen.findByText("addon toolbar", undefined, SLOW)).toBeTruthy();
    expect(screen.getByText("addon after")).toBeTruthy();
    expect(screen.queryByText("other page")).toBeNull();
    expect(screen.getAllByText("row 7").length).toBeGreaterThan(0);
  });

  it("renders frames sandboxed and answers only allowed bridge calls", async () => {
    const { container } = wrap("/admin/invoices");
    await waitFor(() => expect(container.querySelector("iframe[data-addon-frame=demo]")).toBeTruthy());
    const f = container.querySelector("iframe[data-addon-frame=demo]") as HTMLIFrameElement;
    expect(f.getAttribute("sandbox")).toBe("allow-scripts allow-forms");
    expect(f.getAttribute("src")).toBe("/api/v1/addons/demo/assets/w.html");
    expect(f.style.height).toBe("90px");
    const posted: unknown[] = [];
    const win = f.contentWindow!;
    vi.spyOn(win, "postMessage").mockImplementation((m: unknown) => { posted.push(m); });
    const send = (data: unknown, source: MessageEventSource | null = win) => act(() => { window.dispatchEvent(new MessageEvent("message", { data, source })); });

    send({ type: "pelty:ready" });
    await waitFor(() => expect(posted.some((m) => (m as { type: string }).type === "pelty:init")).toBe(true));
    send({ type: "pelty:resize", height: 333 });
    await waitFor(() => expect(f.style.height).toBe("333px"));
    send({ type: "pelty:resize", height: 999 }, window); // not from the frame: ignored
    expect(f.style.height).toBe("333px");

    send({ type: "pelty:request", id: 1, path: "/api/v1/addons/demo/notes", method: "GET" });
    send({ type: "pelty:request", id: 2, path: "/api/v1/admin/users", method: "GET" });
    await waitFor(() => expect(posted.filter((m) => (m as { type: string }).type === "pelty:response").length).toBe(2));
    const res = (id: number) => posted.find((m) => (m as { type: string; id: number }).type === "pelty:response" && (m as { id: number }).id === id) as { ok: boolean; status: number };
    expect(res(1).ok).toBe(true);
    expect(res(2)).toMatchObject({ ok: false, status: 403 });
    expect(calls.some((c) => c.url.endsWith("/admin/users"))).toBe(false);
    const own = calls.find((c) => c.url.endsWith("/addons/demo/notes"))!;
    expect(new Headers(own.init?.headers).get("x-pelty-ui-token")).toBe("tok-1");

    // Event bus: init carries the viewer; subscriptions are forwarded,
    // emits are namespaced by the addon id.
    await waitFor(() => expect(posted.some((m) => (m as { type: string; viewer?: { display_name: string } }).type === "pelty:init" && (m as { viewer?: { display_name: string } }).viewer?.display_name === "Ana")).toBe(true));
    const got: string[] = [];
    on("demo:*", (n) => got.push(n));
    send({ type: "pelty:subscribe", events: ["record.*"] });
    send({ type: "pelty:emit", name: "ping", payload: { n: 1 } });
    expect(got).toEqual(["demo:ping"]);
    act(() => { emitHost("record.viewed", { kind: "invoices", id: "9", area: "admin" }); emitHost("ui.locale_changed", { locale: "ro" }); });
    const events = posted.filter((m) => (m as { type: string }).type === "pelty:event") as Array<{ name: string; payload: unknown }>;
    expect(events.map((e) => e.name)).toEqual(["record.viewed"]);
    expect(events[0]!.payload).toEqual({ kind: "invoices", id: "9", area: "admin" });
  });

  it("wraps and replaces sections", async () => {
    wrap("/admin/invoices");
    expect(await screen.findByText("above list", undefined, SLOW)).toBeTruthy();
    expect(screen.getByText("below list")).toBeTruthy();
    expect(screen.getAllByText("7").length).toBeGreaterThan(0);
    expect(screen.getByText("addon summary")).toBeTruthy();
    expect(screen.queryByText("core summary")).toBeNull();
  });

  it("hides nav entries and runs the nav and title filters", async () => {
    UI.overrides.hide_nav = [{ plugin: "demo", path: "/admin/customers" }];
    addFilter("nav.items", (items) => items.map((i) => ({ ...i, label: `${i.label} (filtered)` })));
    addFilter("page.title", (t) => `${t} ★`);
    addFilter("page.title", () => 42 as unknown as string, -1); // wrong type: skipped
    const { container } = wrap("/admin/invoices");
    await waitFor(() => expect(container.querySelector("a[href='/admin/p/demo/panel']")).toBeTruthy(), SLOW);
    expect(container.querySelector("a[href='/admin/p/demo/panel']")!.textContent).toContain("(filtered)");
    expect(container.querySelector("a[href='/admin/customers']")).toBeNull();
    expect(container.querySelector("a[href='/admin/invoices']")).toBeTruthy();
    expect(screen.getAllByText(/★$/).length).toBeGreaterThan(0);
  });

  it("lets an addon page override a core route, with ?core=1 as escape hatch", async () => {
    const { container } = wrap("/admin/services");
    await waitFor(() => expect(container.querySelector("[data-overridden='admin.services']")).toBeTruthy(), SLOW);
    expect(screen.queryByText("core services")).toBeNull();
    cleanup();
    wrap("/admin/services?core=1");
    expect(await screen.findByText("core services", undefined, SLOW)).toBeTruthy();
  });
});
