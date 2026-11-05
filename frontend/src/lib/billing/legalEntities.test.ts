import { describe, expect, it } from "vitest";
import { emptyEntity, entityPayload, formatNumber } from "./legalEntities";

describe("legal entities", () => {
  it("formats numbers like the server", () => {
    expect(formatNumber("{prefix}-{year}-{seq:5}", "RO", 2026, 7)).toBe("RO-2026-00007");
    expect(formatNumber("{prefix}{yy}/{seq}", "F", 2026, 123)).toBe("F26/123");
    expect(formatNumber("{prefix}-{year}", "F", 2026, 1)).toBeNull();
    expect(formatNumber("{prefix}-{seq:0}", "F", 2026, 1)).toBeNull();
    expect(formatNumber("{prefix}-{bad}-{seq}", "F", 2026, 1)).toBeNull();
    expect(formatNumber("{prefix} {seq}", "F", 2026, 1)).toBeNull();
  });
  it("validates drafts", () => {
    const ok = { ...emptyEntity, name: "Pelty SRL", invoice_prefix: "PS", credit_prefix: "PSC", due_days: "14" };
    expect(entityPayload(ok)?.due_days).toBe(14);
    expect(entityPayload({ ...ok, due_days: "" })?.due_days).toBeNull();
    expect(entityPayload({ ...ok, credit_prefix: "ps" })).toBeNull();
    expect(entityPayload({ ...ok, due_days: "400" })).toBeNull();
    expect(entityPayload({ ...ok, number_format: "{year}-{seq}" })).toBeNull();
    expect(entityPayload({ ...ok, name: " " })).toBeNull();
  });
});
