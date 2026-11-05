export const PROJECT_STATUSES = ["planned", "active", "on_hold", "completed", "cancelled"] as const;
export type ProjectStatus = (typeof PROJECT_STATUSES)[number];

export interface ProjectSummary {
  id: string; customer_id: string | null; customer_email: string | null; title: string; status: ProjectStatus;
  due_date: string | null; client_visible: boolean; hourly_rate: string; tasks_total: number; tasks_done: number; minutes_total: number; created_at: string;
}
export interface ProjectTask { id: string; title: string; done: boolean; due_date: string | null; assignee: string | null }
export interface TimeEntry { id: string; task_id: string | null; minutes: number; note: string; billable: boolean; spent_on: string; invoice_id: string | null; user: string | null }
export interface ProjectLink { kind: "ticket" | "invoice"; target_id: string; label: string | null }
export interface ProjectDetail { project: ProjectSummary & { description: string }; tasks: ProjectTask[]; time: TimeEntry[]; links: ProjectLink[] }

/** "2h 05m" from minutes. */
export function duration(min: number): string {
  return `${Math.floor(min / 60)}h ${String(min % 60).padStart(2, "0")}m`;
}

/** Task progress in whole percent (0 when there are no tasks). */
export function progress(done: number, total: number): number {
  return total > 0 ? Math.round((done / total) * 100) : 0;
}

/** Parse "1:30", "90" or "1.5h" into minutes; null if invalid. */
export function parseMinutes(s: string): number | null {
  const v = s.trim().toLowerCase();
  let m: number;
  const hm = /^(\d{1,2}):([0-5]\d)$/.exec(v);
  if (hm) m = Number(hm[1]) * 60 + Number(hm[2]);
  else if (/^\d+(\.\d+)?h$/.test(v)) m = Math.round(Number(v.slice(0, -1)) * 60);
  else if (/^\d+$/.test(v)) m = Number(v);
  else return null;
  return m >= 1 && m <= 1440 ? m : null;
}

/** Unbilled billable minutes. */
export function unbilledMinutes(time: TimeEntry[]): number {
  return time.filter((e) => e.billable && !e.invoice_id).reduce((a, e) => a + e.minutes, 0);
}
