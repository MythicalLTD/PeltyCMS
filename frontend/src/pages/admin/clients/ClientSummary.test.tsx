// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Link, MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router";
import { AdminClientSummaryPage } from "./ClientSummary";
import type { CustomerWorkspace } from "@/lib/admin/customerWorkspace";

vi.mock("@/lib/query-cache", () => ({ bindCache: async () => {}, clearCache: async () => {} }));
vi.mock("@/lib/addons/plugins", () => ({ usePluginRenderSlots: () => [] }));
vi.mock("@/lib/i18n", async () => {
  const { default: messages } = await import("../../../../../locales/en.json");
  const translate = (key: string) => (messages as Record<string, string>)[key] ?? key;
  return { useI18n: () => ({ lang: "en", t: translate, tCode: translate }) };
});

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
Element.prototype.scrollIntoView ??= () => {};
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as never;

function workspace(id = "c1", manage = false): CustomerWorkspace {
  const customer = { id, email: `${id}@example.test`, first_name: id === "c1" ? "Ada" : "Grace", last_name: "Customer", organization: "Example", status: "active", currency: "USD", phone: "123", address_line1: "Street", address_line2: "", city: "City", region: "", postal_code: "123", country: "US", tax_id: "", created_at: "2026-01-01T00:00:00Z" };
  return {
    customer, access: { finance: false, services: false, support: false, operations: false, audit: false, manage_customer: manage },
    counts: { services_active: null, services_total: null, domains_total: null, tickets_open: null },
    contacts: [], services: [], domains: [], invoices: [], transactions: [], credit_ledger: [], tickets: [], notes: [], log: [], refunds: [], chats: [], emails: [], failures: [],
    attention: [], balances: [], limits: { records: 50 }, totals: {}, meta: {},
  };
}
function LocationTools() {
  const loc = useLocation(); const nav = useNavigate();
  return <><output data-testid="location">{loc.pathname}{loc.search}</output><Link to="/admin/customers/c2?core=1&tab=notes">Other customer</Link><button onClick={() => nav(-1)}>Go back</button></>;
}
function mount(data: CustomerWorkspace, permissions = ["customers.read"], initial = "/admin/customers/c1?core=1", failFirst = false) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  client.setQueryData(["me"], { id: "staff", permissions, csrf_token: "csrf" });
  let workspaceAttempts = 0;
  const fetcher = vi.fn(async (url: string, init?: RequestInit): Promise<Response> => {
    if (url.includes("/workspace")) {
      if (++workspaceAttempts === 1 && failFirst) return Response.json({ error: { code: "error.internal" } }, { status: 500 });
      return Response.json(url.includes("/c2/") ? workspace("c2", data.access.manage_customer) : data);
    }
    if (url.includes("/activity?")) return Response.json({ items: [], next_cursor: null });
    if (url.endsWith("/notes") && init?.method === "POST") return Response.json({ id: "new-note" });
    return Response.json({ items: [], status: {}, documents: [] });
  });
  vi.stubGlobal("fetch", fetcher);
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[initial]}><LocationTools /><Routes><Route path="/admin/customers/:id" element={<AdminClientSummaryPage />} /></Routes></MemoryRouter></QueryClientProvider>);
  return { client, fetcher };
}
function selectTab(name: string) {
  const tab = screen.getByRole("tab", { name });
  fireEvent.mouseDown(tab, { button: 0, ctrlKey: false }); fireEvent.click(tab);
}

describe("unified customer workspace", () => {
  it("defaults to overview and never mounts or requests forbidden customer controls", async () => {
    const { fetcher } = mount(workspace());
    await screen.findByText("Ada Customer");
    expect(screen.getByRole("tab", { name: "Overview" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.queryByRole("tab", { name: "Invoices" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Audit" })).toBeNull();
    expect(screen.queryByText("Identity verification")).toBeNull();
    expect(screen.getByText("Read-only customer access")).toBeTruthy();
    selectTab("Profile");
    selectTab("Contacts");
    selectTab("Email");
    selectTab("Notes");
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(fetcher.mock.calls.every(([u]) => u.includes("/workspace") || u.includes("/activity?"))).toBe(true);
    expect(screen.getByTestId("location").textContent).toContain("core=1&tab=notes");
  });
  it("requires both backend access and actual manage permissions even with a permissive mask", async () => {
    const data = workspace("c1", true);
    data.access = { finance: true, services: true, support: true, operations: true, audit: true, manage_customer: true };
    const { fetcher } = mount(data, ["customers.read"]);
    await screen.findByText("Ada Customer");
    expect(screen.queryByRole("tab", { name: "Invoices" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Operations" })).toBeNull();
    expect(screen.queryByText("Identity verification")).toBeNull();
    selectTab("Notes");
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(fetcher.mock.calls.some(([u]) => /kyc|security-question|subaccounts|email-log/.test(u))).toBe(false);
  });
  it("renders allowed manage actions without eagerly requesting dialog data", async () => {
    const data = workspace("c1", true); data.access.finance = true;
    const { fetcher } = mount(data, ["customers.manage", "invoices.manage"]);
    await screen.findByText("Ada Customer");
    expect(screen.getByRole("button", { name: "Sign in as client" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Identity verification" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Credit" })).toBeTruthy();
    expect(fetcher.mock.calls.every(([u]) => u.includes("/workspace") || u.includes("/activity?"))).toBe(true);
  });
  it("hides credit adjustment from invoice managers without customer manage permission", async () => {
    const data = workspace("c1", true); data.access.finance = true;
    const { fetcher } = mount(data, ["customers.read", "invoices.manage"]);
    await screen.findByText("Ada Customer");
    expect(screen.getByRole("tab", { name: "Invoices" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Credit" })).toBeNull();
    expect(fetcher.mock.calls.some(([u]) => u.endsWith("/credit"))).toBe(false);
  });
  it("allows credit with customer manage plus finance read and refreshes workspace balances after adjustment", async () => {
    const data = workspace("c1", true); data.access.finance = true;
    data.balances = [{ currency: "USD", amount_due: "0.00", amount_paid: "0.00", credit: "5.00", invoices_unpaid: 0 }];
    const { fetcher } = mount(data, ["customers.manage", "invoices.read"]);
    await screen.findByText("5.00 USD");
    await screen.findByText("No activity for this filter.");

    const original = fetcher.getMockImplementation()!;
    fetcher.mockImplementation(async (url, init) => {
      if (url.endsWith("/credit")) {
        if (init?.method === "POST") { data.balances[0]!.credit = "15.00"; return Response.json({ ok: true }); }
        return Response.json({ balances: [], entries: [] });
      }
      return original(url, init);
    });
    fireEvent.click(screen.getByRole("button", { name: "Credit" }));
    await screen.findByRole("dialog");
    fireEvent.change(screen.getByLabelText("Adjustment"), { target: { value: "10.00" } });
    fireEvent.change(screen.getByLabelText("Note"), { target: { value: "Adjustment" } });
    fireEvent.click(screen.getByRole("button", { name: "Post adjustment" }));
    await screen.findByText("15.00 USD");
    const post = fetcher.mock.calls.find(([u, init]) => u.endsWith("/credit") && init?.method === "POST");
    expect(JSON.parse(String(post?.[1]?.body))).toEqual({ currency: "USD", amount: "10.00", note: "Adjustment" });
    expect(fetcher.mock.calls.filter(([u]) => u.includes("/workspace"))).toHaveLength(2);
    expect(fetcher.mock.calls.filter(([u]) => u.includes("/activity?"))).toHaveLength(2);
    expect(fetcher.mock.calls.filter(([u, init]) => u.endsWith("/credit") && init?.method === "GET")).toHaveLength(2);
  });
  it("honors a restrictive backend mask even when the actor has manage permissions", async () => {
    const { fetcher } = mount(workspace(), ["customers.manage", "invoices.manage", "tickets.manage", "services.manage", "jobs.manage", "audit.read"]);
    await screen.findByText("Ada Customer");
    expect(screen.queryByRole("button", { name: "Sign in as client" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Identity verification" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Credit" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Invoices" })).toBeNull();
    selectTab("Notes");
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(fetcher.mock.calls.every(([u]) => u.includes("/workspace") || u.includes("/activity?"))).toBe(true);
  });
  it("does not request the workspace at all without customer read access", async () => {
    const { fetcher } = mount(workspace(), ["invoices.manage"]);
    await screen.findByRole("alert");
    expect(fetcher).not.toHaveBeenCalled();
  });
  it("renders read-level finance safely, with bounded totals and separate exact currency strings", async () => {
    const data = workspace(); data.access.finance = true;
    data.balances = [
      { currency: "USD", amount_due: "9007199254740993.99", amount_paid: "1.01", credit: "0.10", invoices_unpaid: 1 },
      { currency: "EUR", amount_due: "2.50", amount_paid: "9.00", credit: "3.00", invoices_unpaid: 1 },
    ];
    data.attention = [{ kind: "invoice", count: 2 }];
    data.meta.invoices = { total: 72, returned: 50, truncated: true, accessible: true };
    mount(data, ["customers.read", "invoices.read"]);
    await screen.findByText("9007199254740993.99 USD");
    expect(screen.getByText("2.50 EUR")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Credit" })).toBeNull();
    fireEvent.click(screen.getByRole("link", { name: /2 Unpaid invoices/i }));
    await screen.findByText(/50 \/ 72/);
    expect(screen.getByTestId("location").textContent).toContain("core=1&tab=invoices");
    expect(screen.getByText(/Latest records only/)).toBeTruthy();
  });
  it("normalizes a forbidden deep-linked tab without losing other query parameters", async () => {
    mount(workspace(), ["customers.read"], "/admin/customers/c1?core=1&tab=log&other=keep");
    await screen.findByText("Ada Customer");
    await waitFor(() => expect(screen.getByTestId("location").textContent).toContain("core=1&tab=overview&other=keep"));
    expect(screen.getByRole("tab", { name: "Overview" }).getAttribute("aria-selected")).toBe("true");
  });
  it("keeps navigation history URL-backed", async () => {
    mount(workspace()); await screen.findByText("Ada Customer");
    selectTab("Contacts");
    await waitFor(() => expect(screen.getByTestId("location").textContent).toContain("tab=contacts"));
    selectTab("Notes");
    await waitFor(() => expect(screen.getByTestId("location").textContent).toContain("tab=notes"));
    fireEvent.click(screen.getByRole("button", { name: "Go back" }));
    await waitFor(() => expect(screen.getByTestId("location").textContent).toContain("core=1&tab=contacts"));
    expect(screen.getByRole("tab", { name: "Contacts" }).getAttribute("aria-selected")).toBe("true");
  });
  it("offers retry for initial workspace failure", async () => {
    const { fetcher } = mount(workspace(), ["customers.read"], "/admin/customers/c1", true);
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByText("Ada Customer");
    expect(fetcher.mock.calls.filter(([u]) => u.includes("/workspace"))).toHaveLength(2);
  });
  it("refreshes both the snapshot and activity", async () => {
    const { fetcher } = mount(workspace());
    await screen.findByText("No activity for this filter.");
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(fetcher.mock.calls.filter(([u]) => u.includes("/workspace"))).toHaveLength(2));
    await waitFor(() => expect(fetcher.mock.calls.filter(([u]) => u.includes("/activity?"))).toHaveLength(2));
  });
  it("clears note drafts and old customer content while a different customer loads", async () => {
    const { fetcher } = mount(workspace("c1", true), ["customers.manage"], "/admin/customers/c1?core=1&tab=notes");
    await screen.findByLabelText("Note");
    fireEvent.change(screen.getByLabelText("Note"), { target: { value: "Private c1 draft" } });
    let resolve!: (r: Response) => void;
    fetcher.mockImplementationOnce(() => new Promise<Response>(r => { resolve = r; }));
    fireEvent.click(screen.getByRole("link", { name: "Other customer" }));
    expect(screen.queryByText("Ada Customer")).toBeNull();
    expect(screen.queryByDisplayValue("Private c1 draft")).toBeNull();
    await waitFor(() => expect(resolve).toBeTypeOf("function"));
    resolve(Response.json(workspace("c2", true)));
    await screen.findByText("Grace Customer");
    expect((screen.getByLabelText("Note") as HTMLTextAreaElement).value).toBe("");
  });
  it("uses workspace email rows instead of a privileged email-log endpoint", async () => {
    const data = workspace(); data.emails = [{ id: "e1", job_id: null, status: "sent", template: "welcome", subject: "Welcome Ada", created_at: "2026-01-01T00:00:00Z" }];
    const { fetcher } = mount(data, ["customers.read"], "/admin/customers/c1?tab=emails");
    await screen.findAllByText("Welcome Ada");
    expect(fetcher.mock.calls.some(([u]) => /email-log|\/emails/.test(u))).toBe(false);
  });
  it("preserves payment, credit, and refund details", async () => {
    const data = workspace(); data.access.finance = true;
    data.refunds = [{ id: "r", invoice_id: "i", payment_id: "p", amount: "12.34", currency: "EUR", status: "succeeded", to_credit: true, created_at: "2026-01-01T00:00:00Z" }];
    mount(data, ["customers.read", "invoices.read"], "/admin/customers/c1?tab=transactions");
    await screen.findByText("Refunds");
    expect(screen.getByText("Credit ledger")).toBeTruthy();
    expect(screen.getAllByText("12.34 EUR").length).toBeGreaterThan(0);
    const panel = screen.getByRole("tabpanel");
    expect(within(panel).getAllByRole("link").some(link => link.getAttribute("href") === "/admin/invoices/i")).toBe(true);
  });
});
