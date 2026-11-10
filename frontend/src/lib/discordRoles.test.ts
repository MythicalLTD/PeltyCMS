import { describe, expect, it } from "vitest";
import { isSnowflake, parseRoleIds } from "./discordRoles";

describe("discord roles", () => {
  it("checks snowflakes", () => {
    expect(isSnowflake("123456789012345678")).toBe(true);
    expect(isSnowflake("1234")).toBe(false);
    expect(isSnowflake("12345678901234567a")).toBe(false);
  });
  it("parses role lists", () => {
    expect(parseRoleIds("123456789012345678, 123456789012345679\n123456789012345678")).toEqual(["123456789012345678", "123456789012345679"]);
    expect(parseRoleIds("")).toBeNull();
    expect(parseRoleIds("123456789012345678 nope")).toBeNull();
    expect(parseRoleIds(Array.from({ length: 11 }, (_, i) => `1234567890123456${10 + i}`).join(","))).toBeNull();
  });
});
