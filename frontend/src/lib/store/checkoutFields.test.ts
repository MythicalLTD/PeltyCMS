import { describe, expect, it } from "vitest";
import { askedFields, EMPTY_FIELD, fieldBody, fieldDraft, fieldDraftValid, FIELD_KINDS, identityBody, identityDisplay, keyValid, knownIdentity, missingRequired, moduleMissing, moveField, presetMissing, rankPresets, suggestKey, type FieldPreset, type IdentityGame, type PackageField } from "./checkoutFields"
import { emailOk, fieldErrors, inputProps, selectedForConfig, type OrderField } from "./storefront";;
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
    expect(fieldBody(fieldDraft(saved))).toEqual({ name: "Licence", description: "", kind: "text", choices: [], pattern: "", required: true, admin_only: false, sort_order: 0, identity_key: "license", key: null });
    expect(fieldBody(fieldDraft({ ...saved, field_key: "license" })).key).toBe("license");
  });
});

const pf = (p: Partial<PackageField>): PackageField => ({ id: Math.random().toString(36), product_id: "p", name: "x", description: "", kind: "text", choices: [], pattern: "", required: false, admin_only: false, sort_order: 0, identity_key: null, module_key: null, field_key: null, ...p });
const fivemPreset: FieldPreset = {
  id: "fivem", name: "FiveM / RedM player", games: ["fivem", "redm"],
  fields: ["license", "fivem", "steamid", "discord_id", "username"].map((k) => ({ key: k, name: k, kind: "text", identity_key: k, required: k === "license" })),
};
const account: FieldPreset = { id: "account", name: "Account", games: [], fields: [
  { key: "account_username", name: "u", kind: "text", identity_key: null, required: true },
  { key: "account_password", name: "p", kind: "password", identity_key: null, required: true },
  { key: "account_email", name: "e", kind: "email", identity_key: null, required: false },
] };
const mc: FieldPreset = { id: "minecraft_java", name: "MC", games: ["minecraft_java"], fields: [{ key: "username", name: "u", kind: "text", identity_key: "username", required: true }] };

describe("custom-field kit", () => {
  it("has e-mail and password kinds", () => {
    expect(FIELD_KINDS).toContain("email");
    expect(FIELD_KINDS).toContain("password");
    // Passwords are never auto-filled from a player ID.
    expect(fieldBody({ ...EMPTY_FIELD, name: "Pw", kind: "password", identity_key: "license" }).identity_key).toBeNull();
    expect(fieldBody({ ...EMPTY_FIELD, name: "N", kind: "number", pattern: "x" }).pattern).toBe("");
  });
  it("presets only add what the package lacks", () => {
    expect(presetMissing(fivemPreset, []).length).toBe(5);
    const have = [pf({ identity_key: "license", field_key: "rockstar" }), pf({ field_key: "discord_id" })];
    expect(presetMissing(fivemPreset, have).map((f) => f.key)).toEqual(["fivem", "steamid", "username"]);
    expect(presetMissing(account, [pf({ field_key: "account_password", kind: "password" })]).map((f) => f.key)).toEqual(["account_username", "account_email"]);
    // A module-declared field with the same key counts too.
    expect(presetMissing(account, [pf({ module_key: "account_email" })]).map((f) => f.key)).toEqual(["account_username", "account_password"]);
  });
  it("suggests presets for the store's games first", () => {
    const r = rankPresets([mc, account, fivemPreset], ["fivem"]);
    expect(r.map((x) => [x.preset.id, x.suggested])).toEqual([["fivem", true], ["minecraft_java", false], ["account", false]]);
    expect(rankPresets([mc, fivemPreset], []).every((x) => !x.suggested)).toBe(true);
  });
  it("lists module keys still missing", () => {
    const uses = [{ key: "license", name: "L", kind: "text", identity_key: "license", required: true }, { key: "account_password", name: "P", kind: "password", required: true }];
    expect(moduleMissing(uses, [pf({ identity_key: "license", field_key: "lic" })]).map((u) => u.key)).toEqual(["account_password"]);
  });
  it("derives keys like the server and checks them", () => {
    expect(suggestKey("Player license")).toBe("player_license");
    expect(suggestKey("123 abc")).toBe("field_123_abc");
    expect(suggestKey("", "steamid")).toBe("steamid");
    expect(suggestKey("Package")).toBe("package_field");
    expect(keyValid("")).toBe(true);
    expect(keyValid("rank_2")).toBe(true);
    expect(keyValid("2rank")).toBe(false);
    expect(keyValid("server")).toBe(false);
    expect(fieldDraftValid({ ...EMPTY_FIELD, name: "x", key: "Bad Key" })).toBe(false);
  });
  it("moves fields up and down", () => {
    expect(moveField(["a", "b", "c"], "b", -1)).toEqual(["b", "a", "c"]);
    expect(moveField(["a", "b", "c"], "c", 1)).toEqual(["a", "b", "c"]);
  });
});

const of = (p: Partial<OrderField>): OrderField => ({ id: "f", name: "x", description: "", kind: "text", choices: [], pattern: "", required: false, ...p });
describe("checkout field inputs", () => {
  it("uses the right input type", () => {
    expect(inputProps(of({ kind: "email" })).type).toBe("email");
    expect(inputProps(of({ kind: "password" }))).toMatchObject({ type: "password", autoComplete: "new-password" });
    expect(inputProps(of({ field_key: "account_username" })).autoComplete).toBe("username");
  });
  it("validates e-mail, numbers and required fields", () => {
    expect(emailOk("a@b.co")).toBe(true);
    for (const bad of ["a", "a@b", "a b@c.de", "a@@b.co", "a@b..co"]) expect(emailOk(bad)).toBe(false);
    const fields = [of({ id: "e", kind: "email", required: true }), of({ id: "n", kind: "number" }), of({ id: "p", kind: "password", required: true })];
    expect(fieldErrors(fields, { e: "nope", n: "12a", p: "  " })).toEqual({ e: "email", n: "number", p: "required" });
    expect(fieldErrors(fields, { e: "me@x.io", n: "1.5", p: " s3cret " })).toEqual({});
  });
  it("never sends passwords to the dynamic checkout form", () => {
    const fields = [of({ id: "u" }), of({ id: "p", kind: "password" })];
    expect(selectedForConfig(fields, { u: "Niko", p: "hunter2" })).toEqual({ u: "Niko" });
  });
});
