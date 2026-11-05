// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { I18nProvider } from "@/lib/i18n";
import { DuoStep } from "./Duo";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
function mount(url: string, go: (u: string) => void) {
  vi.stubGlobal("fetch", vi.fn(async (u: string) => {
    if (u.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [] }), { status: 200 });
    return new Response(JSON.stringify({ url }), { status: 200 });
  }));
  const qc = new QueryClient();
  render(<QueryClientProvider client={qc}><I18nProvider><MemoryRouter><DuoStep challenge="ch" onRestart={() => {}} go={go} /></MemoryRouter></I18nProvider></QueryClientProvider>);
}
describe("DuoStep", () => {
  it("redirects to the https Duo URL", async () => {
    const go = vi.fn<(url: string) => void>();
    mount("https://api-x.duosecurity.com/oauth/v1/authorize?x=1", go);
    fireEvent.click(screen.getByRole("button", { name: /duo/i }));
    await waitFor(() => expect(go).toHaveBeenCalledWith("https://api-x.duosecurity.com/oauth/v1/authorize?x=1"));
  });
  it("refuses a non-https URL", async () => {
    const go = vi.fn<(url: string) => void>();
    mount("javascript:alert(1)", go);
    fireEvent.click(screen.getByRole("button", { name: /duo/i }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(go).not.toHaveBeenCalled();
  });
});
