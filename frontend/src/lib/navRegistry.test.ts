import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import en from "../../../locales/en.json";
import ro from "../../../locales/ro.json";
import { flatNav, hubLeaves, hubPath, navTree, type NavScope } from "./navRegistry";
import { matchPages, pushRecent, readRecents, hitHref } from "./palette";

const app = readFileSync(resolve(__dirname, "../App.tsx"), "utf8");
const paths = new Set([...app.matchAll(/path="([^"]+)"/g)].map((m) => m[1]));
const routed = (to: string) => (to === "/admin" ? app.includes("<Route index element={<AdminDashboardPage />}") : to.startsWith("/admin/") ? paths.has(to.slice(7)) : paths.has(to));

describe.each<NavScope>(["admin", "client"])("nav registry (%s)", (scope) => {
  const leaves = flatNav(scope);
  it("every route exists in the router", () => {
    for (const l of leaves) expect(routed(l.to)).toBe(true);
    expect(paths.has(scope === "admin" ? "hub/:id" : "/hub/:id")).toBe(true);
    expect(hubPath(scope, "x")).toBe(scope === "admin" ? "/admin/hub/x" : "/hub/x");
  });
  it("links SAML and OAuth clients from the security hub", () => {
    const sec = navTree("admin").find((c) => c.id === "security")!;
    const to = hubLeaves("admin", sec).map((l) => l.to);
    expect(to).toContain("/admin/saml");
    expect(to).toContain("/admin/oauth-clients");
    expect(flatNav("admin").find((l) => l.to === "/admin/saml")?.category.id).toBe("addons");
  });
  it("has unique routes", () => {
    expect(new Set(leaves.map((l) => l.to)).size).toBe(leaves.length);
  });
  it("every i18n key exists in en and ro", () => {
    const keys = [...navTree(scope).map((c) => c.label), ...leaves.flatMap((l) => [l.label, l.desc])];
    for (const d of [en, ro] as Record<string, string>[]) for (const k of keys) expect(d[k]).toBeTruthy();
  });
  it("stays compact", () => {
    expect(navTree(scope).length).toBeLessThanOrEqual(scope === "admin" ? 11 : 7);
    const sidebar = navTree(scope).reduce((n, c) => n + (c.items.length === 1 ? 1 : c.items.filter((i) => i.primary).length), 0);
    expect(sidebar).toBeLessThanOrEqual(scope === "admin" ? 30 : 21);
  });
});

describe("client nav includes orders", () => {
  it("lists /orders under services", () => {
    const leaf = flatNav("client").find((l) => l.to === "/orders");
    expect(leaf?.category.id).toBe("services");
    expect(routed("/orders")).toBe(true);
  });
});

describe("client nav includes ssl", () => {
  it("lists /ssl under domains", () => {
    const leaf = flatNav("client").find((l) => l.to === "/ssl");
    expect(leaf?.category.id).toBe("domains");
    expect(routed("/ssl")).toBe(true);
  });
});

describe("admin nav covers every former sidebar route", () => {
  it("keeps all /admin routes reachable from the registry", () => {
    const old = ["users", "staff", "campaigns", "customers", "client-groups", "products", "orders", "invoices", "billing-tools", "gift-cards", "transactions", "domains", "billable-items", "bundles", "usage-metrics", "currencies", "legal-entities", "tickets", "live-chat", "coupons", "order-forms", "quotes", "affiliates", "downloads", "servers", "module-queue", "module-log", "cancellations", "services", "jobs", "audit", "website", "extensions", "themes", "gateways", "tax", "overdue", "email-templates", "content", "sessions", "general-settings", "mail-settings", "reports", "gdpr", "fraud", "security", "oauth-clients", "api-keys", "network-status", "system-health", "my-notes", "calendar", "login-history", "backups", "csv-import", "notifications", "todos", "client-fields"];
    const have = new Set(flatNav("admin").map((l) => l.to));
    for (const o of old) expect(have.has(`/admin/${o}`)).toBe(true);
  });
});

describe("palette helpers", () => {
  it("matches pages diacritic-insensitively, title first", () => {
    const pages = [{ title: "Facturi", group: "Facturare", hint: "x" }, { title: "Setări", group: "Sistem", hint: "facturi vechi" }];
    expect(matchPages(pages, "setari").map((p) => p.title)).toEqual(["Setări"]);
    expect(matchPages(pages, "fact").map((p) => p.title)).toEqual(["Facturi", "Setări"]);
    expect(matchPages(pages, " ")).toEqual([]);
  });
  it("keeps a bounded, de-duplicated recent list", () => {
    const m = new Map<string, string>();
    const store = { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
    for (let i = 0; i < 10; i++) pushRecent("client", { href: `/x${i}`, title: `${i}` }, store);
    pushRecent("client", { href: "/x5", title: "5" }, store);
    const r = readRecents("client", store);
    expect(r).toHaveLength(6);
    expect(r[0]?.href).toBe("/x5");
    expect(new Set(r.map((x) => x.href)).size).toBe(6);
    expect(readRecents("admin", store)).toEqual([]);
  });
  it("routes client hits to client pages only", () => {
    expect(hitHref("client", "invoices", { id: "a" })).toBe("/invoices/a");
    expect(hitHref("client", "kb", { id: "a", slug: "dns" })).toBe("/kb/dns");
    expect(hitHref("admin", "invoices", { id: "a" })).toBe("/admin/invoices/a");
  });
});
