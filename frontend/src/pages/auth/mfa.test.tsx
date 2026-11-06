// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { LoginPage } from "./Login";
import { Verify2faPage } from "./Verify2fa";

// jsdom lacks elementFromPoint; input-otp calls it from a timer.
if (!document.elementFromPoint) document.elementFromPoint = () => null;

const me = { id: "u", email: "a@b.c", display_name: "A", locale: null, email_verified: true, mfa_enabled: true, roles: [], permissions: [], csrf_token: "c" };
let calls: { url: string; body: unknown }[] = [];
let verifyFails = false;
beforeEach(() => {
  calls = [];
  verifyFails = false;
  sessionStorage.clear();
  globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (url.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [{ code: "en", name: "English" }] }), { status: 200 });
    if (url.endsWith("/auth/login")) return new Response(JSON.stringify({ mfa_required: true, challenge: "ch1" }), { status: 200 });
    if (url.endsWith("/auth/mfa/verify")) {
      if (verifyFails) return new Response(JSON.stringify({ error: { code: "error.mfa_invalid_code", message: "" } }), { status: 422 });
      return new Response(JSON.stringify(me), { status: 200 });
    }
    return new Response(JSON.stringify({}), { status: 200 });
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

function wrap() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}><I18nProvider><MemoryRouter initialEntries={["/login"]}>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/auth/verify-2fa" element={<Verify2faPage />} />
        <Route path="/" element={<p>client home</p>} />
      </Routes>
    </MemoryRouter></I18nProvider></QueryClientProvider>,
  );
}

async function toChallenge() {
  wrap();
  fireEvent.change(screen.getByLabelText(/email/i), { target: { value: "ana@example.com" } });
  fireEvent.change(screen.getByLabelText(/^password$/i), { target: { value: "pw" } });
  fireEvent.click(screen.getByRole("button", { name: /sign in|log in/i }));
  await waitFor(() => { expect(document.body.textContent).toMatch(/two-step verification/i); }, { timeout: 3000 });
}

describe("2FA login", () => {
  it("asks for a second factor and signs in with a backup code", async () => {
    await toChallenge();
    fireEvent.click(screen.getByRole("button", { name: /backup code/i }));
    fireEvent.change(screen.getByLabelText(/backup code/i), { target: { value: "abcde-fghjk" } });
    fireEvent.click(screen.getByRole("button", { name: /^verify$/i }));
    await screen.findByText("client home");
    expect(calls.find((c) => c.url.endsWith("/auth/mfa/verify"))?.body).toEqual({ challenge: "ch1", code: "abcde-fghjk" });
  });

  it("shows a translated error for a wrong code", async () => {
    verifyFails = true;
    await toChallenge();
    fireEvent.click(screen.getByRole("button", { name: /backup code/i }));
    fireEvent.change(screen.getByLabelText(/backup code/i), { target: { value: "zzzzz-zzzzz" } });
    fireEvent.click(screen.getByRole("button", { name: /^verify$/i }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toMatch(/invalid authentication code/i));
  });
});
