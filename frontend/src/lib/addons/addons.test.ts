import { describe as d, expect, it } from "vitest";
import { capabilityLines, dangerLine, describe, filterAddons, initialValues, type Addon } from "./addons";

const a = (o: Partial<Addon>): Addon => ({ id: "x", name: "X", version: "1", kind: "gateway", engine: "x", description: "", author: "", permissions: [], settings_schema: [], source: "builtin", hash: "builtin", status: "available", installed_version: null, integrity_ok: true, ...o });

d("addons", () => {
  const items = [a({ id: "gateway-stripe", name: "Stripe", status: "enabled" }), a({ id: "provisioning-pterodactyl", name: "Pterodactyl", kind: "provisioning" })];
  it("filters by kind, status and query", () => {
    expect(filterAddons(items, "provisioning", "").map((x) => x.id)).toEqual(["provisioning-pterodactyl"]);
    expect(filterAddons(items, "installed", "").map((x) => x.id)).toEqual(["gateway-stripe"]);
    expect(filterAddons(items, "available", "").map((x) => x.id)).toEqual(["provisioning-pterodactyl"]);
    expect(filterAddons(items, "all", "STR").map((x) => x.id)).toEqual(["gateway-stripe"]);
  });
  it("describes built-ins via i18n with fallback", () => {
    const t = (k: string) => (k === "addons.desc.generic" ? "generic" : k);
    expect(describe(a({ description: "addons.desc.gateway.zzz" }), t)).toBe("generic");
    expect(describe(a({ description: "Plain" }), t)).toBe("Plain");
  });
  it("fills defaults", () => {
    expect(initialValues([{ key: "m", label: "", kind: "select", required: false, choices: ["a"], default: "a" }, { key: "b", label: "", kind: "bool", required: false, choices: [] }], {})).toEqual({ m: "a", b: false });
  });
});

d("capabilityLines extension points", () => {
  it("lists hooks, routes and pages", () => {
    const l = capabilityLines({ http: [], secrets: [], kv: false, events: [], pages: 3,
      hooks: [{ event: "login.before" }, { event: "order.before_place", fail_open: false }],
      routes: [{ method: "get", path: "/stats", access: "admin", permission: "reports.read" }, { method: "POST", path: "/notes", access: "client" }] });
    expect(l).toEqual([
      ["addons.cap.hook", { event: "login.before" }],
      ["addons.cap.hook_blocking", { event: "order.before_place" }],
      ["addons.cap.route_admin", { route: "GET /stats", permission: "reports.read" }],
      ["addons.cap.route_client", { route: "POST /notes", permission: "" }],
      ["addons.cap.pages", { count: "3" }],
    ]);
  });
});

d("capabilityLines", () => {
  it("lists hosts, placeholders, secrets, storage and events", () => {
    const l = capabilityLines({ http: ["api.stripe.com", "$server", "$setting:verify_url"], secrets: ["api_key"], kv: true, events: ["invoice.*"] });
    expect(l).toEqual([
      ["addons.cap.http", { host: "api.stripe.com" }],
      ["addons.cap.http_server", {}],
      ["addons.cap.http_setting", { key: "verify_url" }],
      ["addons.cap.secret", { key: "api_key" }],
      ["addons.cap.kv", {}],
      ["addons.cap.event", { event: "invoice.*" }],
    ]);
    expect(capabilityLines(null)).toEqual([]);
  });
});

d("dangerLine", () => {
  it("maps a dangerous capability to its red-warning key with scope", () => {
    const c = { http: [], secrets: [], kv: false, events: [], db: { read: ["clients"], write: ["client.note"], sql: true }, exec: ["/bin/echo"] };
    expect(dangerLine("db.read", c)).toEqual(["addons.cap.danger.db_read", { entities: "clients", ops: "client.note", programs: "/bin/echo", filters: "" }]);
    expect(dangerLine("ui.iframe.same_origin", c)[0]).toBe("addons.cap.danger.ui_iframe_same_origin");
  });
});
