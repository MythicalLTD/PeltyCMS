import { describe, expect, it } from "vitest";
import { renewable } from "./RenewNowButton";

describe("renewable", () => {
  it("allows active/suspended recurring services with a due date", () => {
    expect(renewable({ status: "active", cycle: "monthly", next_due_date: "2026-11-01" })).toBe(true);
    expect(renewable({ status: "suspended", cycle: "annual", next_due_date: "2026-11-01" })).toBe(true);
    expect(renewable({ status: "terminated", cycle: "monthly", next_due_date: "2026-11-01" })).toBe(false);
    expect(renewable({ status: "active", cycle: "one_time", next_due_date: "2026-11-01" })).toBe(false);
    expect(renewable({ status: "active", cycle: "monthly", next_due_date: null })).toBe(false);
  });
});
