// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { I18nProvider } from "@/lib/i18n";
import { Totals } from "./Store";
import type { CartView } from "@/lib/store/storefront";

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ languages: [{ code: "en", name: "English" }] }), { status: 200 })));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const view: CartView = {
  cart: { id: "c", email: null, currency: "EUR", country: "RO", items: [], coupon: "SAVE", order_id: null },
  error: null,
  preview: {
    currency: "EUR", lines: [], subtotal: "17.50", discount: "2.50", group_discount: "0", tax: "3.00",
    taxes: [{ name: "VAT", rate: "20.0000", compound: false, amount: "3.00" }], total: "18.00", inclusive: false, coupon: "SAVE", coupon_error: null,
  },
};

describe("cart totals", () => {
  it("shows subtotal, coupon discount, each tax and the total", () => {
    render(<I18nProvider><Totals view={view} /></I18nProvider>);
    const text = document.body.textContent ?? "";
    expect(text).toContain("VAT (20%)");
    expect(text).toMatch(/18[.,]00/);
    expect(text).toMatch(/2[.,]50/);
    expect(screen.queryByText(/group/i)).toBeNull();
  });
  it("renders nothing without a preview", () => {
    const { container } = render(<I18nProvider><Totals view={{ ...view, preview: null }} /></I18nProvider>);
    expect(container.textContent).toBe("");
  });
});

import { stockState } from "@/pages/client/services/Order";
describe("stockState", () => {
  it("maps stock to badges", () => {
    expect(stockState({})).toBeNull();
    expect(stockState({ stock_enabled: true, stock_qty: 0 })).toBe("out");
    expect(stockState({ stock_enabled: true, stock_qty: 0, allow_backorder: true })).toBe("backorder");
    expect(stockState({ stock_enabled: true, stock_qty: 3 })).toBe("low");
    expect(stockState({ stock_enabled: true, stock_qty: 50 })).toBe("in");
  });
});
