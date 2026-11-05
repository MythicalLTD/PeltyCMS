export type Health = "ok" | "late" | "failing" | "never";
export interface TaskStats { last_finished_at: string | null; last_status: string | null; last_error: string | null; last_result: unknown; next_run_at: string | null; running: number; succeeded_24h: number; failed_24h: number; dead_24h: number }
export interface AutomationTask { kind: string; interval_secs: number; health: Health; enabled: boolean; run_hour: number | null; stats: TaskStats }

/** Only daily tasks accept a preferred start hour (company time zone). */
export const isDaily = (secs: number) => secs >= 86400;
/** A scheduled addon job (`<addon-id>:<op>`); times are unix seconds. */
export interface AddonTask { name: string; source: string; interval_secs: number; enabled: boolean; health: Health; last_status: string | null; last_finished_at: number | null; next_run_at: number | null; last_error: string | null }
export interface Automation { tasks: AutomationTask[]; addon_tasks?: AddonTask[]; totals: Record<string, number> }

/** Interval as a translation key + count, e.g. 300 -> ["automation.every_min", 5]. */
export function intervalParts(secs: number): ["automation.every_day" | "automation.every_hour" | "automation.every_min", number] {
  if (secs % 86400 === 0) return ["automation.every_day", secs / 86400];
  if (secs % 3600 === 0) return ["automation.every_hour", secs / 3600];
  return ["automation.every_min", Math.max(1, Math.round(secs / 60))];
}

export const healthVariant = (h: Health) => (h === "failing" ? "destructive" : h === "ok" ? "default" : h === "late" ? "secondary" : "outline") as "destructive" | "default" | "secondary" | "outline";

/** Compact one-line summary of a job result object ("invoices: 2, deleted: 0"). */
export function resultSummary(r: unknown): string {
  if (!r || typeof r !== "object" || Array.isArray(r)) return "";
  return Object.entries(r as Record<string, unknown>).filter(([, v]) => ["number", "string", "boolean"].includes(typeof v)).map(([k, v]) => `${k}: ${String(v)}`).slice(0, 4).join(", ");
}

/** Human label for a job/task kind; falls back to the raw kind when unknown. */
export function taskLabel(tCode: (key: string) => string, kind: string): string {
  const key = `automation.task.${kind}`;
  const v = tCode(key);
  return v === key ? kind : v;
}

/** One scheduled task from `/admin/automation/tasks` (times are unix seconds). */
export interface SchedTask {
  name: string; source: string; interval_secs: number; enabled: boolean; run_hour: number | null;
  schedule: string | null; default_schedule: string | null; description: string | null; health: Health;
  last_status: string | null; last_finished_at: number | null; last_duration_ms: number | null; last_error: string | null; last_result: unknown;
  next_run_at: number | null; next_runs: number[];
  /** Circuit breaker: failed runs in a row, and when it disabled the task. */
  failures: number; tripped_at: number | null; tripped_error: string | null;
  /** Missed-run policy. */
  missed: "skip" | "run_once";
}
export interface TaskRun { id: number; status: string; attempts: number; created_at: number; finished_at: number | null; duration_ms: number | null; error: string | null; result: unknown }
export interface CronPreview { description: string; next_runs: number[]; timezone: string }

/** Unix seconds to an ISO string for the date formatters. */
export const iso = (t: number | null | undefined) => (t ? new Date(t * 1000).toISOString() : null);

/** "850 ms", "12.3 s", "4 min". */
export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "-";
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.round(ms / 60_000)} min`;
}

/** Rough client-side check before asking the server: five space-separated fields. */
export const looksLikeCron = (v: string) => v.trim().split(/\s+/).length === 5;
