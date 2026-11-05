import { describe, expect, it } from "vitest";
import { checksPath, isOpen, parseList } from "./Fraud";

describe("fraud page helpers", () => {
  it("parses comma, space and newline lists", () => {
    expect(parseList(" a.test,b.test\n\n c.test  ")).toEqual(["a.test", "b.test", "c.test"]);
    expect(parseList("")).toEqual([]);
  });

  it("builds the checks query for each filter", () => {
    expect(checksPath("open")).toBe("/admin/fraud/checks?open=true");
    expect(checksPath("reject")).toBe("/admin/fraud/checks?verdict=reject");
    expect(checksPath("all")).toBe("/admin/fraud/checks");
  });

  it("only offers decisions on undecided reviews of pending orders", () => {
    expect(isOpen({ verdict: "review", decision: null, order_status: "pending" })).toBe(true);
    expect(isOpen({ verdict: "review", decision: "approved", order_status: "pending" })).toBe(false);
    expect(isOpen({ verdict: "review", decision: null, order_status: "cancelled" })).toBe(false);
    expect(isOpen({ verdict: "reject", decision: null, order_status: "fraud" })).toBe(false);
  });
});
