import { describe, expect, it } from "vitest";
import { buildConfig, type SettingField } from "./Gateways";

const f = (key: string, kind: string): SettingField => ({ key, kind, required: false, options: [] });

describe("buildConfig", () => {
  const settings = [f("display_name", "text"), f("currencies", "text"), f("live", "bool"), f("webhook", "json"), f("min_amount", "number")];
  it("drops empty values, splits currencies and parses JSON", () => {
    const r = buildConfig(settings, { display_name: " Card ", currencies: "eur, ron usd", live: true, webhook: '{"secret":"s"}', min_amount: "" });
    expect(r.bad).toBeUndefined();
    expect(r.config).toEqual({ display_name: "Card", currencies: "EUR,RON,USD", live: true, webhook: { secret: "s" } });
  });
  it("omits false booleans", () => {
    expect(buildConfig([f("live", "bool")], { live: false }).config).toEqual({});
  });
  it("reports the field with invalid JSON", () => {
    expect(buildConfig(settings, { webhook: "{nope" }).bad).toBe("webhook");
  });
});
