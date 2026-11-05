// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Link, MemoryRouter } from "react-router";
import { DataTable } from "./DataTable";

vi.mock("@/lib/addons/plugins", () => ({ usePluginRenderSlots: () => [] }));
vi.mock("@/lib/i18n", () => ({ useI18n: () => ({ t: (key: string) => key, tCode: (key: string) => key }) }));
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const first = "11111111-1111-4111-8111-111111111111";
const second = "22222222-2222-4222-8222-222222222222";
function mount(path = "/admin/services") {
  const fetcher = vi.fn(async (url: string) => {
    const params = new URL(url, "https://example.test").searchParams;
    return Response.json({ items: [{ id: params.get("q") ?? "unfiltered" }], page: Number(params.get("page")), per_page: 25, total: 75 });
  });
  vi.stubGlobal("fetch", fetcher);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[`/admin/services?q=${first}&core=1`]}>
    <Link to={`/admin/services?q=${second}&core=1`}>Second service</Link>
    <Link to={`/admin/services?q=${first}&core=1`}>First service</Link>
    <DataTable title="Services" path={path} queryKey={["admin", "test-table"]} columns={[{ header: "ID", cell: row => row.id, primary: true }]} />
  </MemoryRouter></QueryClientProvider>);
  return fetcher;
}

it("forwards the service deep-link UUID from the URL to the API and retains it while paginating", async () => {
  const fetcher = mount();
  await screen.findAllByText(first);
  const initial = new URL(fetcher.mock.calls[0]![0], "https://example.test");
  expect(initial.pathname).toBe("/api/v1/admin/services");
  expect(initial.searchParams.get("q")).toBe(first);
  expect(initial.searchParams.get("page")).toBe("1");
  expect(initial.searchParams.get("per_page")).toBe("25");
  expect(initial.searchParams.has("core")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "common.next" }));
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
  const next = new URL(fetcher.mock.calls[1]![0], "https://example.test");
  expect(next.searchParams.get("q")).toBe(first);
  expect(next.searchParams.get("page")).toBe("2");
});

it("separates cached services by URL filter and resets the page on query navigation", async () => {
  const fetcher = mount();
  await screen.findAllByText(first);
  fireEvent.click(screen.getByRole("button", { name: "common.next" }));
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("link", { name: "Second service" }));
  await screen.findAllByText(second);
  const secondRequest = new URL(fetcher.mock.calls.at(-1)![0], "https://example.test");
  expect(secondRequest.searchParams.get("q")).toBe(second);
  expect(secondRequest.searchParams.get("page")).toBe("1");
  expect(screen.queryByText(first)).toBeNull();
  fireEvent.click(screen.getByRole("link", { name: "First service" }));
  await screen.findAllByText(first);
  const returnRequest = new URL(fetcher.mock.calls.at(-1)![0], "https://example.test");
  expect(returnRequest.searchParams.get("q")).toBe(first);
  expect(returnRequest.searchParams.get("page")).toBe("1");
});

it("does not forward the service URL filter to unrelated tables or disturb explicit path parameters", async () => {
  const fetcher = mount("/admin/jobs?status=failed");
  await screen.findAllByText("unfiltered");
  const request = new URL(fetcher.mock.calls[0]![0], "https://example.test");
  expect(request.pathname).toBe("/api/v1/admin/jobs");
  expect(request.searchParams.has("q")).toBe(false);
  expect(request.searchParams.get("status")).toBe("failed");
});
