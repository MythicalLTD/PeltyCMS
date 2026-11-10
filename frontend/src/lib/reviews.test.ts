import { describe, expect, it } from "vitest";
import { halfStars, histogramPercents, reviewsConfig } from "./reviews";

describe("reviews", () => {
  it("parses settings with defaults", () => {
    expect(reviewsConfig(undefined)).toEqual({ require_approval: true, verified_only: false, questions: true });
    expect(reviewsConfig({ verified_only: true, questions: "no" })).toEqual({ require_approval: true, verified_only: true, questions: true });
  });
  it("builds the histogram from 5 stars down", () => {
    const h = histogramPercents({ count: 4, average: "4.25", histogram: [0, 0, 1, 1, 2] });
    expect(h.map((x) => x.stars)).toEqual([5, 4, 3, 2, 1]);
    expect(h[0]).toEqual({ stars: 5, count: 2, percent: 50 });
    expect(histogramPercents({ count: 0, average: "0", histogram: [0, 0, 0, 0, 0] })[0]?.percent).toBe(0);
  });
  it("rounds averages to half stars", () => {
    expect(halfStars("4.25")).toBe(4.5);
    expect(halfStars("4.24")).toBe(4);
    expect(halfStars("x")).toBe(0);
    expect(halfStars("9")).toBe(5);
  });
});
