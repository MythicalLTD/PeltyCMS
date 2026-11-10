import { describe, expect, it } from "vitest";
import { joinUrl, parseJoinCode, quickAddCycle } from "./game";

const price = (currency: string, cycle: string) => ({ currency, cycle, amount: "5.00", setup_fee: "0" });

describe("join code", () => {
  it("builds the cfx.re link only for a valid code", () => {
    expect(joinUrl("abc123")).toBe("https://cfx.re/join/abc123");
    expect(joinUrl("")).toBeNull();
    expect(joinUrl("abc/../x")).toBeNull();
    expect(joinUrl(undefined)).toBeNull();
  });
  it("extracts the code from a pasted link", () => {
    expect(parseJoinCode("https://cfx.re/join/AbC123")).toBe("abc123");
    expect(parseJoinCode(" abc123 ")).toBe("abc123");
  });
});

describe("quickAddCycle", () => {
  it("allows one-click add for a single price without options", () => {
    expect(quickAddCycle({ prices: [price("EUR", "one_time")], options: [] }, "EUR")).toBe("one_time");
  });
  it("sends the buyer to the product page when there is a choice to make", () => {
    expect(quickAddCycle({ prices: [price("EUR", "monthly"), price("EUR", "annual")], options: [] }, "EUR")).toBeNull();
    expect(quickAddCycle({ prices: [price("EUR", "monthly")], options: [{}] }, "EUR")).toBeNull();
  });
  it("ignores prices in other currencies", () => {
    expect(quickAddCycle({ prices: [price("EUR", "monthly"), price("USD", "annual")], options: [] }, "EUR")).toBe("monthly");
  });
});
