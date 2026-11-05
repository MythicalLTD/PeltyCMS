import { describe, expect, it } from "vitest";
import { activityHref, activityPath, allowedKinds, canViewTab, effectiveAccess, genericActivityTitle, selectedTab, tabSearch, validNote, type ActivityItem, type WorkspaceAccess } from "./customerWorkspace";

const access: WorkspaceAccess = { finance: true, services: true, support: true, operations: true, audit: true, manage_customer: true };
describe("customer workspace contract helpers", () => {
  it("intersects the backend mask with actual read and manage permissions", () => {
    expect(effectiveAccess(access, ["customers.read", "invoices.read"])).toEqual({ finance: true, services: false, support: false, operations: false, audit: false, manage_customer: false });
    expect(effectiveAccess({ ...access, finance: false }, ["invoices.manage"]).finance).toBe(false);
    expect(effectiveAccess(access, ["services.read", "jobs.read"]).operations).toBe(false);
  });
  it("hides restricted tabs and activity filters and falls back safely", () => {
    const restricted = effectiveAccess(access, ["customers.read"]);
    expect(canViewTab("transactions", restricted)).toBe(false);
    expect(selectedTab(new URLSearchParams("tab=invoices&core=1"), restricted)).toBe("overview");
    expect(selectedTab(new URLSearchParams("tab=bogus"), access)).toBe("overview");
    expect(allowedKinds(restricted)).toEqual(["all", "email", "note", "customer"]);
  });
  it("preserves other URL parameters and percent-encodes the raw opaque cursor exactly once", () => {
    const params = new URLSearchParams("core=1&foo=bar&tab=notes");
    expect(tabSearch(params, "contacts").toString()).toBe("core=1&foo=bar&tab=contacts");
    expect(params.get("tab")).toBe("notes");
    const cursor = '{"kind":"all","key":"note:a+b&%"}';
    const url = new URL(activityPath("c", "all", cursor), "https://example.test");
    expect(url.searchParams.get("before")).toBe(cursor);
    expect(url.searchParams.get("limit")).toBe("25");
  });
  it("validates trimmed notes using the backend Unicode character limit", () => {
    expect(validNote("   \n ")).toBe(false);
    expect(validNote("a".repeat(10_001))).toBe(false);
    expect(validNote("😀".repeat(10_000))).toBe(true);
    expect(validNote(" hello ")).toBe(true);
  });
  it("uses real resource routes, repairs old root links, and preserves server deep links", () => {
    const item: ActivityItem = { id: "invoice:i", kind: "invoice", occurred_at: "2026-01-01T00:00:00Z", title: "Invoice", status: null, href: "/admin/customers/c" };
    expect(activityHref("c", item)).toBe("/admin/invoices/i");
    expect(activityHref("c", { ...item, kind: "note", id: "note:n" })).toBe("/admin/customers/c?tab=notes");
    expect(activityHref("c", { ...item, href: "/admin/customers/c?tab=invoices" })).toBe("/admin/customers/c?tab=invoices");
    expect(genericActivityTitle(item)).toBe(true);
    expect(genericActivityTitle({ ...item, title: "INV-2026-123" })).toBe(false);
  });
});
