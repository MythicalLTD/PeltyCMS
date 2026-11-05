import { describe, expect, it } from "vitest";
import { canRate, minutes, parseTags, slaPercent, statusLabel, statusSlugValid, suggestText, distribution } from "./support";

const statusTr = (c: string) => `T(${c})`;

describe("support helpers", () => {
  it("normalises tags like the server", () => {
    expect(parseTags(" VIP Client, vip-client, billing, !!, ")).toEqual(["billing", "vip-client"]);
    expect(parseTags("")).toEqual([]);
  });
  it("computes SLA percentage", () => {
    expect(slaPercent({ within_sla: 3, responded: 4 })).toBe(75);
    expect(slaPercent({ within_sla: null, responded: 4 })).toBeNull();
    expect(slaPercent({ within_sla: 0, responded: 0 })).toBeNull();
  });
  it("formats minutes", () => {
    expect(minutes(null)).toBe("-");
    expect(minutes(45)).toBe("45m");
    expect(minutes(65)).toBe("1h 5m");
    expect(minutes(120)).toBe("2h");
    expect(minutes(1440 + 180)).toBe("1d 3h");
  });
  it("decides when a client may rate", () => {
    expect(canRate({ first_response_at: "x", merged_into: null, rating: null })).toBe(true);
    expect(canRate({ first_response_at: null, merged_into: null, rating: null })).toBe(false);
    expect(canRate({ first_response_at: "x", merged_into: null, rating: 3 })).toBe(false);
  });
});

describe("ticket statuses", () => {
  it("labels", () => {
    expect(statusLabel(undefined, "open", statusTr)).toBe("T(tickets.status.open)");
    expect(statusLabel({ slug: "open", name: "New", color: "default", sort_order: 0, awaiting_staff: true, builtin: true }, "open", statusTr)).toBe("New");
    expect(statusLabel({ slug: "vendor", name: "Vendor", color: "default", sort_order: 0, awaiting_staff: false, builtin: false }, "vendor", statusTr)).toBe("Vendor");
    expect(statusLabel(undefined, "gone", statusTr)).toBe("gone");
  });
  it("slugs", () => {
    expect(statusSlugValid("waiting_vendor")).toBe(true);
    expect(statusSlugValid("Bad")).toBe(false);
    expect(statusSlugValid("a")).toBe(false);
  });
});

describe("suggestText", () => {
  it("needs a real word", () => {
    expect(suggestText("a b", "")).toBeNull();
    expect(suggestText("VPS down", "")).toBe("VPS down");
    expect(suggestText("", "x".repeat(500))!.length).toBe(200);
  });
});

describe("distribution", () => {
  it("orders 5→1 with percents", () => {
    expect(distribution({ ratings: 4, avg_rating: "4", csat: "75", rated_tickets_share: null, r1: 1, r2: 0, r3: 0, r4: 1, r5: 2 })).toEqual([[5, 2, 50], [4, 1, 25], [3, 0, 0], [2, 0, 0], [1, 1, 25]]);
    expect(distribution({ ratings: 0, avg_rating: null, csat: null, rated_tickets_share: null, r1: 0, r2: 0, r3: 0, r4: 0, r5: 0 })[0]).toEqual([5, 0, 0]);
  });
});
