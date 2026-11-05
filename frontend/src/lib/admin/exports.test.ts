import { describe, expect, it } from "vitest";
import { exportUrl, trimRate } from "./exports";

describe("exports", () => {
  it("builds download urls", () => {
    expect(exportUrl("tax", "2026-01", "2026-03")).toBe("/api/v1/admin/reports/tax.csv?from=2026-01&to=2026-03");
    expect(exportUrl("payments", "2026-01", "2026-01")).toBe("/api/v1/admin/exports/payments.csv?from=2026-01&to=2026-01");
  });
  it("trims rates without floats", () => {
    expect(trimRate("24.0000")).toBe("24");
    expect(trimRate("9.50")).toBe("9.5");
    expect(trimRate("0")).toBe("0");
    expect(trimRate("100")).toBe("100");
  });
});
