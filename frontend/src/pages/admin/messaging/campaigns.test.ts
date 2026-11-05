import { describe, expect, it } from "vitest";
import { cleanSegment, emptySegment, localToIso, MERGE_KEY, progress, takeMergeIds } from "./Campaigns";

describe("campaign helpers", () => {
  it("cleans segments", () => {
    expect(cleanSegment({ ...emptySegment(), country: " ro ", currency: "" })).toMatchObject({ country: "RO", currency: null, statuses: ["active"] });
  });
  it("computes progress", () => {
    expect(progress({ total: 0, sent: 0, failed: 0, skipped: 0 })).toBe(0);
    expect(progress({ total: 4, sent: 2, failed: 0, skipped: 1 })).toBe(75);
  });
  it("takes the mail-merge selection once", () => {
    const m = new Map<string, string>([[MERGE_KEY, JSON.stringify(["a", 3, "b"])]]);
    const store = { getItem: (k: string) => m.get(k) ?? null, removeItem: (k: string) => void m.delete(k) };
    expect(takeMergeIds(store)).toEqual(["a", "b"]);
    expect(takeMergeIds(store)).toEqual([]);
    m.set(MERGE_KEY, "{bad");
    expect(takeMergeIds(store)).toEqual([]);
  });
  it("converts local datetime to ISO", () => {
    expect(localToIso("")).toBeNull();
    expect(localToIso("nope")).toBeNull();
    expect(localToIso("2030-01-02T03:04")).toMatch(/^2030-01-0[12]T/);
  });
});
