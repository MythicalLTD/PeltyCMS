// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { RegistrantAlert, VerifyRegistrantPage } from "./RegistrantVerification";

let calls: { url: string; method: string; body: unknown }[] = [];
let ok = true;
beforeEach(() => {
  calls = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (url.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [{ code: "en", name: "English" }] }), { status: 200 });
    if (url.endsWith("/public/domains/verify-registrant")) {
      return ok ? new Response(JSON.stringify({ verified: 2 }), { status: 200 })
        : new Response(JSON.stringify({ error: { code: "error.registrant_token_invalid", message: "bad" } }), { status: 422 });
    }
    return new Response(JSON.stringify({}), { status: 200 });
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

function wrap(ui: React.ReactNode, path: string) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={qc}><I18nProvider><MemoryRouter initialEntries={[path]}><Routes><Route path="*" element={ui} /></Routes></MemoryRouter></I18nProvider></QueryClientProvider>);
}

const v = { domain_id: "d1", email: "ana@example.test", status: "pending" as const, deadline: "2026-11-15", sent_at: "2026-10-31T10:00:00Z", reminders: 0, verified_at: null };

describe("registrant verification", () => {
  it("verify page posts the token once and shows the count", async () => {
    ok = true;
    wrap(<VerifyRegistrantPage />, "/verify-registrant?token=abc");
    await screen.findByTestId("registrant-ok");
    const posts = calls.filter((c) => c.url.endsWith("/public/domains/verify-registrant"));
    expect(posts).toHaveLength(1);
    expect(posts[0]?.body).toEqual({ token: "abc" });
  });
  it("verify page shows the error for a bad token", async () => {
    ok = false;
    wrap(<VerifyRegistrantPage />, "/verify-registrant?token=bad");
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.queryByTestId("registrant-ok")).toBeNull();
  });
  it("alert shows for pending, hides when verified, and resends", async () => {
    wrap(<RegistrantAlert domainId="d1" v={v} />, "/");
    expect(screen.getByTestId("registrant-alert").textContent).toContain("ana@example.test");
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(calls.some((c) => c.url.endsWith("/client/domains/d1/registrant-verification/resend") && c.method === "POST")).toBe(true));
    cleanup();
    wrap(<RegistrantAlert domainId="d1" v={{ ...v, status: "verified" }} />, "/");
    expect(screen.queryByTestId("registrant-alert")).toBeNull();
  });
});
