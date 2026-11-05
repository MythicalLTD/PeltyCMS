import { describe, expect, it } from "vitest";
import { formatBytes, linkOk, monthGrid, move, shiftMonth, toggle } from "./adminOps";

describe("admin ops helpers", () => {
  it("moves and toggles", () => {
    expect(move([1, 2, 3], 0, 1)).toEqual([2, 1, 3]);
    expect(move([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
    expect(toggle(["stats"], "revenue")).toEqual(["stats", "revenue"]);
    expect(toggle(["stats", "revenue"], "stats")).toEqual(["revenue"]);
  });
  it("validates links like the server", () => {
    expect(linkOk("/admin/billing")).toBe(true);
    expect(linkOk("https://x.test")).toBe(true);
    expect(linkOk("//evil.test")).toBe(false);
    expect(linkOk("javascript:alert(1)")).toBe(false);
  });
  it("builds a Monday-first month grid", () => {
    const g = monthGrid("2026-10"); // 1 Oct 2026 is a Thursday
    expect(g.slice(0, 3)).toEqual([null, null, null]);
    expect(g[3]).toBe("2026-10-01");
    expect(g.filter(Boolean).length).toBe(31);
    expect(shiftMonth("2026-01", -1)).toBe("2025-12");
    expect(shiftMonth("2026-12", 1)).toBe("2027-01");
  });
  it("formats bytes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(null)).toBe("-");
  });
});
