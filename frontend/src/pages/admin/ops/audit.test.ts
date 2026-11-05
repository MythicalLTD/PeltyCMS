import { describe, expect, it } from "vitest";
import { auditPath, auditQuery, EMPTY_FILTERS } from "./Audit";

describe("audit filters", () => {
  it("drops blank fields", () => {
    expect(auditQuery(EMPTY_FILTERS)).toBe("");
    expect(auditQuery({ ...EMPTY_FILTERS, q: "  ", ip: " " })).toBe("");
  });
  it("encodes every filter", () => {
    const q = new URLSearchParams(auditQuery({ action: "invoice.", actor: "a@b", ip: "10.0.", q: "x y", target: "t1", from: "2026-01-01", to: "2026-01-31", system: true }));
    expect(Object.fromEntries(q)).toEqual({ action: "invoice.", actor: "a@b", ip: "10.0.", q: "x y", target: "t1", from: "2026-01-01", to: "2026-01-31", system: "true" });
  });
  it("omits system=false", () => {
    expect(auditQuery({ ...EMPTY_FILTERS, action: "auth." })).toBe("action=auth.");
  });
  it("keeps the legacy path builder", () => {
    expect(auditPath("auth.", "", { ip: "1.2.3.4" })).toBe("/admin/audit-logs?action=auth.&ip=1.2.3.4");
  });
});
