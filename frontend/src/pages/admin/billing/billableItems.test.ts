import { describe, expect, it } from "vitest";
import { billablePayload, linesNet, linesPayload, type BillableDraft } from "./BillableItems";

const base: BillableDraft = { customer_id: "c1", description: " Setup ", quantity: "1", unit_price: "12.50", invoice_action: "hold", recur_cycle: "monthly", next_due_date: "", recur_times: "" };

describe("billablePayload", () => {
  it("trims and drops recurring fields for one-off items", () => {
    expect(billablePayload(base)).toEqual({ customer_id: "c1", description: "Setup", quantity: "1", unit_price: "12.50", invoice_action: "hold" });
  });
  it("requires a start date for recurring and passes times", () => {
    expect(billablePayload({ ...base, invoice_action: "recurring" })).toBeNull();
    expect(billablePayload({ ...base, invoice_action: "recurring", next_due_date: "2030-01-01", recur_times: "3" })).toMatchObject({ recur_cycle: "monthly", next_due_date: "2030-01-01", recur_times: 3 });
  });
  it("rejects bad amounts", () => {
    expect(billablePayload({ ...base, unit_price: "-1" })).toBeNull();
    expect(billablePayload({ ...base, quantity: "0" })).toBeNull();
    expect(billablePayload({ ...base, customer_id: "" })).toBeNull();
  });
});

describe("linesPayload", () => {
  it("validates every line and keeps optional tax", () => {
    expect(linesPayload([{ description: "A", quantity: "2", unit_price: "1.5" }])).toEqual([{ description: "A", quantity: "2", unit_price: "1.5" }]);
    expect(linesPayload([{ description: "A", quantity: "2", unit_price: "1.5", tax_rate: "19" }])).toEqual([{ description: "A", quantity: "2", unit_price: "1.5", tax_rate: "19" }]);
    expect(linesPayload([{ description: "A", quantity: "2", unit_price: "1.5" }, { description: "", quantity: "1", unit_price: "1" }])).toBeNull();
    expect(linesPayload([{ description: "A", quantity: "1", unit_price: "1", tax_rate: "101" }])).toBeNull();
    expect(linesPayload([])).toBeNull();
  });
});

describe("linesNet", () => {
  it("sums quantity × unit price", () => {
    expect(linesNet([{ description: "A", quantity: "2", unit_price: "10" }, { description: "B", quantity: "1", unit_price: "3.5" }])).toBe(23.5);
    expect(linesNet([{ description: "A", quantity: "x", unit_price: "10" }])).toBe(0);
  });
});
