export type AddonKind = "provisioning" | "gateway" | "registrar" | "theme" | "plugin" | "captcha" | "sso" | "integration";
export type AddonStatus = "available" | "disabled" | "enabled";
export interface SettingSpec { key: string; label: string; kind: "text" | "number" | "bool" | "select" | "textarea" | "secret"; required: boolean; choices: string[]; default?: unknown }
export interface Addon {
  id: string; name: string; version: string; kind: AddonKind; engine: string; description: string; author: string;
  permissions: string[]; settings_schema: SettingSpec[]; source: "builtin" | "disk"; hash: string; status: AddonStatus;
  installed_version: string | null; integrity_ok: boolean;
  /** WASM addons: sandbox capabilities approved at install. */
  wasm?: boolean; provides?: string | null; abi?: string | null; capabilities?: AddonCapabilities | null;
}
export interface AddonHook { event: string; priority?: number; fail_open?: boolean; timeout_ms?: number }
export interface AddonRoute { method: string; path: string; access: "admin" | "client" | "public"; permission?: string | null }
export interface AddonMixin { target: string; at: "before" | "after" | "around" | "return"; priority?: number }
export interface AddonCapabilities { http: string[]; emits?: string[]; mixins?: AddonMixin[]; secrets: string[]; kv: boolean; events: string[]; hooks?: AddonHook[]; routes?: AddonRoute[]; pages?: number; ui?: string[]; bridge?: string[];
  /** Opt-in powers; every entry in `dangerous` needs its own approval at install. */
  db?: { read: string[]; write: string[]; sql: boolean } | null; exec?: string[]; filters?: Array<{ name: string; priority?: number }>; dangerous?: string[]; exec_blocked?: boolean; limits?: { memory_mib?: number; timeout_secs?: number } | null }

/** The addon ships its own HTML/JS pages (needs explicit approval at install). */
export const wantsIframe = (c: AddonCapabilities | null | undefined) => !!c?.ui?.includes("iframe");

/** Red-warning line for one dangerous capability: [i18n key, params]. */
export function dangerLine(cap: string, c: AddonCapabilities | null | undefined): [string, Record<string, string>] {
  const vars: Record<string, string> = {
    entities: (c?.db?.read ?? []).join(", "),
    ops: (c?.db?.write ?? []).join(", "),
    programs: (c?.exec ?? []).join(", "),
    filters: (c?.filters ?? []).map((f) => f.name).join(", "),
  };
  return [`addons.cap.danger.${cap.replace(/\./g, "_")}`, vars];
}

/** One line per capability for the permissions screen: [i18n key, params]. */
export function capabilityLines(c: AddonCapabilities | null | undefined): Array<[string, Record<string, string>]> {
  if (!c) return [];
  const out: Array<[string, Record<string, string>]> = [];
  for (const h of c.http) {
    if (h === "$server") out.push(["addons.cap.http_server", {}]);
    else if (h.startsWith("$setting:")) out.push(["addons.cap.http_setting", { key: h.slice(9) }]);
    else out.push(["addons.cap.http", { host: h }]);
  }
  for (const s of c.secrets) out.push(["addons.cap.secret", { key: s }]);
  if (c.kv) out.push(["addons.cap.kv", {}]);
  for (const e of c.events) out.push(["addons.cap.event", { event: e }]);
  for (const h of c.hooks ?? []) out.push([h.fail_open === false ? "addons.cap.hook_blocking" : "addons.cap.hook", { event: h.event }]);
  for (const e of c.emits ?? []) out.push(["addons.cap.emit", { event: e }]);
  for (const m of c.mixins ?? []) out.push(["addons.cap.mixin", { at: m.at, target: m.target }]);
  for (const r of c.routes ?? []) out.push([`addons.cap.route_${r.access}`, { route: `${r.method.toUpperCase()} ${r.path}`, permission: r.permission ?? "" }]);
  if (c.pages) out.push(["addons.cap.pages", { count: String(c.pages) }]);
  if (wantsIframe(c)) out.push(["addons.cap.ui_iframe", {}]);
  for (const b of c.bridge ?? []) out.push(["addons.cap.bridge", { prefix: b }]);
  return out;
}

export type AddonFilter = "all" | AddonKind | "installed" | "available";

export function filterAddons(items: Addon[], f: AddonFilter, q: string): Addon[] {
  const needle = q.trim().toLowerCase();
  return items.filter((a) => {
    if (f === "installed" && a.status === "available") return false;
    if (f === "available" && a.status !== "available") return false;
    if (f !== "all" && f !== "installed" && f !== "available" && a.kind !== f) return false;
    return !needle || a.name.toLowerCase().includes(needle) || a.id.includes(needle);
  });
}

/** Translation key for a built-in description, else the manifest text. */
export function describe(a: Addon, t: (k: string) => string): string {
  if (a.description.startsWith("addons.desc.")) {
    const v = t(a.description);
    return v === a.description ? t("addons.desc.generic") : v;
  }
  return a.description;
}

/** Initial form values from the schema (stored values win). */
export function initialValues(schema: SettingSpec[], values: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const s of schema) out[s.key] = values[s.key] ?? s.default ?? (s.kind === "bool" ? false : "");
  return out;
}
