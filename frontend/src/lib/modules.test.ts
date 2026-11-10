import { describe, expect, it } from "vitest";
import { MODULE_IDS, MODULE_ROUTES, moduleForPath, routeAllowed } from "./modules";

describe("feature modules", () => {
  it("maps routes to known modules only", () => {
    for (const [p, m] of MODULE_ROUTES) {
      expect(p.startsWith("/")).toBe(true);
      expect(MODULE_IDS).toContain(m);
      expect(moduleForPath(p)).toBe(m);
    }
  });
  it("leaves core routes alone", () => {
    expect(moduleForPath("/admin/invoices")).toBeUndefined();
    expect(routeAllowed("/admin/invoices", new Set())).toBe(true);
  });
});
