import { describe, expect, it } from "vitest";
import { normalizeCreatorCode, validCreatorDiscount } from "./creatorCodes";

describe("creator codes", () => {
  it("normalizes like the server", () => {
    expect(normalizeCreatorCode("  CoolGuy_99 ")).toBe("coolguy_99");
    expect(normalizeCreatorCode("ab")).toBeNull();
    expect(normalizeCreatorCode("has space")).toBeNull();
    expect(normalizeCreatorCode("x".repeat(25))).toBeNull();
    expect(normalizeCreatorCode("a-b")).toBe("a-b");
  });
  it("bounds the discount", () => {
    expect(validCreatorDiscount("0")).toBe(true);
    expect(validCreatorDiscount("12.5")).toBe(true);
    expect(validCreatorDiscount("50")).toBe(true);
    expect(validCreatorDiscount("50.01")).toBe(false);
    expect(validCreatorDiscount("-1")).toBe(false);
    expect(validCreatorDiscount("1.234")).toBe(false);
  });
});
