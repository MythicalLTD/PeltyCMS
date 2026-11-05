import { describe, expect, it } from "vitest";
import { persistable } from "./query-cache";

const q = (queryKey: unknown[], status = "success", meta?: Record<string, unknown>) =>
  ({ queryKey, state: { status }, meta }) as unknown as Parameters<typeof persistable>[0];

describe("persistable", () => {
  it("keeps ordinary successful data", () => {
    expect(persistable(q(["client", "invoices"]))).toBe(true);
    expect(persistable(q(["admin", "tickets", 3]))).toBe(true);
  });
  it("never stores me, failures, opt-outs or sensitive keys", () => {
    expect(persistable(q(["me"]))).toBe(false);
    expect(persistable(q(["client", "invoices"], "error"))).toBe(false);
    expect(persistable(q(["x"], "success", { persist: false }))).toBe(false);
    for (const k of ["api-keys", "mfa", "settings", "sso", "smtp", "gateways", "tokens", "passwords", "secrets", "webhooks"])
      expect(persistable(q(["admin", k]))).toBe(false);
  });
});
