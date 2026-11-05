// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TicketAiSuggestion } from "./TicketAiSuggestion";
vi.mock("@/lib/i18n", () => ({ useI18n: () => ({ t: (key: string) => key, tCode: (key: string) => key }) }));
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
function mount(enabled = true) {
  const insert = vi.fn();
  const fetcher = vi.fn(async (url: string) => new Response(JSON.stringify(url.endsWith("/status") ? { enabled } : { body: "Suggested <b>text</b>" }), { status: 200 }));
  vi.stubGlobal("fetch", fetcher);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(<QueryClientProvider client={client}><TicketAiSuggestion ticketId="ticket-id" onInsert={insert} /></QueryClientProvider>);
  return { insert, fetcher };
}
it("generates only on request and inserts only after review, never sends", async () => {
  const { insert, fetcher } = mount();
  fireEvent.click(await screen.findByText("tickets.ai_suggest"));
  const preview = await screen.findByText("Suggested <b>text</b>");
  expect(preview.querySelector("b")).toBeNull();
  expect(insert).not.toHaveBeenCalled();
  expect(fetcher.mock.calls.some(([url]) => url.includes("/reply"))).toBe(false);
  fireEvent.click(screen.getByText("tickets.ai_insert"));
  expect(insert).toHaveBeenCalledWith("Suggested <b>text</b>");
  await waitFor(() => expect(screen.queryByText("Suggested <b>text</b>")).toBeNull());
});
it("discards without changing the editor", async () => {
  const { insert } = mount();
  fireEvent.click(await screen.findByText("tickets.ai_suggest"));
  fireEvent.click(await screen.findByText("tickets.ai_discard"));
  expect(insert).not.toHaveBeenCalled();
});
it("stays hidden while disabled", async () => {
  const { fetcher } = mount(false);
  await waitFor(() => expect(fetcher).toHaveBeenCalled());
  expect(screen.queryByText("tickets.ai_suggest")).toBeNull();
});
