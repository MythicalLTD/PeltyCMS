import { describe, expect, it } from "vitest";
import { optionPayload } from "./ConfigOptions";

describe("config option payload", () => {
  it("keeps one choice for quantity, omits empty prices", () => {
    const p = optionPayload({ name: " RAM ", option_type: "quantity", required: true, qty_min: 1, qty_max: 16, choices: [{ label: "GB", prices: { "EUR|monthly": "1.25", "EUR|annual": " " } }, { label: "Extra", prices: {} }] });
    expect(p).toEqual({ name: "RAM", option_type: "quantity", required: true, qty_min: 1, qty_max: 16, choices: [{ label: "GB", prices: [{ currency: "EUR", cycle: "monthly", amount: "1.25" }] }] });
  });
  it("forces yes/no optional", () => {
    expect(optionPayload({ name: "B", option_type: "yesno", required: true, qty_min: 0, qty_max: 100, choices: [{ label: "Yes", prices: {} }] }).required).toBe(false);
  });
});
