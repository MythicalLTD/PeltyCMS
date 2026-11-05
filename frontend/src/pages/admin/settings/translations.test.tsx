// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { AdminTranslationsPage, placeholders } from "./Translations";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe("translation editor", () => {
  it("extracts placeholders", () => {
    expect(placeholders("Hi {{name}}, {{ due }} {{name}}")).toEqual(["{{due}}", "{{name}}"]);
    expect(placeholders("PeltyCMS {version} · {{company}} { not one }")).toEqual(["{version}", "{{company}}"]);
    expect(placeholders("none")).toEqual([]);
  });
  it("lists rows and saves an override", async () => {
    const calls: { url: string; method: string; body?: unknown }[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined });
      if (url.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [] }), { status: 200 });
      if (url.includes("/admin/translations?")) return new Response(JSON.stringify({ items: [{ key: "a.b", base: "Hello {{name}}", file: "Salut {{name}}", override: null }], total: 1, page: 1, per_page: 25, overrides: 0, languages: ["en", "ro"] }), { status: 200 });
      return new Response(JSON.stringify({}), { status: 200 });
    }));
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(<QueryClientProvider client={qc}><I18nProvider><MemoryRouter><AdminTranslationsPage /></MemoryRouter></I18nProvider></QueryClientProvider>);
    const box = await screen.findByLabelText("a.b");
    expect((box as HTMLTextAreaElement).value).toBe("Salut {{name}}");
    expect(screen.getByText("Hello {{name}}")).toBeTruthy();
    fireEvent.change(box, { target: { value: "Bună {{name}}" } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => expect(calls.some((c) => c.method === "PUT" && c.url.endsWith("/admin/translations"))).toBe(true));
    expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ lang: "ro", key: "a.b", value: "Bună {{name}}" });
  });
});
