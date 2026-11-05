import { describe, expect, it } from "vitest";
import { lastMonths, monthsBetween, series, sumExact } from "./reports";

describe("reports helpers", () => {
  it("lists months across a year boundary", () => {
    expect(monthsBetween("2025-11", "2026-02")).toEqual(["2025-11", "2025-12", "2026-01", "2026-02"]);
    expect(lastMonths(12, new Date(Date.UTC(2026, 9, 29)))).toEqual({ from: "2025-11", to: "2026-10" });
  });
  it("zero-fills series and sums exactly", () => {
    const pts = [{ month: "2026-01", currency: "EUR", amount: "0.1000", count: 1 }, { month: "2026-02", currency: "EUR", amount: "0.2000", count: 1 }, { month: "2026-02", currency: "USD", amount: "5", count: 1 }];
    expect(series(pts, ["2025-12", "2026-01", "2026-02"], "EUR")).toEqual([0, 0.1, 0.2]);
    expect(sumExact(pts, "EUR")).toBe("0.3000");
    expect(sumExact([{ month: "x", currency: "EUR", amount: "-1.5", count: 1 }], "EUR")).toBe("-1.5000");
  });
});
