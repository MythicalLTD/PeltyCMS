import { describe, expect, it } from "vitest";
import { checkDebitLines, debitNet, docTitleKey, ublUrl } from "./einvoice";

describe("einvoice helpers", () => {
  it("builds UBL urls", () => {
    expect(ublUrl("client", "a")).toBe("/api/v1/client/invoices/a/ubl");
    expect(ublUrl("admin", "b")).toBe("/api/v1/admin/invoices/b/ubl");
  });
  it("validates debit lines", () => {
    expect(checkDebitLines([])).toBe("debit.invalid_lines");
    expect(checkDebitLines([{ description: " ", quantity: "1", unit_price: "1" }])).toBe("debit.invalid_lines");
    expect(checkDebitLines([{ description: "x", quantity: "0", unit_price: "1" }])).toBe("debit.invalid_amount");
    expect(checkDebitLines([{ description: "x", quantity: "1", unit_price: "-2" }])).toBe("debit.invalid_amount");
    expect(checkDebitLines([{ description: "x", quantity: "2", unit_price: "1.25" }])).toBeNull();
  });
  it("estimates net without float drift", () => {
    expect(debitNet([{ description: "a", quantity: "3", unit_price: "0.1" }, { description: "b", quantity: "1", unit_price: "0.2" }])).toBe("0.50");
  });
  it("maps kinds to titles", () => {
    expect(docTitleKey("debit_note")).toBe("pdf.debit_note");
    expect(docTitleKey("credit_note")).toBe("pdf.credit_note");
    expect(docTitleKey("invoice")).toBe("billing.invoice");
  });
});
