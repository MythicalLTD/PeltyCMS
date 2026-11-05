import { describe, expect, it } from "vitest";
import { chartRows, groupByCategory } from "./reportLibrary";

describe("report library", () => {
  it("groups by category", () => {
    const g = groupByCategory([{ key: "a", category: "x", columns: [], chart: null }, { key: "b", category: "x", columns: [], chart: null }, { key: "c", category: "y", columns: [], chart: null }]);
    expect(Object.keys(g)).toEqual(["x", "y"]);
    expect(g.x!.length).toBe(2);
  });
  it("chart rows merge labels and skip non-numbers", () => {
    const rows = chartRows({ key: "k", columns: ["m", "c", "v"], chart: [0, 2], rows: [["2026-01", "EUR", "10.50"], ["2026-01", "USD", "1"], ["2026-02", "EUR", "∞"]] });
    expect(rows).toEqual([{ label: "2026-01", value: 11.5 }]);
  });
});
