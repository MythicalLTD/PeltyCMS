import { describe, expect, it } from "vitest";
import { blankSlaPolicy, minutesInput, minutesUntil, slaPolicyValid, slaSpecificity, slaStateVariant } from "@/lib/support/support";

describe("SLA policy helpers", () => {
  it("validates like the server", () => {
    expect(slaPolicyValid(blankSlaPolicy)).toBe(true);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: null })).toBe(false);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: null, resolution_minutes: 60 })).toBe(true);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: 0 })).toBe(false);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: 1.5 })).toBe(false);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: 525_601 })).toBe(false);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: 60, resolution_minutes: 30 })).toBe(false);
    expect(slaPolicyValid({ ...blankSlaPolicy, warn_percent: 100 })).toBe(false);
    expect(slaPolicyValid({ ...blankSlaPolicy, first_response_minutes: minutesInput("abc") })).toBe(false);
  });
  it("parses minute inputs", () => {
    expect(minutesInput("")).toBeNull();
    expect(minutesInput(" 45 ")).toBe(45);
  });
  it("ranks specificity like the server", () => {
    const r = (d: string | null, p: string | null) => slaSpecificity({ department_id: d, priority: p });
    expect([r("d", "high"), r("d", null), r(null, "high"), r(null, null)]).toEqual([3, 2, 1, 0]);
  });
  it("maps states to badge variants", () => {
    expect(slaStateVariant("breached")).toBe("destructive");
    expect(slaStateVariant("missed")).toBe("destructive");
    expect(slaStateVariant("warning")).toBe("default");
    expect(slaStateVariant("met")).toBe("secondary");
    expect(slaStateVariant("ok")).toBe("outline");
  });
  it("counts minutes until due", () => {
    const now = Date.parse("2026-01-01T10:00:00Z");
    expect(minutesUntil("2026-01-01T10:30:00Z", now)).toBe(30);
    expect(minutesUntil("2026-01-01T09:00:00Z", now)).toBe(-60);
  });
});
