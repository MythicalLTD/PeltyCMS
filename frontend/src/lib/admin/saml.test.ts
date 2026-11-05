import { describe, expect, it } from "vitest";
import { mapToText, parseMap, splitCerts } from "./saml";
import { samlToSso, ssoHref } from "@/components/account/SsoButtons";

describe("saml helpers", () => {
  it("parses group maps and round-trips", () => {
    const m = parseMap("vip = 1111\n\nbad line\n=x\nadmins=admin\n");
    expect(m).toEqual({ vip: "1111", admins: "admin" });
    expect(parseMap(mapToText(m))).toEqual(m);
    expect(mapToText({ a: "b", n: 3 })).toBe("a = b");
  });
  it("splits certificates", () => {
    const pem = "-----BEGIN CERTIFICATE-----\nAAA\n-----END CERTIFICATE-----";
    expect(splitCerts(`${pem}\n${pem}`)).toHaveLength(2);
    expect(splitCerts(" MIIB ")).toEqual(["MIIB"]);
    expect(splitCerts("")).toEqual([]);
  });
  it("maps SAML providers to SSO buttons with identity ids", () => {
    const p = samlToSso([{ id: "u1", label: "Corp", url: "/api/v1/sso/saml/u1/login" }])[0]!;
    expect(p).toMatchObject({ id: "saml:u1", engine: "saml", label: "Corp" });
    expect(ssoHref(p, "link")).toBe("/api/v1/sso/saml/u1/login?mode=link");
    expect(ssoHref({ id: "sso-github", engine: "github", label: "GH" }, "login")).toBe("/api/v1/auth/oauth/sso-github/start?mode=login");
  });
});
