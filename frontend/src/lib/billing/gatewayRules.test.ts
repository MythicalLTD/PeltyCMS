import { describe, expect, it } from "vitest";
import { fromRules, toRules } from "./gatewayRules";

describe("gateway rules form", () => {
  it("round-trips and normalizes", () => {
    const body = toRules({ fee_percent: " 2.9 ", currencies: "eur, ron,,", limits: [{ currency: "eur", min: "1.00", max: "", fee_fixed: "" }, { currency: " ", min: "5", max: "", fee_fixed: "" }] });
    expect(body).toEqual({ fee_percent: "2.9", currencies: ["EUR", "RON"], limits: { EUR: { min: "1.00", max: null, fee_fixed: "0" } } });
    expect(fromRules(body)).toEqual({ fee_percent: "2.9", currencies: "EUR, RON", limits: [{ currency: "EUR", min: "1.00", max: "", fee_fixed: "0" }] });
  });
});
