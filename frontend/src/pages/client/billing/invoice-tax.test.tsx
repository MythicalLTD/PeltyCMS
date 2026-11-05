// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { I18nProvider } from "@/lib/i18n";
import { InvoiceLines } from "./InvoiceDetail";
import type { InvoiceDetail } from "@/lib/billing/billing";

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ languages: [{ code: "en", name: "English" }] }), { status: 200 })));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const base: InvoiceDetail = {
  invoice: { id: "i", number: "INV-000001", kind: "invoice", customer_id: "c", currency: "EUR", status: "unpaid", subtotal: "100.0000", tax_total: "15.4700", total: "115.4700", amount_paid: "0", issued_at: null, due_at: null, paid_at: null, created_at: "2026-01-01T00:00:00Z",
    tax_breakdown: [{ name: "GST", rate: "5.0000", compound: false, amount: "5.0000" }, { name: "QST", rate: "9.9750", compound: true, amount: "10.4700" }] },
  items: [{ id: "a", description: "VPS", quantity: "1", unit_price: "100", tax_rate: "15.4738", amount: "100", tax_amount: "15.47" }],
  payments: [], balance: "115.4700",
};

describe("invoice tax display", () => {
  it("shows one row per tax in the breakdown", () => {
    render(<I18nProvider><InvoiceLines d={base} /></I18nProvider>);
    expect(screen.getAllByText(/GST 5 %/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/QST 9[.,]975 %/).length).toBeGreaterThan(0);
    expect(screen.queryByTestId("tax-notes")).toBeNull();
  });
  it("shows the reverse charge note and VAT number", () => {
    const d = { ...base, invoice: { ...base.invoice, tax_breakdown: [], tax_total: "0", reverse_charge: true, customer_snapshot: { vat_number: "DE123456789" } } };
    render(<I18nProvider><InvoiceLines d={d} /></I18nProvider>);
    expect(screen.getByTestId("tax-notes").textContent).toContain("DE123456789");
  });
});
