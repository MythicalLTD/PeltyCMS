import { describe, expect, it } from "vitest";
import { bundleDiscount, crossSells, matchBundle, type Bundle } from "./cart";

const b: Bundle = { id: "b", name: "B", description: "", active: true, discount_percent: "10.00", items: [{ product_id: "a", product_name: "A", cycle: "monthly" }, { product_id: "c", product_name: "C", cycle: "annual" }] };
const line = (p: string, c: string) => ({ product_id: p, cycle: c, config: {} });

describe("cart", () => {
  it("matches bundles", () => {
    expect(matchBundle(b, [line("c", "annual"), line("x", "monthly"), line("a", "monthly")])).toEqual([2, 0]);
    expect(matchBundle(b, [line("a", "monthly"), line("c", "monthly")])).toBeNull();
  });
  it("estimates bundle discount", () => {
    expect(bundleDiscount(b, [line("a", "monthly"), line("c", "annual")], [10, 100])).toBe(11);
    expect(bundleDiscount(b, [line("a", "monthly")], [10])).toBe(0);
  });
  it("filters cross-sells", () => {
    expect(crossSells({ upsell: [], cross_sell: ["a", "b", "c", "d", "e"] }, [line("a", "monthly")], "b")).toEqual(["c", "d", "e"]);
  });
});
