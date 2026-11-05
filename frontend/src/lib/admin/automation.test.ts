import { describe, expect, it } from "vitest";
import { formatDuration, healthVariant, intervalParts, iso, looksLikeCron, resultSummary, taskLabel } from "./automation";

describe("automation", () => {
  it("formats intervals", () => {
    expect(intervalParts(86400)).toEqual(["automation.every_day", 1]);
    expect(intervalParts(3600)).toEqual(["automation.every_hour", 1]);
    expect(intervalParts(300)).toEqual(["automation.every_min", 5]);
  });
  it("maps health", () => {
    expect(healthVariant("failing")).toBe("destructive");
    expect(healthVariant("never")).toBe("outline");
  });
  it("summarizes results", () => {
    expect(resultSummary({ invoices: 2, nested: { a: 1 }, ok: true })).toBe("invoices: 2, ok: true");
    expect(resultSummary(null)).toBe("");
  });
});

describe("taskLabel", () => {
  const dict: Record<string, string> = { "automation.task.billing.autopay": "Automatic card payments" };
  const tc = (k: string) => dict[k] ?? k;
  it("translates known kinds and falls back to the raw kind", () => {
    expect(taskLabel(tc, "billing.autopay")).toBe("Automatic card payments");
    expect(taskLabel(tc, "custom.thing")).toBe("custom.thing");
  });
});

describe("cron helpers", () => {
  it("formats durations", () => {
    expect(formatDuration(null)).toBe("-");
    expect(formatDuration(850)).toBe("850 ms");
    expect(formatDuration(12_300)).toBe("12.3 s");
    expect(formatDuration(240_000)).toBe("4 min");
  });
  it("checks field count and converts times", () => {
    expect(looksLikeCron("0 3 * * *")).toBe(true);
    expect(looksLikeCron("0 0 3 * * *")).toBe(false);
    expect(iso(0)).toBeNull();
    expect(iso(1_793_534_400)).toBe("2026-11-01T12:00:00.000Z");
  });
});
