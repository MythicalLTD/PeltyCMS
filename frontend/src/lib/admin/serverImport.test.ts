import { describe, expect, it } from "vitest";
import { importBody, type RemoteAccount } from "./serverImport";

const acc = (id: string, linked: string | null = null): RemoteAccount => ({ remote_id: id, username: id, domain: "", email: "", package: "", status: "", linked_service_id: linked });
const d = { customer_id: "c", product_id: "p", cycle: "monthly", recurring_amount: "9.99", next_due_date: "2027-01-01", suspended: false };

describe("importBody", () => {
  it("skips linked and unselected accounts", () => {
    const b = importBody([acc("a"), acc("b", "s1"), acc("c")], new Set(["a", "b"]), d);
    expect(b?.items.map((i) => i.remote_id)).toEqual(["a"]);
    expect(b?.items[0]).toMatchObject({ status: "active", recurring_amount: "9.99", next_due_date: "2027-01-01" });
  });
  it("validates the form", () => {
    expect(importBody([acc("a")], new Set(), d)).toBeNull();
    expect(importBody([acc("a")], new Set(["a"]), { ...d, recurring_amount: "1.23456" })).toBeNull();
    expect(importBody([acc("a")], new Set(["a"]), { ...d, next_due_date: "" })).toBeNull();
    expect(importBody([acc("a")], new Set(["a"]), { ...d, customer_id: "" })).toBeNull();
    expect(importBody([acc("a")], new Set(["a"]), { ...d, cycle: "one_time", next_due_date: "" })?.items[0]?.next_due_date).toBeNull();
    expect(importBody([acc("a")], new Set(["a"]), { ...d, suspended: true })?.items[0]?.status).toBe("suspended");
  });
});
