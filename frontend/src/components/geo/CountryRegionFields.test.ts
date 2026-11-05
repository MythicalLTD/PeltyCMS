import { describe, expect, it } from "vitest";
import { foldGeo } from "./CountryRegionFields";

describe("foldGeo", () => {
  it("matches diacritics-insensitively", () => {
    expect(foldGeo("Caraș-Severin").includes(foldGeo("caras"))).toBe(true);
    expect(foldGeo("București").includes(foldGeo("bucuresti"))).toBe(true);
    expect(foldGeo("Timiș").includes(foldGeo("timis"))).toBe(true);
  });
});
