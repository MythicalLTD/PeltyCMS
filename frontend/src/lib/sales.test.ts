import { describe, expect, it } from "vitest";
import { fromLocalInput, salePrice, saleFor, timeLeft, toLocalInput, type PublicSale } from "./sales";

const s = (id: string, percent: string, products: string[], groups: string[]): PublicSale => ({ id, name: id, percent, product_ids: products, group_ids: groups, ends_at: "2099-01-01T00:00:00Z" });

describe("sales", () => {
  it("picks the highest matching sale", () => {
    const list = [s("a", "10", ["p"], []), s("b", "25", [], ["g"]), s("c", "50", ["x"], [])];
    expect(saleFor(list, "p", "g")?.id).toBe("b");
    expect(saleFor(list, "p", "other")?.id).toBe("a");
    expect(saleFor(list, "q", "h")).toBeNull();
  });
  it("prices and counts down", () => {
    expect(salePrice("10.00", "25")).toBe("7.50");
    expect(salePrice("4.99", "10")).toBe("4.49");
    expect(timeLeft("2026-01-02T01:30:00Z", Date.parse("2026-01-01T00:00:00Z"))).toEqual({ d: 1, h: 1, m: 30 });
    expect(timeLeft("2026-01-01T00:00:00Z", Date.parse("2026-01-01T00:00:01Z"))).toBeNull();
  });
  it("round-trips local datetimes", () => {
    const iso = "2026-03-04T05:06:00.000Z";
    expect(fromLocalInput(toLocalInput(iso))).toBe(iso);
  });
});
