import { describe, expect, it } from "vitest";
import { mergeValid, splitValid } from "./InvoiceOps";
import { giftPayload, normalizeCode } from "@/pages/admin/billing/GiftCards";
import { currencyPayload, markupValid } from "@/pages/admin/billing/Currencies";

const c = (id: string, currency: string) => ({ id, number: id, currency, total: "1", due_at: null });

describe("invoice ops", () => {
  it("split keeps a line on each side", () => {
    expect(splitValid([], 3)).toBe(false);
    expect(splitValid(["a"], 3)).toBe(true);
    expect(splitValid(["a", "b", "c"], 3)).toBe(false);
  });
  it("merge needs two invoices in one currency", () => {
    const rows = [c("a", "EUR"), c("b", "EUR"), c("x", "USD")];
    expect(mergeValid(["a"], rows)).toBe(false);
    expect(mergeValid(["a", "b"], rows)).toBe(true);
    expect(mergeValid(["a", "x"], rows)).toBe(false);
  });
});

describe("gift cards", () => {
  it("validates the create form", () => {
    expect(giftPayload({ count: "2", currency: "eur", amount: "25.50", note: " x ", expires: "" })).toEqual({ count: 2, currency: "EUR", amount: "25.50", note: "x", expires_at: null });
    expect(giftPayload({ count: "0", currency: "EUR", amount: "5", note: "", expires: "" })).toBeNull();
    expect(giftPayload({ count: "1", currency: "EUR", amount: "-5", note: "", expires: "" })).toBeNull();
  });
  it("normalises typed codes", () => {
    expect(normalizeCode("abcd efgh-jkmn pqrs")).toBe("ABCD-EFGH-JKMN-PQRS");
  });
});

describe("currency formats and rates", () => {
  const base = { code: "RON", exponent: "2", enabled: true, rate: "5", prefix: "", suffix: " lei" };
  it("sends separators and rejects equal ones", () => {
    expect(currencyPayload({ ...base, decimal_sep: ",", thousands_sep: "." }, true)).toMatchObject({ decimal_sep: ",", thousands_sep: ".", auto_update: true });
    expect(currencyPayload({ ...base, decimal_sep: ",", thousands_sep: "," }, true)).toBeNull();
    expect(currencyPayload({ ...base, decimal_sep: ".", thousands_sep: "" }, true)).toMatchObject({ thousands_sep: "" });
  });
  it("bounds the markup", () => {
    expect(markupValid("2.5")).toBe(true);
    expect(markupValid("-50")).toBe(true);
    expect(markupValid("101")).toBe(false);
    expect(markupValid("abc")).toBe(false);
  });
});

import { txQuery } from "@/pages/admin/billing/Transactions";
describe("transactions filters", () => {
  it("drops empty filters", () => {
    expect(txQuery({ q: " INV-1 ", status: "", gateway: "", currency: "", from: "2026-01-01", to: "" }, 2)).toBe("/admin/transactions?q=INV-1&from=2026-01-01&page=2");
  });
});
