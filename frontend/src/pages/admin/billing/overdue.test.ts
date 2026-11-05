import { describe, expect, it } from "vitest";
import { parseDays } from "./Overdue";

describe("parseDays", () => {
  it("splits, trims and drops empties", () => {
    expect(parseDays("1, 7,14, ")).toEqual([1, 7, 14]);
    expect(parseDays("")).toEqual([]);
  });
  it("keeps invalid entries so the server rejects them", () => {
    expect(parseDays("1, x")[1]).toBeNaN();
  });
});
