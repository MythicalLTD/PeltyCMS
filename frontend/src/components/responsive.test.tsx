// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { HardDrivesIcon, UsersIcon } from "@phosphor-icons/react";
import { I18nProvider } from "@/lib/i18n";
import { ResponsiveTable } from "./ResponsiveTable";
import { Shell } from "@/layouts/Shell";

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    const body = url.endsWith("/auth/me")
      ? { id: "u", email: "ana@example.com", display_name: "Ana", locale: null, roles: ["customer"], permissions: [], csrf_token: "c" }
      : url.endsWith("/i18n") ? { languages: [{ code: "en", name: "English" }, { code: "ro", name: "Română" }] } : {};
    return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  }));
  localStorage.clear();
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

function wrap(ui: React.ReactNode, path = "/") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}><I18nProvider><MemoryRouter initialEntries={[path]}>{ui}</MemoryRouter></I18nProvider></QueryClientProvider>,
  );
}

describe("ResponsiveTable", () => {
  const rows = [{ id: 1, name: "VPS S", price: "€10.00" }, { id: 2, name: "VPS M", price: "€20.00" }];
  const cols = [
    { header: "Product", primary: true, cell: (r: (typeof rows)[0]) => r.name },
    { header: "Price", cell: (r: (typeof rows)[0]) => r.price },
    { header: "", cell: (r: (typeof rows)[0]) => <button type="button">Act {r.id}</button> },
  ];
  it("renders a table for md+ and labelled cards for phones", () => {
    wrap(<ResponsiveTable rows={rows} columns={cols} rowKey={(r) => r.id} />);
    const table = screen.getByTestId("rt-table");
    expect(table.className).toContain("hidden md:block");
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Product", "Price", ""]);
    const cards = screen.getByTestId("rt-cards");
    expect(cards.className).toContain("md:hidden");
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    // Each card: title from the primary column, label/value pairs, action row.
    expect(within(items[0]!).getByText("Price").tagName).toBe("DT");
    expect(within(items[0]!).getByText("€10.00").tagName).toBe("DD");
    expect(within(items[1]!).getByRole("button", { name: "Act 2" })).toBeTruthy();
    expect(within(items[0]!).queryByText("Product")).toBeNull();
  });
  it("shows the empty state in both layouts", () => {
    wrap(<ResponsiveTable rows={[]} columns={cols} rowKey={(r) => r.id} />);
    expect(screen.getAllByText("Nothing here yet.")).toHaveLength(2);
  });
});

function shellApp(path = "/dashboard") {
  return wrap(
    <Routes><Route element={<Shell scope="client" section="nav.client_area" slot="client.nav" />}>
      <Route path="dashboard" element={<p>home</p>} /><Route path="services" element={<p>services page</p>} />
    </Route></Routes>, path);
}

function setTestViewport(w: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: w });
  vi.stubGlobal("matchMedia", (q: string) => ({ matches: w < 768, media: q, addEventListener() {}, removeEventListener() {} }));
}

describe("Shell navigation", () => {
  globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver;
  const nav = [
    { to: "/", label: "nav.dashboard" as const, icon: UsersIcon, end: true },
    { to: "/services", label: "nav.services" as const, icon: HardDrivesIcon },
  ];
  void nav;

  it("phone: trigger opens the shadcn Sheet drawer, navigation closes it", async () => {
    setTestViewport(375);
    shellApp();
    const trigger = await screen.findByRole("button", { name: "Open menu" });
    expect(trigger.className).toContain("size-11"); // 44px tap target
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(trigger);
    const sheet = await screen.findByRole("dialog");
    fireEvent.click(within(sheet).getByTestId("navcat-services"));
    expect(JSON.parse(localStorage.getItem("pelty.nav.open.client") ?? "{}").services).toBe(true);
    fireEvent.click(await within(sheet).findByRole("link", { name: "Services" }));
    expect(await screen.findByText("services page")).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("phone: Escape closes the drawer", async () => {
    setTestViewport(375);
    shellApp();
    fireEvent.click(await screen.findByRole("button", { name: "Open menu" }));
    const sheet = await screen.findByRole("dialog");
    fireEvent.keyDown(sheet, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("desktop: sidebar collapses to icons and persists the state", async () => {
    setTestViewport(1280);
    document.cookie = "sidebar_state=; max-age=0; path=/";
    const { container } = shellApp();
    const wrapper = () => container.querySelector('[data-slot="sidebar"][data-state]')!;
    await waitFor(() => expect(wrapper().getAttribute("data-state")).toBe("expanded"));
    fireEvent.click(screen.getByRole("button", { name: "Open menu" }));
    expect(wrapper().getAttribute("data-state")).toBe("collapsed");
    expect(wrapper().getAttribute("data-collapsible")).toBe("icon");
    expect(document.cookie).toContain("sidebar_state=false");
  });
});
