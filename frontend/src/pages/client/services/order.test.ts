import { describe, expect, it } from "vitest";
import { defaultConfig, estimate } from "./Order";
import { toPayload, couponValue } from "../../admin/billing/Coupons";

const p = {
  id: "p1", name: "VPS", description: "", group_id: "g",
  prices: [{ currency: "EUR", cycle: "monthly", amount: "10.00", setup_fee: "5.00" }],
  options: [{ id: "o1", name: "Loc", required: true, choices: [{ id: "c1", label: "EU", prices: [] }, { id: "c2", label: "US", prices: [{ currency: "EUR", cycle: "monthly", amount: "2.50" }] }] }],
};

describe("order estimate", () => {
  it("adds options, setup and a percent coupon", () => {
    const c = { code: "X", kind: "percent" as const, value: "20", currency: null, recurring: true, product_ids: [] };
    expect(estimate(p, "monthly", "EUR", { o1: "c2" }, c)).toEqual({ recurring: 12.5, setup: 5, discount: 2.5, total: 15 });
  });
  it("ignores coupons restricted to other products and caps fixed discounts", () => {
    const other = { code: "X", kind: "fixed" as const, value: "3", currency: "EUR", recurring: false, product_ids: ["zzz"] };
    expect(estimate(p, "monthly", "EUR", {}, other)?.discount).toBe(0);
    expect(estimate(p, "monthly", "EUR", {}, { ...other, value: "99", product_ids: [] })?.discount).toBe(10);
    expect(estimate(p, "annual", "EUR", {}, null)).toBeNull();
  });
});

describe("configurable option types", () => {
  const q = { ...p, options: [
    { id: "ram", name: "RAM", required: true, option_type: "quantity" as const, qty_min: 2, qty_max: 8, choices: [{ id: "gb", label: "GB", prices: [{ currency: "EUR", cycle: "monthly", amount: "1.25" }] }] },
    { id: "bk", name: "Backups", required: false, option_type: "yesno" as const, choices: [{ id: "y", label: "Yes", prices: [{ currency: "EUR", cycle: "monthly", amount: "3.00" }] }] },
  ] };
  it("multiplies quantity and adds yes/no only when on", () => {
    expect(estimate(q, "monthly", "EUR", { ram: 4 }, null)?.recurring).toBe(15);
    expect(estimate(q, "monthly", "EUR", { ram: 4, bk: "y" }, null)?.recurring).toBe(18);
  });
  it("defaults quantity to min and yes/no to off", () => {
    expect(defaultConfig(q.options)).toEqual({ ram: 2 });
    expect(defaultConfig(p.options)).toEqual({ o1: "c1" });
  });
});

describe("coupon helpers", () => {
  it("drops currency for percent coupons and formats values", () => {
    const d = { code: "A", description: "", kind: "percent" as const, value: "10", currency: "EUR", recurring: false, max_uses: null, max_uses_per_customer: null, starts_at: null, expires_at: null, active: true, product_ids: [], recurring_cycles: null, applies_addons: false, applies_domains: false, tlds: [], customer_ids: [], client_group_ids: [] };
    expect(toPayload(d).currency).toBeNull();
    expect(couponValue({ kind: "percent", value: "15.0000", currency: null }, "en")).toBe("15%");
  });
});
