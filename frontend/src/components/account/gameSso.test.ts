import { describe, expect, it } from "vitest";
import { orderProviders, ssoHref, SSO_ICON, type SsoProvider } from "@/components/account/SsoButtons";
import { signupEmailOk } from "@/components/account/SsoSignup";

const p = (id: string, engine: string, featured = false): SsoProvider => ({ id, engine, label: id, featured });

describe("game sign-in buttons", () => {
  it("puts FiveM / Steam first, keeps the rest in order", () => {
    const list = [p("sso-discord", "discord"), p("sso-steam", "steam", true), p("sso-microsoft", "microsoft"), p("sso-cfx", "cfx", true)];
    expect(orderProviders(list).map((x) => x.id)).toEqual(["sso-steam", "sso-cfx", "sso-discord", "sso-microsoft"]);
  });

  it("has icons for the game engines", () => {
    expect(SSO_ICON.cfx).toBeDefined();
    expect(SSO_ICON.steam).toBeDefined();
  });

  it("passes only same-site return paths", () => {
    expect(ssoHref(p("sso-cfx", "cfx"), "login", "/checkout")).toBe("/api/v1/auth/oauth/sso-cfx/start?mode=login&return_to=%2Fcheckout");
    expect(ssoHref(p("sso-cfx", "cfx"), "login", "//evil.test")).toBe("/api/v1/auth/oauth/sso-cfx/start?mode=login");
    expect(ssoHref(p("sso-cfx", "cfx"), "login", "https://evil.test")).toBe("/api/v1/auth/oauth/sso-cfx/start?mode=login");
    expect(ssoHref(p("sso-steam", "steam"), "link")).toBe("/api/v1/auth/oauth/sso-steam/start?mode=link");
  });

  it("checks the sign-up e-mail shape", () => {
    expect(signupEmailOk("player@example.com")).toBe(true);
    expect(signupEmailOk(" player@example.com ")).toBe(true);
    expect(signupEmailOk("player")).toBe(false);
    expect(signupEmailOk("a@b")).toBe(false);
  });
});
