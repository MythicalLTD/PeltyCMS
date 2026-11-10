import { describe, expect, it } from "vitest";
import { creatorNet, creatorSlug, marketConfig, validAmount, validPercent } from "./marketplace";

describe("marketplace helpers", () => {
  it("computes creator net", () => {
    expect(creatorNet("50", "20")).toBe("40.00");
    expect(creatorNet("9.99", "10")).toBe("8.99");
    expect(creatorNet("x", "10")).toBe("0.00");
  });
  it("reads settings with defaults", () => {
    expect(marketConfig(undefined).mode).toBe("multi_vendor");
    expect(marketConfig({ mode: "admin_only", hold_days: 3 })).toMatchObject({ mode: "admin_only", hold_days: 3, commission_percent: "10" });
  });
  it("slugs names", () => {
    expect(creatorSlug("  Carol's Dev Shop! ")).toBe("carol-s-dev-shop");
    expect(creatorSlug("Renée Co")).toBe("renee-co");
    expect(creatorSlug(`${"a".repeat(39)} b`)).toBe("a".repeat(39));
  });
  it("validates settings numbers", () => {
    expect(validPercent("12.5")).toBe(true);
    expect(validPercent("100")).toBe(true);
    expect(validPercent("101")).toBe(false);
    expect(validPercent("-1")).toBe(false);
    expect(validAmount("abc")).toBe(false);
    expect(validAmount("0")).toBe(true);
  });
});
