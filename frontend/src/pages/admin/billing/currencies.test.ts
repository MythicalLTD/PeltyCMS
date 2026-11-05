import { describe, expect, it } from "vitest";
import { currencyPayload, example } from "./Currencies";

const d = { code: "chf", exponent: "2", enabled: true, rate: "0.95", prefix: "", suffix: " CHF" };

describe("currencyPayload", () => {
  it("uppercases the code on create and omits it on update", () => {
    expect(currencyPayload(d, true)).toMatchObject({ code: "CHF", exponent: 2, rate: "0.95" });
    expect(currencyPayload(d, false)).not.toHaveProperty("code");
  });
  it("rejects bad input", () => {
    expect(currencyPayload({ ...d, code: "CH" }, true)).toBeNull();
    expect(currencyPayload({ ...d, rate: "0" }, true)).toBeNull();
    expect(currencyPayload({ ...d, rate: "-1" }, true)).toBeNull();
    expect(currencyPayload({ ...d, exponent: "5" }, true)).toBeNull();
  });
  it("formats examples", () => {
    expect(example("4.975", 2)).toBe("497.50");
    expect(example("161.5", 0)).toBe("16150");
  });
});
