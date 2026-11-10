import { describe, expect, it } from "vitest";
import { applyBrand, cells, cyclesOf, moveBlock, newBlock, productLanding, safeHref, BLOCK_TYPES } from "./site";

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
  it("brand colour overrides the theme accent and clearing it restores the theme's", () => {
    const set: Record<string, string> = { "--primary": "#3366ff", "--ring": "#3366ff" };
    const style = { setProperty: (k: string, v: string) => { set[k] = v; }, removeProperty: (k: string) => { delete set[k]; }, getPropertyValue: (k: string) => set[k] ?? "" };
    applyBrand({ primary_color: "", font: "manrope" }, { style });
    expect(set["--primary"]).toBe("#3366ff");
    applyBrand({ primary_color: "#ff0000", font: "manrope" }, { style });
    expect(set["--primary"]).toBe("#ff0000");
    applyBrand({ primary_color: "", font: "manrope" }, { style });
    expect([set["--primary"], set["--ring"]]).toEqual(["#3366ff", "#3366ff"]);
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

describe("landing helpers", () => {
  it("orders cycles and builds a landing page", () => {
    expect(cyclesOf([{ prices: [{ currency: "EUR", cycle: "annual" }, { currency: "EUR", cycle: "monthly" }, { currency: "USD", cycle: "quarterly" }] }], "EUR")).toEqual(["monthly", "annual"]);
    expect(cells("✓, —,x")).toEqual(["✓", "—", "x"]);
    const pg = productLanding("VPS Hosting!", ["p1"]);
    expect(pg.slug).toBe("vps-hosting");
    expect(pg.blocks.map((b) => b.type)).toEqual(["hero", "features", "products", "comparison", "faq", "cta"]);
    expect(pg.blocks[2]?.props.product_ids).toEqual(["p1"]);
  });
});
