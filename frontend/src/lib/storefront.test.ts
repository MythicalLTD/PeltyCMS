import { describe, expect, it } from "vitest";
import { accentStyle, heroImageValid, move, newWidget, widgetValid } from "./storefront";

describe("storefront", () => {
  it("validates widgets like the server", () => {
    expect(widgetValid(newWidget("recent_payments"))).toBe(true);
    expect(widgetValid({ type: "recent_payments", limit: 11 })).toBe(false);
    expect(widgetValid(newWidget("text"))).toBe(false);
    expect(widgetValid({ type: "discord", invite_url: "https://discord.gg/abc-1", label: "Join" })).toBe(true);
    expect(widgetValid({ type: "discord", invite_url: "https://evil.example/discord.gg/x", label: "" })).toBe(false);
    expect(widgetValid({ type: "discord", invite_url: `https://discord.gg/${"a".repeat(190)}`, label: "" })).toBe(false);
    expect(widgetValid(newWidget("featured_package"))).toBe(false);
  });
  it("checks hero images", () => {
    expect(heroImageValid("")).toBe(true);
    expect(heroImageValid("/uploads/a.png")).toBe(true);
    expect(heroImageValid("//evil/x.png")).toBe(false);
    expect(heroImageValid("javascript:alert(1)")).toBe(false);
  });
  it("moves and colors", () => {
    expect(move([1, 2, 3], 0, 1)).toEqual([2, 1, 3]);
    expect(move([1, 2, 3], 2, 5)).toEqual([1, 2, 3]);
    expect(accentStyle("default")).toBeUndefined();
    expect(accentStyle("violet")).toMatchObject({ "--primary": "#a78bfa" });
  });
});

import { layoutFor } from "./storefront";
describe("per-category layout", () => {
  it("falls back to the store layout", () => {
    expect(layoutFor({ layout: "list", group_layouts: { g1: "grid" } }, "g1")).toBe("grid");
    expect(layoutFor({ layout: "list", group_layouts: { g1: "grid" } }, "g2")).toBe("list");
    expect(layoutFor({ layout: "grid" }, null)).toBe("grid");
    expect(layoutFor(undefined, "g1")).toBe("list");
  });
});
