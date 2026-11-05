import { describe, expect, it } from "vitest";
import { duration, parseMinutes, progress, unbilledMinutes } from "./projects";

describe("projects helpers", () => {
  it("formats durations", () => { expect(duration(127)).toBe("2h 07m"); expect(duration(0)).toBe("0h 00m"); });
  it("computes progress", () => { expect(progress(1, 3)).toBe(33); expect(progress(0, 0)).toBe(0); });
  it("parses minutes", () => {
    expect(parseMinutes("1:30")).toBe(90);
    expect(parseMinutes("1.5h")).toBe(90);
    expect(parseMinutes("45")).toBe(45);
    expect(parseMinutes("0")).toBeNull();
    expect(parseMinutes("25:00")).toBeNull();
    expect(parseMinutes("abc")).toBeNull();
  });
  it("sums unbilled billable time", () => {
    const base = { task_id: null, note: "", spent_on: "2026-01-01", user: null };
    expect(unbilledMinutes([{ ...base, id: "a", minutes: 30, billable: true, invoice_id: null }, { ...base, id: "b", minutes: 20, billable: false, invoice_id: null }, { ...base, id: "c", minutes: 10, billable: true, invoice_id: "x" }])).toBe(30);
  });
});
