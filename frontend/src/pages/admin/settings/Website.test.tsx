// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { AdminWebsitePage } from "./Website";

const REAL = {"client_dashboard_widgets":["services","invoices","tickets","announcements","kb"],"default_mode":"dark","font":"manrope","footer_columns":[{"links":[{"href":"/store","label":"Plans"}],"title":"Product"}],"footer_text":"","homepage":"home","logo_url":"","nav":[{"href":"/kb","label":"Knowledgebase"}],"primary_color":"","show_store_in_nav":true,"site_name":"PeltyCMS","tagline":"Hosting that just works."};
const PAGE = { id: "p1", slug: "home", title: "Home", description: "", published: true, show_in_nav: false, sort_order: 0, blocks: [{ type: "hero", props: { title: "Hi", subtitle: "" } }] };

function mock(site: unknown, pages: unknown) {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    const body = url.endsWith("/admin/site") ? site : url.endsWith("/admin/site/pages") ? pages
      : url.endsWith("/auth/me") ? { id: "u", email: "a@b.c", display_name: "A", roles: ["admin"], permissions: [], csrf_token: "c" }
      : url.endsWith("/i18n") ? { languages: [{ code: "en", name: "English" }] } : {};
    return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  }));
}
beforeEach(() => localStorage.clear());
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const wrap = () => render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><I18nProvider><MemoryRouter><AdminWebsitePage /></MemoryRouter></I18nProvider></QueryClientProvider>);

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as never;
Element.prototype.scrollIntoView ??= () => {};
describe("AdminWebsitePage", () => {
  it("renders settings with real data", async () => {
    mock({ config: REAL, fonts: [] }, { items: [PAGE] }); wrap();
    await waitFor(() => expect((document.getElementById("s-name") as HTMLInputElement).value).toBe("PeltyCMS"));
  });
  it("survives null / partial config", async () => {
    mock(null, null); wrap();
    await waitFor(() => expect((document.getElementById("s-name") as HTMLInputElement).value).toBe("PeltyCMS"));
    mock({ config: { site_name: "X" } }, [{ id: "p", slug: "x", title: "X" }]); cleanup(); wrap();
    await waitFor(() => expect((document.getElementById("s-name") as HTMLInputElement).value).toBe("X"));
  });
  it("pages tab and block editor render", async () => {
    mock({ config: REAL }, { items: [PAGE] }); wrap();
    const tab = screen.getAllByRole("tab")[1]!; fireEvent.mouseDown(tab, { button: 0, ctrlKey: false }); fireEvent.click(tab);
    expect(await screen.findByText("Home", {}, { timeout: 3000 })).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: /edit|editeaz/i })[0]!);
    expect(await screen.findByDisplayValue("home")).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: /edit|editeaz/i })[0]!);
    expect(await screen.findByDisplayValue("Hi")).toBeTruthy();
  });
  it("saves the config as a JSON object (not a double-encoded string)", async () => {
    mock({ config: REAL, fonts: [] }, { items: [PAGE] }); wrap();
    await waitFor(() => expect((document.getElementById("s-name") as HTMLInputElement).value).toBe("PeltyCMS"));
    fireEvent.change(document.getElementById("s-tag")!, { target: { value: "New tagline" } });
    fireEvent.click(screen.getAllByRole("button", { name: /save/i })[0]!);
    const f = fetch as unknown as { mock: { calls: [string, RequestInit][] } };
    await waitFor(() => expect(f.mock.calls.some(([u, i]) => u.endsWith("/admin/site") && i?.method === "PUT")).toBe(true));
    const [, init] = f.mock.calls.find(([u, i]) => u.endsWith("/admin/site") && i?.method === "PUT")!;
    const sent = JSON.parse(String(init.body));
    expect(typeof sent).toBe("object");
    expect(sent.tagline).toBe("New tagline");
  });
});
