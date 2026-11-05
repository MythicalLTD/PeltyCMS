import { describe, expect, it } from "vitest";
import { metricPayload, trimDecimal, validQuantity } from "./usage";

const base = { product_id: "p", key: "bandwidth_gb", name: "Bandwidth", unit: "GB", unit_price: "0.01234567", included_quantity: "", active: true, source_key: "", source_divisor: "1", aggregate: "sum" as const };
describe("usage", () => {
  it("builds metric payloads", () => {
    expect(metricPayload(base)?.included_quantity).toBe("0");
    expect(metricPayload({ ...base, key: "Bad-Key" })).toBeNull();
    expect(metricPayload({ ...base, unit_price: "0.123456789" })).toBeNull();
    expect(metricPayload({ ...base, product_id: "" })).toBeNull();
    expect(metricPayload({ ...base, source_key: "bw.gb", source_divisor: "1024", aggregate: "max" })).toMatchObject({ source_key: "bw.gb", source_divisor: "1024", aggregate: "max" });
    expect(metricPayload({ ...base, source_key: "a b" })).toBeNull();
    expect(metricPayload({ ...base, source_key: "a..b" })).toBeNull();
    expect(metricPayload({ ...base, source_divisor: "0" })).toBeNull();
    expect(metricPayload({ ...base, source_divisor: "" })?.source_divisor).toBe("1");
  });
  it("validates quantities", () => {
    expect(validQuantity("1.5")).toBe(true);
    expect(validQuantity("0")).toBe(false);
    expect(validQuantity("1.1234567")).toBe(false);
    expect(validQuantity("-1")).toBe(false);
  });
  it("trims decimals", () => {
    expect(trimDecimal("1.500000")).toBe("1.5");
    expect(trimDecimal("2.000")).toBe("2");
    expect(trimDecimal("10")).toBe("10");
  });
});
