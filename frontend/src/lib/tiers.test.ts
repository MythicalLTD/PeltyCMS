import { describe, expect, it } from "vitest";
import { moveTier, tierBody, tierDraft, tierValid, upgradePrice, type TierGroup } from "./tiers";

describe("tier groups", () => {
  it("drafts from a group in rank order", () => {
    const g: TierGroup = { id: "g", name: "Ranks", credit_mode: "difference", min_price: "1.5000", revoke_old: true, cancel_old_recurring: false,
      tiers: [{ product_id: "b", name: "VIP+", rank: 2 }, { product_id: "a", name: "VIP", rank: 1 }] };
    expect(tierDraft(g)).toEqual({ name: "Ranks", credit_mode: "difference", min_price: "1.5", revoke_old: true, cancel_old_recurring: false, product_ids: ["a", "b"] });
    expect(tierDraft().product_ids).toEqual([]);
  });

  it("validates like the server", () => {
    const d = { ...tierDraft(), name: "Ranks", product_ids: ["a", "b"] };
    expect(tierValid(d)).toBe(true);
    expect(tierValid({ ...d, product_ids: ["a"] })).toBe(false);
    expect(tierValid({ ...d, product_ids: ["a", "a"] })).toBe(false);
    expect(tierValid({ ...d, name: "  " })).toBe(false);
    expect(tierValid({ ...d, min_price: "-1" })).toBe(false);
    expect(tierValid({ ...d, min_price: "2.555" })).toBe(false);
    expect(tierValid({ ...d, min_price: "" })).toBe(true);
    expect(tierBody({ ...d, name: " Ranks ", min_price: "" })).toMatchObject({ name: "Ranks", min_price: "0" });
  });

  it("reorders tiers", () => {
    expect(moveTier(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    expect(moveTier(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(moveTier(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
  });

  it("shows the upgrade price", () => {
    const c = { group_id: "g", amount: "9.99", source: "s", from_name: "VIP" };
    expect(upgradePrice("19.99", c)).toBe(10);
    expect(upgradePrice("5", c)).toBe(0);
    expect(upgradePrice("5", undefined)).toBe(5);
  });
});
