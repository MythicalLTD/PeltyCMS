import { describe, expect, it } from "vitest";
import { communityConfig, fillPercent, voteLink } from "./community";

describe("community", () => {
  it("fills vote links", () => {
    expect(voteLink("https://v.example/s/1?u={username}", "Steve Two")).toBe("https://v.example/s/1?u=Steve%20Two");
    expect(voteLink("https://v.example/s/1?u={username}", "a&b=c")).toBe("https://v.example/s/1?u=a%26b%3Dc");
    expect(voteLink("https://v.example/s/1", "Steve")).toBe("https://v.example/s/1");
  });
  it("computes fill", () => {
    expect(fillPercent(3, 50)).toBe(6);
    expect(fillPercent(60, 50)).toBe(100);
    expect(fillPercent(1, 0)).toBe(0);
  });
  it("parses settings", () => {
    expect(communityConfig(undefined)).toMatchObject({ goal_amount: "0", leaderboard_size: 10, show_donors: false });
    expect(communityConfig({ leaderboard_size: 500 }).leaderboard_size).toBe(50);
  });
});
