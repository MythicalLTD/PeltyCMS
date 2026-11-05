import { describe, expect, it } from "vitest";
import { groupIssues, type NetworkIssue } from "./NetworkStatus";

const base: NetworkIssue = { id: "1", title: "x", description: "", component: "", kind: "outage", priority: "low", status: "investigating", starts_at: "2026-01-01T00:00:00Z", ends_at: null, created_at: "", updated_at: "", updates: [] };

describe("network status grouping", () => {
  it("splits active, future scheduled and resolved", () => {
    const now = Date.parse("2026-06-01T00:00:00Z");
    const g = groupIssues([
      base,
      { ...base, id: "2", status: "scheduled", kind: "maintenance", starts_at: "2026-07-01T00:00:00Z" },
      { ...base, id: "3", status: "scheduled", starts_at: "2026-05-01T00:00:00Z" },
      { ...base, id: "4", status: "resolved" },
    ], now);
    expect(g.active.map((i) => i.id)).toEqual(["1", "3"]);
    expect(g.scheduled.map((i) => i.id)).toEqual(["2"]);
    expect(g.resolved.map((i) => i.id)).toEqual(["4"]);
  });
});
