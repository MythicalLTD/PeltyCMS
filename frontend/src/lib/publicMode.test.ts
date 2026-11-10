import { describe, expect, it } from "vitest";
import { applyVars, cheapest, connectText, resolveMode, themeVars, THEME_VARS } from "./publicMode";
import { browseQuery, stars } from "./market";
import { modeNav, supportLinks } from "@/layouts/SiteLayout";

const on = (...m: string[]) => new Set(m);
const t = (k: string) => k;

describe("resolveMode", () => {
  it("keeps the hosting landing with neither module", () => {
    expect(resolveMode("auto", on())).toBe("hosting");
    expect(resolveMode("store", on())).toBe("hosting");
  });
  it("auto prefers the store, then the marketplace", () => {
    expect(resolveMode("auto", on("gaming_storefront", "creator_marketplace"))).toBe("store");
    expect(resolveMode("auto", on("creator_marketplace"))).toBe("marketplace");
  });
  it("lets the admin pick when both are on, and force hosting", () => {
    expect(resolveMode("marketplace", on("gaming_storefront", "creator_marketplace"))).toBe("marketplace");
    expect(resolveMode("hosting", on("gaming_storefront"))).toBe("hosting");
    expect(resolveMode("marketplace", on("gaming_storefront"))).toBe("store");
  });
});

describe("helpers", () => {
  it("connect text", () => {
    expect(connectText("fivem", "rp.example.com:30120")).toBe("connect rp.example.com:30120");
    expect(connectText("minecraft_java", "play.example.com")).toBe("play.example.com");
    expect(connectText("fivem", "")).toBe("");
  });
  it("theme vars: preset plus accent; hosting untouched", () => {
    expect(themeVars("hosting", { theme: "fivem", accent: "default" }, () => undefined)).toEqual({});
    expect(themeVars("store", { theme: "fivem", accent: "default" }, () => undefined)["--primary"]).toBe(THEME_VARS.fivem["--primary"]);
    expect(themeVars("store", { theme: "minecraft", accent: "rose" }, () => ({ "--primary": "#f00" }))["--primary"]).toBe("#f00");
    expect(themeVars("marketplace", undefined, () => undefined)).toBe(THEME_VARS.marketplace);
  });
  it("applyVars restores what was there", () => {
    const props = new Map<string, string>([["--primary", "#123456"]]);
    const classes = new Set<string>();
    const root = {
      style: { getPropertyValue: (k: string) => props.get(k) ?? "", setProperty: (k: string, v: string) => void props.set(k, v), removeProperty: (k: string) => void props.delete(k) },
      classList: { contains: (c: string) => classes.has(c), toggle: (c: string, v: boolean) => void (v ? classes.add(c) : classes.delete(c)) },
    };
    const undo = applyVars(root, { "--primary": "#fff", "--card": "#000" });
    expect(props.get("--card")).toBe("#000");
    expect(classes.has("dark")).toBe(true);
    undo();
    expect(props.get("--primary")).toBe("#123456");
    expect(props.has("--card")).toBe(false);
    expect(classes.has("dark")).toBe(false);
  });
  it("cheapest price in a currency", () => {
    expect(cheapest([{ currency: "EUR", amount: "9.00" }, { currency: "EUR", amount: "4.50" }, { currency: "USD", amount: "1" }], "EUR")).toBe("4.5");
    expect(cheapest([], "EUR")).toBeNull();
  });
  it("marketplace query and stars", () => {
    expect(browseQuery({ q: "  phone ", category: "fivem", sort: "new", limit: 8 })).toBe("q=phone&category=fivem&sort=new&limit=8");
    expect(browseQuery({})).toBe("");
    expect(stars(4.26)).toBe(4.5);
    expect(stars(null)).toBe(0);
    expect(stars(9)).toBe(5);
  });
});

describe("mode nav", () => {
  it("store nav: vote/leaderboards only with community, Discord when set", () => {
    const hrefs = modeNav("store", t as never, { enabled: on("gaming_storefront"), discord: "https://discord.gg/x" }).map((l) => l.href);
    expect(hrefs).toEqual(["/store", "https://discord.gg/x", "/support"]);
    const full = modeNav("store", t as never, { enabled: on("gaming_storefront", "server_community", "creator_marketplace") }).map((l) => l.href);
    expect(full).toEqual(["/store", "/community#vote", "/community#leaderboards", "/marketplace", "/support"]);
  });
  it("marketplace nav links the store when it is on", () => {
    const hrefs = modeNav("marketplace", t as never, { enabled: on("creator_marketplace", "gaming_storefront") }).map((l) => l.href);
    expect(hrefs).toEqual(["/marketplace", "/creators", "/store", "/creator", "/support"]);
    expect(modeNav("hosting", t as never, { enabled: on() })).toEqual([]);
  });
  it("support links cover tickets, kb, status", () => {
    expect(supportLinks(t as never).map((l) => l.href)).toEqual(["/support", "/kb", "/tickets", "/status", "/announcements"]);
  });
});
