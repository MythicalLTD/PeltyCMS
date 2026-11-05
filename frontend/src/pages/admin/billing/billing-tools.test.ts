import { describe, expect, it } from "vitest";
import { cleanRules, parseDays, toggle } from "./BillingTools";

describe("billing tools helpers", () => {
  it("parses reminder days", () => {
    expect(parseDays("7, 3,3 ; 0 x 61 1")).toEqual([1, 3, 7]);
    expect(parseDays("")).toEqual([]);
  });
  it("toggles selection", () => {
    expect(toggle(["a"], "b", true)).toEqual(["a", "b"]);
    expect(toggle(["a", "b"], "a", false)).toEqual(["b"]);
    expect(toggle(["a"], "a", true)).toEqual(["a"]);
  });
  it("cleans rules", () => {
    const r = cleanRules({ auto_approve: true, require_paid: true, max_total: "  ", existing_clients_only: false, manual_product_ids: [" x ", ""], hold_fraud_review: true });
    expect(r.max_total).toBeNull();
    expect(r.manual_product_ids).toEqual(["x"]);
  });
});
