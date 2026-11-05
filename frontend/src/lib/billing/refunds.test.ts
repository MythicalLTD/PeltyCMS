import { describe, expect, it } from "vitest";
import { refundError, refundedTotal, type Refund } from "./refunds";

const r = (amount: string, status: Refund["status"]): Refund => ({ id: amount + status, payment_id: "p", invoice_id: "i", gateway: "stripe", amount, currency: "EUR", status, reason: "", error: null, credit_note_id: null, created_at: "" });

describe("refunds", () => {
  it("validates amounts exactly", () => {
    expect(refundError("", "10.00")).toBeNull();
    expect(refundError("", "0.0000")).toBe("error.payment_fully_refunded");
    expect(refundError("10.00", "10")).toBeNull();
    expect(refundError("10.01", "10")).toBe("error.refund_exceeds_payment");
    expect(refundError("0", "10")).toBe("error.invalid_amount");
    expect(refundError("1.001", "10")).toBe("error.invalid_amount");
    expect(refundError("-1", "10")).toBe("error.invalid_amount");
  });
  it("sums succeeded refunds without floats", () => {
    expect(refundedTotal([r("0.1000", "succeeded"), r("0.2000", "succeeded"), r("5", "failed")])).toBe("0.30");
    expect(refundedTotal([])).toBe("0.00");
    expect(refundedTotal([r("109.0000", "succeeded"), r("10", "succeeded")])).toBe("119.00");
  });
});
