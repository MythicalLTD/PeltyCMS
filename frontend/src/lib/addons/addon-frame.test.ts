import { describe, expect, it } from "vitest";
import { bridgeAllowed, clampHeight, hasId, navigateAllowed, pageSlots, parseFrameMessage, routeKey, themeVars } from "./addon-frame";

describe("route keys", () => {
  it("drops ids and the area prefix", () => {
    expect(routeKey("/admin/invoices/42", "admin")).toBe("invoices");
    expect(routeKey("/admin/settings/tax", "admin")).toBe("settings-tax");
    expect(routeKey("/admin", "admin")).toBe("dashboard");
    expect(routeKey("/services/0b6f3c3e-8f7e-4c1a-9d55-1c2b3a4d5e6f", "client")).toBe("services");
    expect(routeKey("/", "client")).toBe("dashboard");
    expect(routeKey("/tickets/TCK-12", "client")).toBe("tickets");
    expect(hasId("/admin/invoices/42")).toBe(true);
    expect(hasId("/admin/invoices")).toBe(false);
  });
  it("lists the specific slot before the global one", () => {
    expect(pageSlots("admin", "invoices", "toolbar")).toEqual(["admin.invoices.toolbar", "admin.*.toolbar"]);
  });
});

describe("frame bridge", () => {
  it("accepts only known message shapes", () => {
    expect(parseFrameMessage({ type: "pelty:resize", height: 300 })).toEqual({ type: "pelty:resize", height: 300 });
    expect(parseFrameMessage({ type: "pelty:request", id: 1, path: "/api/v1/addons/x/a", method: "post" })).toMatchObject({ method: "POST" });
    expect(parseFrameMessage({ type: "pelty:request", id: "1", path: "/x" })).toBeNull();
    expect(parseFrameMessage({ type: "pelty:request", id: 1, path: "/x", method: "TRACE" })).toBeNull();
    expect(parseFrameMessage({ type: "pelty:resize", height: Infinity })).toBeNull();
    expect(parseFrameMessage({ type: "eval", code: "x" })).toBeNull();
    expect(parseFrameMessage("pelty:ready")).toBeNull();
    expect(parseFrameMessage({ type: "pelty:toast", text: "x".repeat(999) })).toMatchObject({ tone: "success" });
  });
  it("scopes requests to the addon and approved GET prefixes", () => {
    const b = ["/api/v1/auth/me", "/api/v1/client/services"];
    expect(bridgeAllowed("x", b, "POST", "/api/v1/addons/x/notes")).toBe(true);
    expect(bridgeAllowed("x", b, "GET", "/api/v1/addons/y/notes")).toBe(false);
    expect(bridgeAllowed("x", b, "POST", "/api/v1/addons/x/ui-token")).toBe(false);
    expect(bridgeAllowed("x", b, "GET", "/api/v1/auth/me")).toBe(true);
    expect(bridgeAllowed("x", b, "GET", "/api/v1/client/services/12?x=1")).toBe(true);
    expect(bridgeAllowed("x", b, "GET", "/api/v1/client/servicesX")).toBe(false);
    expect(bridgeAllowed("x", b, "DELETE", "/api/v1/client/services/12")).toBe(false);
    expect(bridgeAllowed("x", b, "GET", "/api/v1/addons/x/../../admin/users")).toBe(false);
    expect(bridgeAllowed("x", b, "GET", "/api/v1/addons/x/%2e%2e/admin")).toBe(false);
    expect(bridgeAllowed("x", b, "GET", "https://evil/api/v1/addons/x/a")).toBe(false);
    expect(bridgeAllowed("x", [], "GET", "/api/v1/admin/users")).toBe(false);
  });
  it("keeps navigation inside the area", () => {
    expect(navigateAllowed("/admin/invoices", "admin")).toBe(true);
    expect(navigateAllowed("/services", "admin")).toBe(false);
    expect(navigateAllowed("//evil.example", "client")).toBe(false);
    expect(navigateAllowed("javascript:alert(1)", "client")).toBe(false);
    expect(navigateAllowed("/admin", "client")).toBe(false);
  });
  it("forwards theme tokens and clamps heights", () => {
    const vars = themeVars((n) => (n === "--primary" ? " oklch(0.5 0.2 260) " : ""));
    expect(vars).toEqual({ "--primary": "oklch(0.5 0.2 260)" });
    expect(clampHeight(5)).toBe(40);
    expect(clampHeight(99999)).toBe(4000);
  });
});

import { capabilityLines, wantsIframe } from "./addons";
import { hubPluginPages } from "@/pages/Hub";

describe("install review and hubs", () => {
  it("lists iframe and bridge powers", () => {
    const c = { http: [], secrets: [], kv: false, events: [], ui: ["iframe"], bridge: ["/api/v1/auth/me"] };
    expect(wantsIframe(c)).toBe(true);
    expect(capabilityLines(c)).toEqual([["addons.cap.ui_iframe", {}], ["addons.cap.bridge", { prefix: "/api/v1/auth/me" }]]);
    expect(wantsIframe({ http: [], secrets: [], kv: false, events: [] })).toBe(false);
  });
  it("puts addon pages in their hub (admin default: addons)", () => {
    const pages = [{ id: "a", area: "admin", hub: null }, { id: "b", area: "admin", hub: "billing" }, { id: "c", area: "client", hub: null }];
    expect(hubPluginPages(pages.filter((p) => p.area === "admin"), "admin", "addons").map((p) => p.id)).toEqual(["a"]);
    expect(hubPluginPages(pages.filter((p) => p.area === "admin"), "admin", "billing").map((p) => p.id)).toEqual(["b"]);
    expect(hubPluginPages(pages.filter((p) => p.area === "client"), "client", "billing")).toEqual([]);
  });
});
