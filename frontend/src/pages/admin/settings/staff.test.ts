import { describe, expect, it } from "vitest";
import { filterPerms, groupPermissions, setGroup, toggle, validRoleName } from "./Staff";
import { auditPath } from "../ops/Audit";

const permLabel = (p: string) => (p === "tickets.manage" ? "Handle support" : p);

describe("staff helpers", () => {
  it("groups permissions by prefix", () => {
    expect(groupPermissions(["users.read", "admin.access", "users.manage"])).toEqual([["admin", ["admin.access"]], ["users", ["users.read", "users.manage"]]]);
  });
  it("toggles sorted", () => {
    expect(toggle(["b"], "a")).toEqual(["a", "b"]);
    expect(toggle(["a", "b"], "a")).toEqual(["b"]);
  });
  it("validates role names like the backend", () => {
    expect(validRoleName("billing_staff")).toBe(true);
    expect(validRoleName("Billing")).toBe(false);
    expect(validRoleName("a")).toBe(false);
  });
  it("builds audit paths", () => {
    expect(auditPath("", " ")).toBe("/admin/audit-logs");
    expect(auditPath("invoice.", "a@b")).toBe("/admin/audit-logs?action=invoice.&actor=a%40b");
  });
});

describe("role editor helpers", () => {
  it("toggles a whole group", () => {
    expect(setGroup(["a.x", "b.y"], ["b.y", "b.z"], true)).toEqual(["a.x", "b.y", "b.z"]);
    expect(setGroup(["a.x", "b.y", "b.z"], ["b.y", "b.z"], false)).toEqual(["a.x"]);
  });
  it("filters by name or label", () => {
    expect(filterPerms(["tickets.manage", "users.read"], "support", permLabel)).toEqual(["tickets.manage"]);
    expect(filterPerms(["tickets.manage", "users.read"], " ", permLabel)).toHaveLength(2);
  });
});
