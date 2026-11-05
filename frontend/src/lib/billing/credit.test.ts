import { describe, expect, it } from "vitest";
import { applicable, cmpDec, creditIn, isAdjustment, pdfUrl } from "./credit";

describe("credit helpers", () => {
  it("compares decimals exactly", () => {
    expect(cmpDec("10.00", "10")).toBe(0);
    expect(cmpDec("0.1", "0.10000000000000000001")).toBe(-1);
    expect(cmpDec("100", "99.99")).toBe(1);
    expect(cmpDec("-1", "0")).toBe(-1);
    expect(cmpDec("-2", "-1.5")).toBe(-1);
    expect(cmpDec("-0.00", "0")).toBe(0);
    expect(() => cmpDec("abc", "1")).toThrow();
  });
  it("finds balance per currency", () => {
    const s = { balances: [{ currency: "EUR", amount: "12.50" }], entries: [] };
    expect(creditIn(s, "EUR")).toBe("12.50");
    expect(creditIn(s, "USD")).toBe("0");
    expect(creditIn(undefined, "EUR")).toBe("0");
  });
  it("applies min(credit, balance)", () => {
    expect(applicable("12.50", "100.00")).toBe("12.50");
    expect(applicable("50", "30.00")).toBe("30.00");
    expect(applicable("0", "30")).toBe("0");
    expect(applicable("5", "0.00")).toBe("0");
  });
  it("validates adjustments", () => {
    expect(isAdjustment("5")).toBe(true);
    expect(isAdjustment("-2.25")).toBe(true);
    expect(isAdjustment("0.00")).toBe(false);
    expect(isAdjustment("1.001")).toBe(false);
    expect(isAdjustment("1.001", 3)).toBe(true);
    expect(isAdjustment("1e3")).toBe(false);
  });
  it("builds pdf urls", () => {
    expect(pdfUrl("client", "abc")).toBe("/api/v1/client/invoices/abc/pdf");
  });
});
