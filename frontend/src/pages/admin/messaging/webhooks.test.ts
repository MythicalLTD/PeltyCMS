import { describe, expect, it } from "vitest";
import { deliveriesQuery, groupEvents, toggleFilter } from "./Webhooks";

describe("webhook endpoints page helpers", () => {
  it("groups events by sorted category", () => {
    const g = groupEvents([{ name: "ticket.opened", category: "ticket", description: "" }, { name: "invoice.paid", category: "invoice", description: "" }, { name: "invoice.created", category: "invoice", description: "" }]);
    expect(g.map(([c]) => c)).toEqual(["invoice", "ticket"]);
    expect(g[0]?.[1]).toHaveLength(2);
  });
  it("wildcards replace what they cover", () => {
    expect(toggleFilter(["invoice.paid", "ticket.opened"], "invoice.*", true)).toEqual(["ticket.opened", "invoice.*"]);
    expect(toggleFilter(["invoice.paid"], "*", true)).toEqual(["*"]);
    expect(toggleFilter(["*"], "*", false)).toEqual([]);
    expect(toggleFilter([], "order.created", true)).toEqual(["order.created"]);
  });
  it("builds the delivery query", () => {
    expect(deliveriesQuery("", "")).toBe("/admin/webhook-deliveries");
    expect(deliveriesQuery("e1", "failed")).toBe("/admin/webhook-deliveries?endpoint_id=e1&status=failed");
  });
});
