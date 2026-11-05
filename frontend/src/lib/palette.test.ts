import { describe, expect, it } from "vitest";
import { suggestedPages } from "@/lib/palette";
import { categoryForPath } from "@/lib/navRegistry";

describe("palette suggestions", () => {
  it("lists primary pages and skips recents", () => {
    const pages = [{ to: "/a", primary: true }, { to: "/b" }, { to: "/c", primary: true }];
    expect(suggestedPages(pages, ["/a"]).map((p) => p.to)).toEqual(["/c"]);
  });
});

describe("categoryForPath", () => {
  it("resolves hubs, aliases and leaves", () => {
    expect(categoryForPath("admin", "/admin/hub/billing")).toMatchObject({ hub: true, cat: { id: "billing" } });
    expect(categoryForPath("admin", "/admin/settings")).toMatchObject({ hub: true, cat: { id: "system" } });
    expect(categoryForPath("admin", "/admin/invoices/x")).toMatchObject({ hub: false, cat: { id: "billing" } });
    expect(categoryForPath("client", "/hub/account")).toMatchObject({ hub: true, cat: { id: "account" } });
    expect(categoryForPath("admin", "/admin")?.cat.id).toBe("dashboard");
  });
  it("admin dashboard is a single direct link", () => {
    expect(categoryForPath("admin", "/admin")?.cat.items).toHaveLength(1);
  });
});
