import { describe, expect, it } from "vitest";
import { creatorNet, creatorSlug, galleryValid, inTransit, marketConfig, moveItem, moveTo, paypalEmailValid, stripeReady, validAmount, validPercent } from "./marketplace";
import { resourceImages } from "./market";

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

describe("payout helpers", () => {
  it("validates PayPal emails", () => {
    expect(paypalEmailValid("dev@example.com")).toBe(true);
    expect(paypalEmailValid(" dev@example.com ")).toBe(true);
    for (const bad of ["", "dev", "@x.com", "a@b", "a b@c.com", "a@.com", "a@b.", "a@b@c.com"]) expect(paypalEmailValid(bad)).toBe(false);
  });
  it("only an enabled Stripe account is ready", () => {
    expect(stripeReady("enabled")).toBe(true);
    expect(stripeReady("restricted")).toBe(false);
    expect(stripeReady(undefined)).toBe(false);
  });
  it("reads in-transit totals per currency", () => {
    const info = { in_transit: [{ currency: "EUR", amount: "12.50" }] } as Parameters<typeof inTransit>[0];
    expect(inTransit(info, "EUR")).toBe("12.50");
    expect(inTransit(info, "USD")).toBe("0");
    expect(inTransit(undefined, "EUR")).toBe("0");
  });
  it("parses payout schedule settings with safe defaults", () => {
    const d = marketConfig({});
    expect([d.payout_schedule, d.payout_weekday, d.payout_monthday, d.payout_approval, d.reverse_transfers_on_refund]).toEqual(["manual", 1, 1, true, true]);
    expect(d.payout_methods).toEqual(["manual", "stripe_connect", "paypal"]);
    const c = marketConfig({ payout_schedule: "weekly", payout_weekday: 9, payout_monthday: 31, payout_approval: false, payout_methods: ["paypal", "bogus"] });
    expect([c.payout_schedule, c.payout_weekday, c.payout_monthday, c.payout_approval]).toEqual(["weekly", 7, 28, false]);
    expect(c.payout_methods).toEqual(["paypal"]);
    expect(marketConfig({ payout_schedule: "daily" }).payout_schedule).toBe("manual");
  });
});

describe("gallery helpers", () => {
  it("validates gallery URLs and size", () => {
    expect(galleryValid([{ url: "https://x.test/a.png", caption: "" }, { url: "/img/b.png", caption: "b" }, { url: "", caption: "" }])).toBe(true);
    expect(galleryValid([{ url: "javascript:alert(1)", caption: "" }])).toBe(false);
    expect(galleryValid([{ url: "//evil.test/a.png", caption: "" }])).toBe(false);
    expect(galleryValid(Array.from({ length: 13 }, (_, i) => ({ url: `/i${i}.png`, caption: "" })))).toBe(false);
  });
  it("moves items within bounds", () => {
    expect(moveItem(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(moveItem(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    const xs = ["a"];
    expect(moveItem(xs, 0, -1)).toBe(xs);
  });
  it("drag-reorders to an index", () => {
    expect(moveTo(["a", "b", "c", "d"], 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(moveTo(["a", "b", "c", "d"], 3, 0)).toEqual(["d", "a", "b", "c"]);
    const xs = ["a", "b"];
    expect(moveTo(xs, 1, 1)).toBe(xs);
    expect(moveTo(xs, 0, 5)).toBe(xs);
  });
  it("puts the cover first and drops duplicates", () => {
    const listing = { image_url: "/cover.png" } as Parameters<typeof resourceImages>[0]["listing"];
    expect(resourceImages({ listing, gallery: [{ url: "/cover.png", caption: "" }, { url: "/b.png", caption: "B" }] }).map((g) => g.url)).toEqual(["/cover.png", "/b.png"]);
    expect(resourceImages({ listing: { ...listing, image_url: "" }, gallery: undefined })).toEqual([]);
  });
});
