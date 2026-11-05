export const WIDGETS = ["stats", "revenue", "activity", "staff_online", "quick_links", "todos", "calendar", "health"] as const;
export type Widget = (typeof WIDGETS)[number];
export interface QuickLink { label: string; url: string }
export interface Prefs { widgets: Widget[]; quick_links: QuickLink[] }
export interface CalendarItem { kind: "invoice" | "domain" | "service" | "todo"; id: string; date: string; title: string; customer_id: string | null }
export interface LoginEntry { id: string; user_id: string; email: string; display_name: string; ip: string | null; user_agent: string | null; created_at: string; last_seen_at: string; state: "active" | "expired" | "revoked" }
export interface OnlineStaff { user_id: string; email: string; display_name: string; last_seen_at: string }
export interface Backup { id: string; file_name: string; size_bytes: number | null; status: "queued" | "done" | "failed"; error: string | null; created_at: string }
export interface EmailLogEntry { id: number; template: string; locale: string; status: string; attempts: number; last_error: string | null; created_at: string; finished_at: string | null }

/** Move an item one step up (-1) or down (+1); out-of-range moves are no-ops. */
export function move<T>(list: T[], i: number, dir: -1 | 1): T[] {
  const j = i + dir;
  if (i < 0 || j < 0 || i >= list.length || j >= list.length) return list;
  const out = list.slice();
  [out[i], out[j]] = [out[j]!, out[i]!];
  return out;
}

/** Toggle a widget on (appended) or off. */
export function toggle(list: Widget[], w: Widget): Widget[] {
  return list.includes(w) ? list.filter((x) => x !== w) : [...list, w];
}

/** Same rule as the server: internal path or https URL. */
export function linkOk(url: string): boolean {
  const u = url.trim();
  return (u.startsWith("/") && !u.startsWith("//")) || u.startsWith("https://");
}

/** Month grid (Monday first): leading blanks as null, then ISO dates. */
export function monthGrid(month: string): (string | null)[] {
  const [y, m] = month.split("-").map(Number) as [number, number];
  const first = new Date(Date.UTC(y, m - 1, 1));
  const days = new Date(Date.UTC(y, m, 0)).getUTCDate();
  const lead = (first.getUTCDay() + 6) % 7;
  const out: (string | null)[] = Array.from({ length: lead }, () => null);
  for (let d = 1; d <= days; d++) out.push(`${month}-${String(d).padStart(2, "0")}`);
  return out;
}

export function shiftMonth(month: string, by: number): string {
  const [y, m] = month.split("-").map(Number) as [number, number];
  const d = new Date(Date.UTC(y, m - 1 + by, 1));
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
}

export function formatBytes(n: number | null): string {
  if (n === null) return "-";
  const u = ["B", "KB", "MB", "GB"];
  let v = n, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${i === 0 ? v : v.toFixed(1)} ${u[i]}`;
}
