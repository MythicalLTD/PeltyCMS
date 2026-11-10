import { describe, expect, it } from "vitest";
import { MODULE_IDS, MODULE_ROUTES, MODULES_QUERY, moduleForPath, routeAllowed } from "./modules";
import { persistable } from "./query-cache";

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

describe("module list cache", () => {
  it("is never persisted to the browser cache", () => {
    const q = { queryKey: [...MODULES_QUERY.queryKey], meta: MODULES_QUERY.meta, state: { status: "success" } };
    expect(persistable(q as unknown as Parameters<typeof persistable>[0])).toBe(false);
  });
});
