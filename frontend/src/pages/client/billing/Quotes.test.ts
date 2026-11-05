import { describe, expect, it } from "vitest";
import { quoteExpired } from "./Quotes";

describe("quoteExpired", () => {
  it("only marks sent quotes past valid_until", () => {
    const past = "2020-01-01T00:00:00Z";
    const future = "2099-01-01T00:00:00Z";
    expect(quoteExpired({ status: "sent", valid_until: past })).toBe(true);
    expect(quoteExpired({ status: "sent", valid_until: future })).toBe(false);
    expect(quoteExpired({ status: "accepted", valid_until: past })).toBe(false);
    expect(quoteExpired({ status: "draft", valid_until: past })).toBe(false);
  });
});
