import { describe, expect, it } from "vitest";
import { bundlePrice, bundleValid, couponBody, couponDraft, couponPreview, couponValid, toggle, unreadFor, type Conversation } from "./creatorTools";

describe("creator discount codes", () => {
  it("prefixes once and upper-cases", () => {
    expect(couponPreview("NOVA-", " spring ")).toBe("NOVA-SPRING");
    expect(couponPreview("NOVA-", "nova-spring")).toBe("NOVA-SPRING");
  });
  it("validates drafts", () => {
    const d = { ...couponDraft(), code: "spring", value: "10", product_ids: ["a"] };
    expect(couponValid(d, "NOVA-")).toBe(true);
    expect(couponValid({ ...d, product_ids: [] }, "NOVA-")).toBe(false);
    expect(couponValid({ ...d, value: "120" }, "NOVA-")).toBe(false);
    expect(couponValid({ ...d, kind: "fixed", value: "120", currency: "EUR" }, "NOVA-")).toBe(true);
    expect(couponValid({ ...d, code: "bad code!" }, "NOVA-")).toBe(false);
    expect(couponValid({ ...d, max_uses: "0" }, "NOVA-")).toBe(false);
  });
  it("builds the API body and round-trips a saved code", () => {
    const b = couponBody({ ...couponDraft(), code: "x", value: "5", max_uses: "3", expires_at: "2026-12-31", product_ids: ["a"] });
    expect(b).toMatchObject({ code: "x", kind: "percent", currency: null, max_uses: 3, max_uses_per_customer: 1, expires_at: "2026-12-31T23:59:59.000Z" });
    const d = couponDraft({ id: "1", code: "NOVA-X", description: "", kind: "fixed", value: "5.0000", currency: "USD", max_uses: null, max_uses_per_customer: null, uses: 0, starts_at: null, expires_at: null, active: true, product_ids: ["a"] }, "NOVA-");
    expect(d).toMatchObject({ code: "X", value: "5", currency: "USD", max_uses: "" });
  });
});

describe("bundles", () => {
  it("validates", () => {
    const d = { name: "Duo", description: "", discount_percent: "20", active: true, product_ids: ["a", "b"] };
    expect(bundleValid(d)).toBe(true);
    expect(bundleValid({ ...d, product_ids: ["a"] })).toBe(false);
    expect(bundleValid({ ...d, discount_percent: "101" })).toBe(false);
    expect(bundleValid({ ...d, discount_percent: "" })).toBe(false);
  });
  it("prices", () => {
    expect(bundlePrice([50, 30], 20)).toEqual({ full: 80, price: 64, saved: 16 });
    expect(bundlePrice([9.99, 4.99], 15).price).toBe(12.73);
  });
});

describe("helpers", () => {
  it("toggles and counts unread per side", () => {
    expect(toggle(["a"], "b")).toEqual(["a", "b"]);
    expect(toggle(["a", "b"], "a")).toEqual(["b"]);
    const c = { creator_id: "cr", creator_unread: 2, buyer_unread: 1 } as Conversation;
    expect(unreadFor(c, "cr")).toBe(2);
    expect(unreadFor(c, null)).toBe(1);
  });
});
