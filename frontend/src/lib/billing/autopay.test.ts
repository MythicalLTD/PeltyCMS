import { describe, expect, it } from "vitest";
import { expiry, isExpired, tokenError } from "./autopay";

describe("autopay", () => {
  it("detects expiry by month", () => {
    const now = new Date(Date.UTC(2026, 9, 29));
    expect(isExpired({ exp_month: 10, exp_year: 2026 }, now)).toBe(false);
    expect(isExpired({ exp_month: 9, exp_year: 2026 }, now)).toBe(true);
    expect(isExpired({ exp_month: 1, exp_year: 2025 }, now)).toBe(true);
    expect(isExpired({ exp_month: 1, exp_year: 2027 }, now)).toBe(false);
    expect(expiry({ exp_month: 3, exp_year: 2031 })).toBe("03/2031");
  });
  it("validates tokens", () => {
    expect(tokenError("stripe", "pm_card_visa")).toBeNull();
    expect(tokenError("stripe", "tok_1")).toBe("error.invalid_token");
    expect(tokenError("stripe", "pm_../x")).toBe("error.invalid_token");
    expect(tokenError("other", " ")).toBe("error.invalid_token");
    expect(tokenError("other", "abc")).toBeNull();
  });
});
