/**
 * Declarative plugin UI: types + pure binding helpers. Node trees come from
 * plugin manifests (validated server-side, see crates/pelty-extensions/src/ui_schema.rs)
 * and are rendered natively by components/plugin-ui/PluginRenderer.tsx.
 *
 * Bindings: a string `"{{expr}}"` resolves to the raw value (arrays/objects
 * kept); `"a {{expr}} b"` interpolates. Expressions:
 *   data.<source>.<path>   ctx.<key>   params.<key>   row.<path>   item.<path>   index
 *   t:<translation.key>    count <path>    sum <path> <field>    not <path>
 *   <path> == <literal>    <path> != <literal>
 * Paths support `.length` and numeric indexes.
 */
export type UiNode = { type: string; children?: UiNode[]; [k: string]: unknown };
export interface DataSource { get: string; refresh_secs?: number | null }
/** Sandboxed iframe (WASM addons with `ui.iframe`); `src` is the asset URL. */
export interface UiFrame { src: string; height?: number | null }
export interface PluginPage { plugin: string; id: string; area: "admin" | "client"; title: string; icon?: string | null; nav: boolean; hub?: string | null; data: Record<string, DataSource>; body: UiNode | null; frame?: UiFrame | null; bridge?: string[]; same_origin?: boolean }
export interface PluginSlotUi { plugin: string; slot: string; data: Record<string, DataSource>; body: UiNode | null; frame?: UiFrame | null; bridge?: string[]; same_origin?: boolean; order?: number }
export type Scope = Record<string, unknown>;
export type Translate = (key: string) => string;

const WHOLE = /^\{\{\s*([^}]+?)\s*\}\}$/;
const ANY = /\{\{\s*([^}]+?)\s*\}\}/g;

export function lookup(scope: Scope, path: string): unknown {
  let cur: unknown = scope;
  for (const part of path.split(".")) {
    if (part === "") continue;
    if (cur === null || cur === undefined) return undefined;
    if (part === "length" && (Array.isArray(cur) || typeof cur === "string")) { cur = cur.length; continue; }
    if (typeof cur !== "object") return undefined;
    // Own properties only: no prototype walking.
    if (!Object.prototype.hasOwnProperty.call(cur, part)) return undefined;
    cur = (cur as Record<string, unknown>)[part];
  }
  return cur;
}

function literal(s: string): unknown {
  const v = s.trim();
  if (v === "true") return true;
  if (v === "false") return false;
  if (v === "null") return null;
  if (/^-?\d+(\.\d+)?$/.test(v)) return Number(v);
  return v.replace(/^['"]|['"]$/g, "");
}

export function evaluate(expr: string, scope: Scope, t: Translate): unknown {
  const e = expr.trim();
  if (e.startsWith("t:")) return t(e.slice(2).trim());
  const eq = e.match(/^(\S+)\s*(==|!=)\s*(.+)$/);
  if (eq) {
    const same = String(lookup(scope, eq[1]!) ?? "") === String(literal(eq[3]!) ?? "");
    return eq[2] === "==" ? same : !same;
  }
  const [op, a, b] = e.split(/\s+/);
  if (op === "not" && a) return !truthy(lookup(scope, a));
  if (op === "count" && a) { const v = lookup(scope, a); return Array.isArray(v) ? v.length : 0; }
  if (op === "sum" && a) {
    const v = lookup(scope, a);
    if (!Array.isArray(v)) return 0;
    return v.reduce((acc: number, x) => acc + (Number(b ? lookup(x as Scope, b) : x) || 0), 0);
  }
  return lookup(scope, e);
}

export function truthy(v: unknown): boolean {
  if (Array.isArray(v)) return v.length > 0;
  return v !== undefined && v !== null && v !== false && v !== "" && v !== 0;
}

/** Resolve one prop value (deep for objects/arrays of plain values). */
export function resolve(v: unknown, scope: Scope, t: Translate): unknown {
  if (typeof v === "string") {
    const whole = v.match(WHOLE);
    if (whole) return evaluate(whole[1]!, scope, t);
    if (!v.includes("{{")) return v;
    return v.replace(ANY, (_, ex: string) => display(evaluate(ex, scope, t)));
  }
  if (Array.isArray(v)) return v.map((x) => resolve(x, scope, t));
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, resolve(x, scope, t)]));
  return v;
}

export function display(v: unknown): string {
  if (v === undefined || v === null) return "";
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

/** Text for label-like props: translation keys (`plugin.*`, `common.*`…) are translated, bindings resolved. */
export function text(v: unknown, scope: Scope, t: Translate): string {
  if (typeof v !== "string") return display(resolve(v, scope, t));
  if (/^[a-z][a-z0-9_-]*(\.[a-z0-9_-]+)+$/.test(v) && !v.includes("{{")) return t(v);
  return display(resolve(v, scope, t));
}

/** Fill `{{…}}` in a path with URL-encoded values. Returns null if unsafe or a value is missing. */
export function fillPath(path: string, scope: Scope, t: Translate): string | null {
  let missing = false;
  const out = path.replace(ANY, (_, ex: string) => {
    const v = evaluate(ex, scope, t);
    if (v === undefined || v === null || v === "") { missing = true; return ""; }
    return encodeURIComponent(display(v));
  });
  if (missing || !out.startsWith("/") || out.startsWith("//") || (out.split("?")[0] ?? "").split("/").some((s) => s === ".." || s === ".")) return null;
  return out;
}

/** `/api/v1/x` → `/x` for the `api()` helper (which prefixes /api/v1). */
export function apiRel(path: string): string | null {
  return path.startsWith("/api/v1/") ? path.slice("/api/v1".length) : null;
}
