import { describe, expect, it } from "vitest";
import { askedFields, EMPTY_FIELD, fieldBody, fieldDraft, fieldDraftValid, identityBody, identityDisplay, knownIdentity, missingRequired, type IdentityGame, type PackageField } from "./checkoutFields";
import { identityComplete, identityError } from "../guestCheckout";

const fivem: IdentityGame = {
  game: "fivem", name: "FiveM",
  fields: [
    { field: "username", value: null, required: false },
    { field: "license", value: null, required: true },
    { field: "steamid", value: "76561198000000001", required: false },
    { field: "discord_id", value: null, required: false },
    { field: "fivem", value: "4242", required: false },
  ],
};

describe("player details at checkout", () => {
  it("asks only for missing required IDs and the FiveM licence", () => {
    expect(askedFields(fivem).map((f) => f.field)).toEqual(["license"]);
    const optional = { ...fivem, fields: fivem.fields.map((f) => ({ ...f, required: false })) };
    expect(askedFields(optional).map((f) => f.field)).toEqual(["license"]);
    const rust: IdentityGame = { game: "rust", name: "Rust", fields: [{ field: "username", value: null, required: false }, { field: "steamid", value: null, required: true }] };
    expect(askedFields(rust).map((f) => f.field)).toEqual(["steamid"]);
  });
  it("tracks what is still missing and what is known", () => {
    expect(missingRequired([fivem], {})).toEqual(["fivem.license"]);
    expect(missingRequired([fivem], { fivem: { license: "  " } })).toEqual(["fivem.license"]);
    expect(missingRequired([fivem], { fivem: { license: "license:abc" } })).toEqual([]);
    expect(knownIdentity([fivem], { fivem: { license: " ab " } })).toEqual({ steamid: "76561198000000001", fivem: "4242", license: "ab" });
  });
  it("builds the checkout identity body without empty values", () => {
    expect(identityBody({ fivem: { license: " license:aa ", username: "" }, redm: { steamid: " " } })).toEqual({ fivem: { license: "license:aa" } });
  });
  it("keeps FiveM identifier prefixes", () => {
    expect(identityDisplay("license", "ab")).toBe("license:ab");
    expect(identityDisplay("license", "license:ab")).toBe("license:ab");
    expect(identityDisplay("fivem", "42")).toBe("fivem:42");
    expect(identityDisplay("steamid", "7656")).toBe("7656");
  });
  it("checks formats including the Cfx.re id and Discord", () => {
    expect(identityError("fivem", "fivem", "fivem:123")).toBeNull();
    expect(identityError("fivem", "fivem", "abc")).toBe("format");
    expect(identityError("fivem", "discord_id", "123456789012345678")).toBeNull();
    expect(identityError("fivem", "discord_id", "12")).toBe("format");
    expect(identityComplete("fivem", { username: "Niko" }, ["license"])).toBe(false);
    expect(identityComplete("fivem", { username: "Niko", license: "x" }, ["license"])).toBe(true);
  });
});

describe("package checkout fields", () => {
  it("sends identity_key only for text and number fields", () => {
    expect(fieldBody({ ...EMPTY_FIELD, name: " Cfx ", identity_key: "fivem" })).toMatchObject({ name: "Cfx", kind: "text", identity_key: "fivem", choices: [] });
    expect(fieldBody({ ...EMPTY_FIELD, name: "Pick", kind: "select", choices: "a\nb, c", identity_key: "license" })).toMatchObject({ choices: ["a", "b", "c"], identity_key: null });
    expect(fieldBody({ ...EMPTY_FIELD, name: "x", identity_key: "" }).identity_key).toBeNull();
  });
  it("validates drafts and round-trips saved fields", () => {
    expect(fieldDraftValid(EMPTY_FIELD)).toBe(false);
    expect(fieldDraftValid({ ...EMPTY_FIELD, name: "Pick", kind: "select" })).toBe(false);
    expect(fieldDraftValid({ ...EMPTY_FIELD, name: "Pick", kind: "select", choices: "a" })).toBe(true);
    const saved: PackageField = { id: "1", product_id: "p", name: "Licence", description: "", kind: "text", choices: [], pattern: "", required: true, admin_only: false, sort_order: 0, identity_key: "license" };
    expect(fieldBody(fieldDraft(saved))).toEqual({ name: "Licence", description: "", kind: "text", choices: [], pattern: "", required: true, admin_only: false, sort_order: 0, identity_key: "license" });
  });
});
