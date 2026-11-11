import { describe, expect, it } from "vitest";
import { DEFAULT_GUEST, emailLooksValid, identityComplete, identityError, parseGuest, shownFields, validLimit } from "./guestCheckout";

describe("guest checkout", () => {
  it("parses settings tolerantly", () => {
    expect(parseGuest(undefined)).toEqual(DEFAULT_GUEST);
    expect(parseGuest({ verify_minecraft: false, ip_per_hour: "3", email_per_hour: 0 })).toEqual({ verify_minecraft: false, ip_per_hour: 3, email_per_hour: 5 });
    expect(validLimit("10")).toBe(true);
    expect(validLimit("0")).toBe(false);
    expect(validLimit("1001")).toBe(false);
  });
  it("checks identity formats per game", () => {
    expect(identityError("minecraft_java", "username", "Notch")).toBeNull();
    expect(identityError("minecraft_java", "username", "no-dash")).toBe("format");
    expect(identityError("fivem", "username", "Some.Name-1")).toBeNull();
    expect(identityError("fivem", "license", "license:" + "a".repeat(40))).toBeNull();
    expect(identityError("fivem", "license", "abc")).toBe("format");
    expect(identityError("fivem", "steamid", "76561198000000000")).toBeNull();
    expect(identityError("fivem", "steamid", "12345")).toBe("format");
    expect(identityError("fivem", "license", "")).toBeNull();
  });
  it("requires a name for Minecraft and any identifier elsewhere", () => {
    expect(identityComplete("minecraft_java", { uuid: "x" })).toBe(false);
    expect(identityComplete("minecraft_java", { username: "Notch" })).toBe(true);
    expect(identityComplete("fivem", { license: "a" })).toBe(true);
    expect(identityComplete("fivem", { username: " " })).toBe(false);
  });
  it("hides the UUID when Mojang fills it", () => {
    expect(shownFields({ available: true, game: "minecraft_java", fields: ["username", "uuid"], verify_minecraft: true })).toEqual(["username"]);
    expect(shownFields({ available: true, game: "minecraft_java", fields: ["username", "uuid"], verify_minecraft: false })).toEqual(["username", "uuid"]);
  });
  it("validates emails", () => {
    expect(emailLooksValid("a@b.co")).toBe(true);
    expect(emailLooksValid("a@b")).toBe(false);
    expect(emailLooksValid("a b@c.de")).toBe(false);
  });
});
