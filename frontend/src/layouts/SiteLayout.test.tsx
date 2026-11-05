// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { SiteLayout } from "./SiteLayout";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); localStorage.clear(); });

function mock(cartStatus: number) {
  const calls: string[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/me")) return new Response(JSON.stringify({ code: "error.unauthorized" }), { status: 401, headers: { "content-type": "application/json" } });
    if (url.includes("/store/carts/")) return new Response(JSON.stringify(cartStatus === 200 ? { cart: { items: [{}, {}], order_id: null }, preview: null, error: null } : { code: "error.not_found" }), { status: cartStatus, headers: { "content-type": "application/json" } });
    const body = url.endsWith("/i18n") ? { languages: [{ code: "en", name: "English" }] } : url.endsWith("/public/site") ? { nav_pages: [], config: { site_name: "Acme", footer_columns: [{ title: "Legal", links: [{ label: "Terms of Service", href: "/page/terms" }] }], social: [{ network: "github", href: "https://github.com/x" }], copyright: "© Acme Ltd" } } : {};
    return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  }));
  return calls;
}
const wrap = () => render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><I18nProvider><MemoryRouter><Routes><Route element={<SiteLayout />}><Route index element={<p>home</p>} /></Route></Routes></MemoryRouter></I18nProvider></QueryClientProvider>);

describe("SiteLayout footer + cart badge", () => {
  it("renders configurable columns, social icons and copyright", async () => {
    mock(404); wrap();
    expect(await screen.findByText("Terms of Service")).toBeTruthy();
    expect(screen.getByLabelText("github").getAttribute("href")).toBe("https://github.com/x");
    expect(screen.getByText("© Acme Ltd")).toBeTruthy();
  });
  it("drops a stale stored cart id on 404 and shows no badge", async () => {
    const stale = "11111111-1111-4111-8111-111111111111";
    localStorage.setItem("pelty.cart", stale);
    const calls = mock(404); wrap();
    await screen.findByText("Terms of Service");
    await vi.waitFor(() => expect(localStorage.getItem("pelty.cart")).not.toBe(stale));
    expect(calls.some((u) => u.includes(stale))).toBe(true);
    expect(screen.queryByText("2")).toBeNull();
  });
});
