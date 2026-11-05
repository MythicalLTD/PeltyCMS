import { describe, expect, it } from "vitest";
import { daysUntil, mulDecimal, orderPrice, parseNameservers, statusVariant, yearsRange, type Tld, TLD_DEFAULTS, addDecimal, orderTotal, effectivePricing, missingFields, type TldField } from "./domains";

const t: Tld = { tld: "com", currency: "EUR", register_price: "10.9900", renew_price: "12.50", transfer_price: "8.00", min_years: 1, max_years: 3, registrar: "mock", active: true, ...TLD_DEFAULTS };

describe("domains", () => {
  it("multiplies decimals exactly", () => {
    expect(mulDecimal("10.9900", 3)).toBe("32.9700");
    expect(mulDecimal("0.10", 3)).toBe("0.30");
    expect(mulDecimal("5", 2)).toBe("10");
    expect(mulDecimal("-0.05", 2)).toBe("-0.10");
    expect(mulDecimal("0.0001", 0)).toBe("0.0000");
  });
  it("prices orders", () => {
    expect(orderPrice(t, "register", 2)).toBe("21.9800");
    expect(orderPrice(t, "transfer", 5)).toBe("8.00");
    expect(yearsRange(t)).toEqual([1, 2, 3]);
  });
  it("parses nameservers", () => {
    expect(parseNameservers("NS1.x.io\nns2.x.io., ns1.x.io")).toEqual(["ns1.x.io", "ns2.x.io"]);
    expect(parseNameservers("  ")).toEqual([]);
  });
  it("counts days and maps status", () => {
    expect(daysUntil("2026-11-05", "2026-10-29")).toBe(7);
    expect(daysUntil(null, "2026-10-29")).toBeNull();
    expect(statusVariant("expired")).toBe("destructive");
    expect(statusVariant("active")).toBe("default");
  });
});

describe("addDecimal", () => {
  it("adds exact decimals of mixed scale", () => {
    expect(addDecimal("10.99", "2.5")).toBe("13.49");
    expect(addDecimal("0.1", "0.2")).toBe("0.3");
    expect(addDecimal("1", "-1.25")).toBe("-0.25");
  });
});

describe("orderTotal", () => {
  it("adds per-year add-ons exactly", () => {
    const p = { ...t, id_protection_price: "2.50", dns_management_price: "1.00" };
    expect(orderTotal(p, "register", 2, true, true)).toBe("28.9800");
    expect(orderTotal(p, "register", 2, false, false)).toBe("21.9800");
    expect(orderTotal(p, "transfer", 3, true, false)).toBe("10.50");
  });
});

describe("premium + extra fields", () => {
  const base = { tld: "ro", currency: "EUR", register_price: "10.00", renew_price: "12.00", transfer_price: "8.00", min_years: 1, max_years: 5, registrar: "mock", active: true, ...TLD_DEFAULTS } as Tld;
  it("applies premium prices to register/renew only", () => {
    const p = effectivePricing(base, { register_price: "275.00", renew_price: "33.00" });
    expect([p.register_price, p.renew_price, p.transfer_price]).toEqual(["275.00", "33.00", "8.00"]);
    expect(effectivePricing(base, null)).toBe(base);
  });
  it("lists missing required fields", () => {
    const f: TldField[] = [
      { tld: "ro", key: "cnp", label: "CNP", kind: "text", options: [], required: true, max_length: 13, sort_order: 0 },
      { tld: "ro", key: "agree", label: "Agree", kind: "checkbox", options: [], required: true, max_length: 200, sort_order: 1 },
      { tld: "ro", key: "note", label: "Note", kind: "text", options: [], required: false, max_length: 200, sort_order: 2 },
    ];
    expect(missingFields(f, {})).toEqual(["cnp", "agree"]);
    expect(missingFields(f, { cnp: "  ", agree: false })).toEqual(["cnp", "agree"]);
    expect(missingFields(f, { cnp: "1", agree: true })).toEqual([]);
  });
});
