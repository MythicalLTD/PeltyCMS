// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { CreateClientDialog } from "./CreateClientDialog";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
describe("CreateClientDialog", () => {
  it("posts the new client and opens its page", async () => {
    const calls: { url: string; body: unknown }[] = [];
    vi.stubGlobal("fetch", vi.fn(async (u: string, init?: RequestInit) => {
      if (init?.method === "POST") { calls.push({ url: u, body: JSON.parse(String(init.body)) }); return new Response(JSON.stringify({ id: "c1" }), { status: 200 }); }
      if (u.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [] }), { status: 200 });
      return new Response(JSON.stringify({ items: [] }), { status: 200 });
    }));
    render(<QueryClientProvider client={new QueryClient()}><I18nProvider><MemoryRouter initialEntries={["/admin/customers"]}><Routes>
      <Route path="/admin/customers" element={<CreateClientDialog />} />
      <Route path="/admin/customers/:id" element={<p>opened</p>} />
    </Routes></MemoryRouter></I18nProvider></QueryClientProvider>);
    fireEvent.click(screen.getByRole("button"));
    const email = await screen.findByLabelText(/email/i);
    fireEvent.change(email, { target: { value: "new@pelty.test" } });
    fireEvent.submit(email.closest("form")!);
    await screen.findByText("opened");
    expect(calls[0]!.url).toMatch(/\/admin\/customers$/);
    expect(calls[0]!.body).toMatchObject({ email: "new@pelty.test", login: "invite" });
  });
});
