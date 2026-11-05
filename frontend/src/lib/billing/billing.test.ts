import { describe, expect, it } from "vitest";
import { formatDate, formatMoney, statusTone } from "./billing";

describe("billing display", () => {
  it("formats per currency exponent", () => {
    expect(formatMoney("17.5000", "EUR", "en")).toBe("€17.50");
    expect(formatMoney("1500.0000", "JPY", "en")).toBe("¥1,500");
    expect(formatMoney("2.4690", "KWD", "en")).toContain("2.469");
    expect(formatMoney("1", "NOPE1", "en")).toBe("1 NOPE1");
  });
  it("keeps precision for large amounts", () => {
    expect(formatMoney("123456789012345.67", "EUR", "en")).toBe("€123,456,789,012,345.67");
  });
  it("dates and tones", () => {
    expect(formatDate("2026-11-29", "en")).toBe("2026-11-29");
    expect(formatDate(null, "en")).toBe("-");
    expect(statusTone("paid")).toBe("success");
    expect(statusTone("dead")).toBe("danger");
  });
});

describe("awaitingPayment", () => {
  it("flags active orders whose invoice is unpaid", async () => {
    const { awaitingPayment } = await import("./billing");
    expect(awaitingPayment("active", "unpaid")).toBe(true);
    expect(awaitingPayment("active", "paid")).toBe(false);
    expect(awaitingPayment("pending", "unpaid")).toBe(false);
  });
});
