import { describe, expect, it } from "vitest";
import { parsePayManyIds } from "./PayMany";

describe("parsePayManyIds", () => {
  it("needs two to fifty unique ids", () => {
    expect(parsePayManyIds(null)).toEqual([]);
    expect(parsePayManyIds("a")).toEqual([]);
    expect(parsePayManyIds("a,b")).toEqual(["a", "b"]);
    expect(parsePayManyIds("b, a, a")).toEqual(["a", "b"]);
    expect(parsePayManyIds(Array.from({ length: 51 }, (_, i) => `i${i}`).join(","))).toEqual([]);
  });
});
