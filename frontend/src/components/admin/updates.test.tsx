// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { I18nProvider } from "@/lib/i18n";
import { UpdateBanner } from "./UpdateCheck";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
function mount(status: number, body: unknown) {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (url.endsWith("/i18n")) return new Response(JSON.stringify({ languages: [] }), { status: 200 });
    return new Response(JSON.stringify(body), { status });
  }));
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={qc}><I18nProvider><UpdateBanner /></I18nProvider></QueryClientProvider>);
}
const base = {
  enabled: true,
  channel: "stable" as const,
  url: "",
  current: "0.1.0",
  latest: "0.2.0",
  release_url: "https://github.com/x/r",
  notes: "",
  published_at: null,
  checked_at: null,
  error: null,
  install: "binary" as const,
  update_command: "pelty-server update --yes --version 0.2.0",
};

describe("UpdateBanner", () => {
  it("shows the new version with a safe external link", async () => {
    mount(200, { ...base, update_available: true });
    const b = await screen.findByTestId("update-banner");
    expect(b.textContent).toContain("0.2.0");
    const a = b.querySelector("a");
    expect(a?.getAttribute("href")).toBe("https://github.com/x/r");
    expect(a?.getAttribute("rel")).toContain("noopener");
  });
  it("renders nothing when up to date or forbidden", async () => {
    mount(200, { ...base, latest: "0.1.0", update_available: false });
    await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalled());
    expect(screen.queryByTestId("update-banner")).toBeNull();
    cleanup();
    mount(403, { error: { code: "error.forbidden", message: "no" } });
    await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalled());
    expect(screen.queryByTestId("update-banner")).toBeNull();
  });
});
