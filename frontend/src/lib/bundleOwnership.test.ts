import { describe, expect, it } from "vitest";
import { DEFAULT_OWNERSHIP, ownedBundlePrice, parseOwnership, validMinPercent } from "./bundleOwnership";

describe("ownedBundlePrice", () => {
  const prices = [10, 10, 20];
  it("share mode keeps the bundle percent on the rest", () => {
    const r = ownedBundlePrice(prices, [true, false, false], 20, DEFAULT_OWNERSHIP);
    expect(r).toEqual({ full: 30, price: 24, saved: 6, owned: 1, size: 3, allOwned: false });
  });
  it("list mode charges the bundle price minus owned list prices", () => {
    expect(ownedBundlePrice(prices, [true, false, false], 20, { mode: "list", min_percent: "0" }).price).toBe(22);
  });
  it("floors at the minimum percent", () => {
    expect(ownedBundlePrice(prices, [true, false, false], 20, { mode: "list", min_percent: "90" }).price).toBe(27);
  });
  it("matches the plain bundle when nothing is owned", () => {
    expect(ownedBundlePrice(prices, [false, false, false], 20, { mode: "list", min_percent: "0" }).price).toBe(32);
  });
  it("flags a fully owned bundle", () => {
    const r = ownedBundlePrice(prices, [true, true, true], 20, DEFAULT_OWNERSHIP);
    expect(r.allOwned).toBe(true);
    expect(r.price).toBe(0);
  });
});

describe("settings", () => {
  it("parses tolerantly", () => {
    expect(parseOwnership(undefined)).toEqual(DEFAULT_OWNERSHIP);
    expect(parseOwnership({ mode: "list", min_percent: "150" })).toEqual({ mode: "list", min_percent: "100" });
    expect(parseOwnership({ mode: "x", min_percent: 5 })).toEqual({ mode: "share", min_percent: "5" });
  });
  it("validates the floor", () => {
    expect(validMinPercent("12.5")).toBe(true);
    expect(validMinPercent("101")).toBe(false);
    expect(validMinPercent("-1")).toBe(false);
  });
});
