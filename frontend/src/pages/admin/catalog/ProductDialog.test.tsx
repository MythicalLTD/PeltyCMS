// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { I18nProvider } from "@/lib/i18n";
import { ProductDialog, slugify } from "./ProductDialog";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
describe("slugify", () => {
  it("makes server-valid slugs", () => {
    expect(slugify("  Șablon VPS, Pro! ")).toBe("sablon-vps-pro");
  });
});
describe("ProductDialog", () => {
  it("creates a new group, then the product, then saves its prices", async () => {
    const calls: { method: string; url: string; body: unknown }[] = [];
    vi.stubGlobal("fetch", vi.fn(async (u: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      if (method !== "GET") calls.push({ method, url: u, body: init?.body ? JSON.parse(String(init.body)) : null });
      if (u.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [] }), { status: 200 });
      if (u.endsWith("/admin/catalog/groups") && method === "POST") return new Response(JSON.stringify({ id: "g1" }), { status: 200 });
      if (u.endsWith("/admin/catalog/products") && method === "POST") return new Response(JSON.stringify({ id: "p1" }), { status: 200 });
      if (u.endsWith("/currencies")) return new Response(JSON.stringify({ items: [{ code: "EUR" }] }), { status: 200 });
      return new Response(JSON.stringify({ items: [] }), { status: 200 });
    }));
    render(<QueryClientProvider client={new QueryClient()}><I18nProvider><ProductDialog /></I18nProvider></QueryClientProvider>);
    fireEvent.click(await screen.findByRole("button"));
    fireEvent.change(await screen.findByLabelText(/^name/i), { target: { value: "VPS Small" } });
    fireEvent.change(document.getElementById("pd-newgroup")!, { target: { value: "Cloud" } });
    await waitFor(() => expect((screen.getByText(/add price/i).closest("button") as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByText(/add price/i));
    fireEvent.change(document.getElementById("pd-a0")!, { target: { value: "9.99" } });
    fireEvent.submit(document.getElementById("pd-name")!.closest("form")!);
    await waitFor(() => expect(calls.length).toBe(3));
    expect(calls[0]).toMatchObject({ method: "POST", body: { name: "Cloud", slug: "cloud" } });
    expect(calls[1]).toMatchObject({ method: "POST", body: { group_id: "g1", name: "VPS Small", slug: "vps-small", status: "active" } });
    expect(calls[2]).toMatchObject({ method: "PUT", body: { prices: [{ currency: "EUR", cycle: "monthly", amount: "9.99", setup_fee: "0" }] } });
    expect(calls[2]!.url).toMatch(/\/products\/p1\/prices$/);
  });
});
