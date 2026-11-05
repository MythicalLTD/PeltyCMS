// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { ForgotPasswordPage, ResetPasswordPage, VerifyEmailPage } from "../auth/PasswordReset";
import { VerifyBanner } from "@/components/account/VerifyBanner";

let calls: { url: string; method: string; body: unknown }[] = [];
let verified = false;
let perms: string[] = [];
beforeEach(() => {
  calls = [];
  perms = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (url.endsWith("/auth/me")) {
      return new Response(JSON.stringify({ id: "u", email: "a@b.c", display_name: "A", locale: null, email_verified: verified, roles: [], permissions: perms, csrf_token: "c" }), { status: 200 });
    }
    if (url.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [{ code: "en", name: "English" }] }), { status: 200 });
    if (url.endsWith("/auth/email/verify")) {
      return new Response(JSON.stringify({ error: { code: "error.invalid_token", message: "This link is invalid or has expired." } }), { status: 422 });
    }
    return new Response(null, { status: 204 });
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

function wrap(ui: React.ReactNode, path: string) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}><I18nProvider><MemoryRouter initialEntries={[path]}>
      <Routes><Route path="*" element={ui} /><Route path="/login" element={<p>login page</p>} /></Routes>
    </MemoryRouter></I18nProvider></QueryClientProvider>,
  );
}

describe("account recovery pages", () => {
  it("forgot password always shows the neutral confirmation", async () => {
    wrap(<ForgotPasswordPage />, "/forgot-password");
    fireEvent.change(screen.getByLabelText(/email/i), { target: { value: "x@y.z" } });
    fireEvent.click(screen.getByRole("button", { name: /reset link/i }));
    await screen.findByTestId("forgot-sent");
    expect(calls.find((c) => c.url.endsWith("/auth/password/forgot"))?.body).toEqual({ email: "x@y.z" });
  });

  it("reset blocks mismatched passwords and posts the token", async () => {
    wrap(<ResetPasswordPage />, "/reset-password?token=abc123");
    const submit = screen.getByRole("button", { name: /set new password/i });
    fireEvent.change(screen.getByLabelText("New password"), { target: { value: "Long-Enough-Pass-1" } });
    fireEvent.change(screen.getByLabelText("Confirm new password"), { target: { value: "different" } });
    expect(screen.getByText("Passwords don't match.")).toBeTruthy();
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Confirm new password"), { target: { value: "Long-Enough-Pass-1" } });
    fireEvent.click(submit);
    await screen.findByText("login page");
    expect(calls.find((c) => c.url.endsWith("/auth/password/reset"))?.body).toEqual({ token: "abc123", password: "Long-Enough-Pass-1" });
  });

  it("reset without a token explains the link is invalid", () => {
    wrap(<ResetPasswordPage />, "/reset-password");
    expect(screen.getByRole("alert").textContent).toMatch(/invalid or has expired/);
  });

  it("verify page shows the server error for a bad token, calling the API once", async () => {
    wrap(<VerifyEmailPage />, "/verify-email?token=bad");
    await waitFor(() => expect(screen.getByRole("alert").textContent).toMatch(/expired/));
    expect(calls.filter((c) => c.url.endsWith("/auth/email/verify"))).toHaveLength(1);
  });

  it("banner appears only for unverified users and resends", async () => {
    verified = false;
    wrap(<VerifyBanner />, "/");
    fireEvent.click(await screen.findByRole("button", { name: /resend/i }));
    await screen.findByText(/new confirmation link/);
    expect(calls.some((c) => c.url.endsWith("/auth/email/resend") && c.method === "POST")).toBe(true);
    cleanup();
    verified = true;
    wrap(<VerifyBanner />, "/");
    await waitFor(() => expect(calls.filter((c) => c.url.endsWith("/auth/me")).length).toBeGreaterThan(1));
    expect(screen.queryByTestId("verify-banner")).toBeNull();
  });

  it("banner can be dismissed for the session", async () => {
    verified = false;
    sessionStorage.clear();
    wrap(<VerifyBanner />, "/");
    fireEvent.click(await screen.findByTestId("verify-banner-dismiss"));
    expect(screen.queryByTestId("verify-banner")).toBeNull();
    expect(sessionStorage.getItem("pelty_verify_banner_dismissed")).toBe("1");
    cleanup();
    wrap(<VerifyBanner />, "/");
    await waitFor(() => expect(calls.filter((c) => c.url.endsWith("/auth/me")).length).toBeGreaterThan(0));
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId("verify-banner")).toBeNull();
    sessionStorage.clear();
  });

  it("banner is hidden for staff even when unverified", async () => {
    verified = false;
    perms = ["admin.access"];
    wrap(<VerifyBanner />, "/");
    await waitFor(() => expect(calls.some((c) => c.url.endsWith("/auth/me"))).toBe(true));
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId("verify-banner")).toBeNull();
  });
});
