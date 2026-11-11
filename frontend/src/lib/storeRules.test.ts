import { describe, expect, it } from "vitest";
import { banValueValid, detectCurrency, giftNameValid, payAmountValid, ruleBody, ruleDraft, ruleValid } from "./storeRules";

describe("package rules", () => {
  it("round-trips drafts and validates", () => {
    const d = ruleDraft("p1", { product_id: "p1", user_limit: 1, user_period: "month", global_limit: null, global_period: "all", required_product_ids: ["p2"], pay_what_you_want: true, giftable: false });
    expect(d.user_limit).toBe("1");
    expect(d.global_limit).toBe("");
    expect(ruleValid(d)).toBe(true);
    expect(ruleBody(d)).toMatchObject({ user_limit: 1, global_limit: null, pay_what_you_want: true, giftable: false });
    expect(ruleValid({ ...d, user_limit: "0" })).toBe(false);
    expect(ruleValid({ ...d, global_limit: "1.5" })).toBe(false);
    expect(ruleValid({ ...d, required_product_ids: ["p1"] })).toBe(false);
    expect(ruleDraft("x")).toMatchObject({ giftable: true, pay_what_you_want: false, user_period: "all" });
  });
  it("checks pay-what-you-want amounts like the server", () => {
    expect(payAmountValid("12.50", 10)).toBe(true);
    expect(payAmountValid("9.99", 10)).toBe(false);
    expect(payAmountValid("12.555", 10)).toBe(false);
    expect(payAmountValid("1001", 10)).toBe(false);
    expect(payAmountValid("1", 0)).toBe(true);
    expect(payAmountValid("abc", 0)).toBe(false);
  });
  it("checks gift names and ban values", () => {
    expect(giftNameValid(" Steve ")).toBe(true);
    expect(giftNameValid("Cool Gamer")).toBe(true);
    expect(giftNameValid('x" op')).toBe(false);
    expect(giftNameValid("")).toBe(false);
    expect(banValueValid("email", "a@b.c")).toBe(true);
    expect(banValueValid("email", "nope")).toBe(false);
    expect(banValueValid("username", "Griefer_1")).toBe(true);
  });
});

describe("currency detection", () => {
  it("uses the first region the store sells in", () => {
    expect(detectCurrency(["en-US", "de-DE"], ["EUR", "USD"])).toBe("USD");
    expect(detectCurrency(["en-GB", "de-AT"], ["EUR", "USD"])).toBe("EUR");
    expect(detectCurrency(["en"], ["EUR"])).toBeNull();
    expect(detectCurrency(["ro-RO"], ["EUR"])).toBeNull();
  });
});
