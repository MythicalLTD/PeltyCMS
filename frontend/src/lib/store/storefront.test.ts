import { describe, expect, it } from "vitest";
import { addonPrice, cartIdFrom, fieldErrors, fromPrice, mayExistOnServer, orderStage, resetCartId, type Addon, type OrderField } from "./storefront";

function mem(init: Record<string, string> = {}) {
  const m = new Map(Object.entries(init));
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), m };
}
const A = "11111111-1111-4111-8111-111111111111";
const B = "22222222-2222-4222-9222-222222222222";

const orderField = (o: Partial<OrderField>): OrderField => ({ id: "f", name: "Host", description: "", kind: "text", choices: [], pattern: "", required: false, ...o });
const readOnlyStorage = (v: string | null) => ({ getItem: () => v });

describe("cart token", () => {
  it("creates and keeps a v4 id", () => {
    const s = mem();
    expect(cartIdFrom(s, "", () => A)).toBe(A);
    expect(cartIdFrom(s, "", () => B)).toBe(A);
  });
  it("a recovery link (?c=) replaces the stored cart", () => {
    const s = mem({ "pelty.cart": A });
    expect(cartIdFrom(s, `?c=${B}`, () => "x")).toBe(B);
    expect(s.m.get("pelty.cart")).toBe(B);
  });
  it("ignores malformed ids", () => {
    const s = mem({ "pelty.cart": "nope" });
    expect(cartIdFrom(s, "?c=../../x", () => A)).toBe(A);
  });
  it("reset gives a fresh cart after checkout", () => {
    const s = mem({ "pelty.cart": A });
    resetCartId(s, () => B);
    expect(cartIdFrom(s, "", () => "x")).toBe(B);
  });
});

describe("pricing helpers", () => {
  const prices = [
    { currency: "EUR", cycle: "annually", amount: "100.00", setup_fee: "0" },
    { currency: "EUR", cycle: "monthly", amount: "10.00", setup_fee: "0" },
    { currency: "USD", cycle: "monthly", amount: "5.00", setup_fee: "0" },
  ];
  it("from price is the cheapest in the currency", () => {
    expect(fromPrice(prices, "EUR")?.cycle).toBe("monthly");
    expect(fromPrice(prices, "RON")).toBeUndefined();
  });
  it("addon price matches the cycle, falling back to one-time", () => {
    const a: Addon = { id: "a", name: "Backups", description: "", prices: [
      { currency: "EUR", cycle: "monthly", amount: "2.00", setup_fee: "0" },
      { currency: "EUR", cycle: "one_time", amount: "15.00", setup_fee: "0" },
    ] };
    expect(addonPrice(a, "EUR", "monthly")?.amount).toBe("2.00");
    expect(addonPrice(a, "EUR", "annually")?.amount).toBe("15.00");
    expect(addonPrice(a, "USD", "monthly")).toBeUndefined();
  });
});

describe("custom order fields", () => {
  it("flags missing required values and bad patterns", () => {
    const fields = [orderField({ id: "a", required: true }), orderField({ id: "b", pattern: "[a-z]+\\.[a-z]+" }), orderField({ id: "c" })];
    expect(fieldErrors(fields, {})).toEqual({ a: "required" });
    expect(fieldErrors(fields, { a: " x ", b: "Bad Host" })).toEqual({ b: "pattern" });
    expect(fieldErrors(fields, { a: "x", b: "my.host" })).toEqual({});
  });
  it("leaves an invalid admin regex to the server", () => {
    expect(fieldErrors([orderField({ id: "a", pattern: "(" })], { a: "x" })).toEqual({});
  });
});

describe("order status stage", () => {
  it("walks placed → paid → accepted → active", () => {
    expect(orderStage("pending", "unpaid", [])).toBe(0);
    expect(orderStage("pending", "paid", [])).toBe(1);
    expect(orderStage("active", "paid", [{ status: "pending" }])).toBe(2);
    expect(orderStage("active", "paid", [{ status: "active" }])).toBe(3);
    expect(orderStage("cancelled", "paid", [])).toBe(-1);
  });
});

describe("mayExistOnServer", () => {
  const savedId = "11111111-1111-4111-8111-111111111111";
  const otherId = "22222222-2222-4222-8222-222222222222";
  it("skips the request for an id that was never saved", () => {
    expect(mayExistOnServer(readOnlyStorage(""), savedId, "")).toBe(false);
    expect(mayExistOnServer(readOnlyStorage(otherId), savedId, "")).toBe(false);
  });
  it("asks for saved ids, shared links and ids from before the marker", () => {
    expect(mayExistOnServer(readOnlyStorage(savedId), savedId, "")).toBe(true);
    expect(mayExistOnServer(readOnlyStorage(""), savedId, `?c=${savedId}`)).toBe(true);
    expect(mayExistOnServer(readOnlyStorage(null), savedId, "")).toBe(true);
  });
});
