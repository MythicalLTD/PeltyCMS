/**
 * Addon UI everywhere: route keys for the generic page slots and the pure
 * rules of the sandboxed iframe bridge (see components/plugin-ui/AddonFrame.tsx
 * and the generated docs, "UI" page).
 *
 * Slot names: `<area>.<route>.<position>` (route = routeKey() or `*`) and
 * `<area>.layout.<header|sidebar|footer|banner>`.
 */
import { HOST_FILTERS, type HostFilters } from "@/lib/addons/addon-events";
export type Area = "admin" | "client";
export type PagePosition = "before" | "after" | "toolbar" | "row-actions" | "tabs" | "detail-panel" | `replace:${string}` | `before:${string}` | `after:${string}`;
export type LayoutPosition = "header" | "sidebar" | "footer" | "banner";

const ID = /^(\d+|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[0-9a-f]{16,}|[A-Z0-9]{2,}-\d+)$/i;

/** Route key of a pathname: segments without ids, joined with `-`.
 *  `/admin/invoices/42` → `invoices`, `/admin/settings/tax` → `settings-tax`, `/admin` → `dashboard`. */
export function routeKey(pathname: string, area: Area): string {
  let segs = pathname.split("/").filter(Boolean);
  if (area === "admin" && segs[0] === "admin") segs = segs.slice(1);
  const keep = segs.filter((s) => !ID.test(s)).map((s) => s.toLowerCase().replace(/[^a-z0-9-]/g, "-")).filter(Boolean);
  return keep.length ? keep.join("-").slice(0, 64) : "dashboard";
}

/** Has the path an id segment (detail pages get `detail-panel`)? */
export const hasId = (pathname: string) => pathname.split("/").some((s) => s !== "" && ID.test(s));

/** Slot names to render for one page position (specific first, then `*`). */
export function pageSlots(area: Area, key: string, pos: PagePosition): string[] {
  return [`${area}.${key}.${pos}`, `${area}.*.${pos}`];
}

export const layoutSlot = (area: Area, pos: LayoutPosition) => `${area}.layout.${pos}`;

// ----- iframe bridge -----

/** Messages a frame may send. Anything else is ignored. */
export type FrameMessage =
  | { type: "pelty:ready" }
  | { type: "pelty:resize"; height: number }
  | { type: "pelty:request"; id: number; path: string; method: string; body?: unknown }
  | { type: "pelty:navigate"; to: string }
  | { type: "pelty:toast"; text: string; tone?: "success" | "error" }
  | { type: "pelty:subscribe"; events: string[] }
  | { type: "pelty:emit"; name: string; payload?: unknown }
  | { type: "pelty:filter_register"; name: keyof HostFilters; priority: number }
  | { type: "pelty:filter_result"; id: number; value: unknown };

/** Event names: `a-z0-9._:-`, max 96 chars. */
export const EVENT_NAME = /^[a-z0-9][a-z0-9._:*-]{0,95}$/;

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE"];

/** Validate an untrusted message (structure only). */
export function parseFrameMessage(m: unknown): FrameMessage | null {
  if (!m || typeof m !== "object") return null;
  const o = m as Record<string, unknown>;
  switch (o.type) {
    case "pelty:ready": return { type: "pelty:ready" };
    case "pelty:resize": return typeof o.height === "number" && Number.isFinite(o.height) ? { type: "pelty:resize", height: o.height } : null;
    case "pelty:request": {
      const method = typeof o.method === "string" ? o.method.toUpperCase() : "GET";
      if (typeof o.id !== "number" || typeof o.path !== "string" || !METHODS.includes(method)) return null;
      return { type: "pelty:request", id: o.id, path: o.path, method, body: o.body };
    }
    case "pelty:navigate": return typeof o.to === "string" ? { type: "pelty:navigate", to: o.to } : null;
    case "pelty:toast": return typeof o.text === "string" ? { type: "pelty:toast", text: o.text.slice(0, 300), tone: o.tone === "error" ? "error" : "success" } : null;
    case "pelty:subscribe": {
      if (!Array.isArray(o.events)) return null;
      const events = o.events.filter((e): e is string => typeof e === "string" && EVENT_NAME.test(e)).slice(0, 50);
      return { type: "pelty:subscribe", events };
    }
    case "pelty:filter_register": {
      if (typeof o.name !== "string" || !(HOST_FILTERS as string[]).includes(o.name)) return null;
      const priority = typeof o.priority === "number" && Number.isFinite(o.priority) ? Math.max(-1000, Math.min(1000, Math.round(o.priority))) : 0;
      return { type: "pelty:filter_register", name: o.name as keyof HostFilters, priority };
    }
    case "pelty:filter_result": return typeof o.id === "number" ? { type: "pelty:filter_result", id: o.id, value: o.value } : null;
    case "pelty:emit": return typeof o.name === "string" && EVENT_NAME.test(o.name) && !o.name.includes("*") ? { type: "pelty:emit", name: o.name, payload: o.payload } : null;
    default: return null;
  }
}

function cleanPath(p: string): string | null {
  if (!p.startsWith("/api/v1/") || p.includes("\\") || p.length > 512) return null;
  const [path] = p.split("?");
  if (!path || path.split("/").some((s) => s === ".." || s === "." || /%2e|%2f|%5c/i.test(s))) return null;
  return p;
}

/** May this frame call `path`? Own routes: any method. Core `bridge`
 *  prefixes (manifest, approved at install): GET only. */
export function bridgeAllowed(plugin: string, bridge: string[], method: string, path: string): boolean {
  const p = cleanPath(path);
  if (!p) return false;
  if (p.startsWith(`/api/v1/addons/${plugin}/`) && !p.startsWith(`/api/v1/addons/${plugin}/ui-token`) && !p.startsWith(`/api/v1/addons/${plugin}/assets/`)) return true;
  if (method !== "GET") return false;
  const path0 = p.split("?")[0]!;
  return bridge.some((b) => path0 === b || path0.startsWith(b.endsWith("/") ? b : `${b}/`));
}

/** In-app navigation targets a frame may ask for (same area, no protocol). */
export function navigateAllowed(to: string, area: Area): boolean {
  if (!to.startsWith("/") || to.startsWith("//") || to.includes("\\") || to.split("?")[0]!.split("/").some((s) => s === "..")) return false;
  return area === "admin" ? to.startsWith("/admin") : !to.startsWith("/admin");
}

/** Theme tokens forwarded into frames (values from the live document). */
export const THEME_VARS = [
  "--background", "--foreground", "--card", "--card-foreground", "--popover", "--popover-foreground", "--primary", "--primary-foreground",
  "--secondary", "--secondary-foreground", "--muted", "--muted-foreground", "--accent", "--accent-foreground", "--destructive",
  "--border", "--input", "--ring", "--radius", "--font-sans", "--font-mono", "--chart-1", "--chart-2", "--chart-3", "--chart-4", "--chart-5",
];

export function themeVars(get: (name: string) => string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const v of THEME_VARS) {
    const x = get(v).trim();
    if (x) out[v] = x;
  }
  return out;
}

/** Clamp a requested frame height. */
export const clampHeight = (h: number) => Math.max(40, Math.min(4000, Math.round(h)));
