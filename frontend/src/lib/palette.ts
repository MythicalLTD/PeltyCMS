import type { NavScope } from "@/lib/navRegistry";
import { searchHref, type SearchGroup } from "@/lib/admin/adminTools";

export interface Recent { href: string; title: string }

const KEY = (s: NavScope) => `pelty.palette.recent.${s}`;
const MAX = 6;

export function readRecents(scope: NavScope, store: Pick<Storage, "getItem"> = localStorage): Recent[] {
  try {
    const v = JSON.parse(store.getItem(KEY(scope)) ?? "[]") as unknown;
    return Array.isArray(v) ? v.filter((x): x is Recent => typeof x?.href === "string" && typeof x?.title === "string").slice(0, MAX) : [];
  } catch { return []; }
}

export function pushRecent(scope: NavScope, r: Recent, store: Pick<Storage, "getItem" | "setItem"> = localStorage): Recent[] {
  const next = [r, ...readRecents(scope, store).filter((x) => x.href !== r.href)].slice(0, MAX);
  try { store.setItem(KEY(scope), JSON.stringify(next)); } catch { /* private mode */ }
  return next;
}

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** Diacritic-insensitive page match on title, category and description. */
export function matchPages<T extends { title: string; group: string; hint: string }>(pages: T[], q: string, limit = 8): T[] {
  const n = fold(q.trim());
  if (!n) return [];
  const score = (p: T) => (fold(p.title).startsWith(n) ? 0 : fold(p.title).includes(n) ? 1 : fold(p.group).includes(n) ? 2 : fold(p.hint).includes(n) ? 3 : 9);
  return pages.map((p) => [score(p), p] as const).filter(([s]) => s < 9).sort((a, b) => a[0] - b[0]).slice(0, limit).map(([, p]) => p);
}

/** Quick links for an empty query: primary pages not already in recents. */
export function suggestedPages<T extends { to: string; primary?: boolean }>(pages: T[], exclude: string[], limit = 6): T[] {
  return pages.filter((p) => p.primary && !exclude.includes(p.to)).slice(0, limit);
}

export const resultGroups = (scope: NavScope): string[] =>
  scope === "admin" ? ["customers", "invoices", "services", "tickets", "domains", "orders"] : ["invoices", "services", "tickets", "domains", "kb"];

export function hitLabel(g: string, h: Record<string, unknown>): string {
  const s = (k: string) => (h[k] == null ? "" : String(h[k]));
  switch (g) {
    case "customers": return `${s("name") || s("organization")} · ${s("email")}`;
    case "invoices": return `#${s("number") || "draft"} · ${s("total")} ${s("currency")}${h.customer ? ` · ${s("customer")}` : ""}`;
    case "services": return h.customer ? `${s("product")} · ${s("customer")}` : s("product");
    case "tickets": return `#${s("number")} · ${s("subject")}`;
    case "domains": return s("name");
    case "kb": return s("title");
    default: return `#${s("number")} · ${s("total")} ${s("currency")}`;
  }
}

export function hitHref(scope: NavScope, g: string, h: Record<string, unknown>): string {
  const id = String(h.id);
  if (scope === "admin") return searchHref(g as SearchGroup, { id, email: h.email as string | undefined });
  switch (g) {
    case "invoices": return `/invoices/${id}`;
    case "services": return `/services?q=${encodeURIComponent(id)}`;
    case "tickets": return `/tickets/${id}`;
    case "domains": return `/domains/${id}`;
    case "kb": return `/kb/${encodeURIComponent(String(h.slug))}`;
    default: return "/dashboard";
  }
}
