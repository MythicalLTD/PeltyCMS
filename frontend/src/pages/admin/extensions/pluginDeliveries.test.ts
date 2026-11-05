import { describe, expect, it } from "vitest";
import { deliveriesPath, DELIVERY_STATUSES } from "./PluginDeliveries";

describe("plugin delivery log", () => {
  it("builds the list path with an optional status", () => {
    expect(deliveriesPath("example-webhook", "")).toBe("/admin/plugins/example-webhook/deliveries");
    expect(deliveriesPath("a b", "failed")).toBe("/admin/plugins/a%20b/deliveries?status=failed");
  });
  it("matches the server's status set", () => {
    expect([...DELIVERY_STATUSES]).toEqual(["pending", "delivered", "failed"]);
  });
});
