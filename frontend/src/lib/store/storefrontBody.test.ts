import { describe, expect, it } from "vitest";
import { cartBody } from "./storefront";

describe("cartBody", () => {
  it("drops server-only fields so applying a coupon is not rejected", () => {
    const fromApi = { id: "x", order_id: null, customer_id: "c", updated_at: "2026-01-01T00:00:00Z", email: "", currency: "EUR", country: "RO", items: [], coupon: " WELCOME10 " };
    const body = cartBody({ ...fromApi, locale: "en" });
    expect(Object.keys(body).sort()).toEqual(["country", "coupon", "currency", "email", "items", "locale"]);
    expect(body.coupon).toBe("WELCOME10");
    expect(body.email).toBeNull();
  });
});
