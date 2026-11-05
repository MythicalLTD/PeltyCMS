import { describe, expect, it } from "vitest";
import { margin, salePrice, toUnits } from "./vendor";

describe("vendor pricing", () => {
  it("parses decimals exactly", () => {
    expect(toUnits("4.5")).toBe(45000n);
    expect(toUnits("0.12345")).toBeNull();
    expect(toUnits("1e3")).toBeNull();
  });
  it("matches server rounding", () => {
    expect(salePrice("4.50", "40")).toBe("6.30");
    expect(salePrice("0.125", "0")).toBe("0.13");
    expect(salePrice("3.333", "0")).toBe("3.33");
    expect(salePrice("1500", "12.5", 0)).toBe("1688");
    expect(salePrice("10", "-100")).toBe("0.00");
    expect(salePrice("10", "abc")).toBeNull();
  });
  it("computes margin", () => {
    expect(margin("4.5000", "9.0000")).toEqual({ amount: "4.50", percent: "50.0" });
    expect(margin("4.50", null)).toBeNull();
  });
});
