import { describe, expect, it } from "vitest";
import { applyBrand, moveBlock, newBlock, safeHref, BLOCK_TYPES } from "./site";

describe("site", () => {
  it("moves blocks within bounds", () => {
    expect(moveBlock([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
    expect(moveBlock([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
    expect(moveBlock([1, 2, 3], 2, 1)).toEqual([1, 3, 2]);
  });
  it("creates every block type", () => {
    for (const t of BLOCK_TYPES) expect(newBlock(t).type).toBe(t);
  });
  it("only allows safe links", () => {
    expect(safeHref("/store")).toBe("/store");
    expect(safeHref("https://x.io")).toBe("https://x.io");
    expect(safeHref("javascript:alert(1)")).toBeNull();
    expect(safeHref("//evil.io")).toBeNull();
    expect(safeHref(5)).toBeNull();
  });
  it("applies brand variables", () => {
    const set: Record<string, string> = {};
    const root = { style: { setProperty: (k: string, v: string) => { set[k] = v; }, removeProperty: (k: string) => { delete set[k]; } } };
    applyBrand({ primary_color: "#59b5d3", font: "inter" }, root);
    expect(set["--primary"]).toBe("#59b5d3");
    expect(set["--font-sans"]).toContain("Inter");
    applyBrand({ primary_color: "bad", font: "manrope" }, root);
    expect(set["--primary"]).toBeUndefined();
  });
  it("applies the default colour mode", () => {
    const cls: Record<string, boolean> = {};
    const root = { style: { setProperty() {}, removeProperty() {} }, classList: { toggle: (c: string, on: boolean) => { cls[c] = on; } } };
    applyBrand({ primary_color: "#59b5d3", font: "inter", default_mode: "light" }, root);
    expect(cls.dark).toBe(false);
    applyBrand({ primary_color: "#59b5d3", font: "inter", default_mode: "system" }, root, true);
    expect(cls.dark).toBe(true);
  });
});
