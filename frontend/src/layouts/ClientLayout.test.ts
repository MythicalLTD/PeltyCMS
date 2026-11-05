import { describe, expect, it } from "vitest";
import en from "../../../locales/en.json";
import ro from "../../../locales/ro.json";
import { CLIENT_NAV } from "./ClientLayout";

describe("client nav", () => {
  it("has no duplicate routes or labels", () => {
    expect(new Set(CLIENT_NAV.map((n) => n.to)).size).toBe(CLIENT_NAV.length);
    expect(new Set(CLIENT_NAV.map((n) => n.label)).size).toBe(CLIENT_NAV.length);
  });
  it("has translated labels in en and ro", () => {
    for (const dict of [en, ro] as Record<string, string>[]) {
      const labels = CLIENT_NAV.map((n) => dict[n.label]);
      expect(labels.every(Boolean)).toBe(true);

    }
  });
  it("has a single storefront entry", () => {
    expect(CLIENT_NAV.filter((n) => n.to === "/store" || n.to === "/order")).toHaveLength(1);
  });
});
