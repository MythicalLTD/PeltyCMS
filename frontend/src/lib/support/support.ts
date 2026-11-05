/** Support desk types and pure helpers (tested in support.test.ts). */
export interface Ticket {
  id: string; number: number; customer_id: string; customer_name: string; customer_email: string;
  department_id: string; department_name: string; service_id: string | null; assigned_to: string | null;
  subject: string; status: string; priority: string; created_at: string; last_reply_at: string; closed_at: string | null;
  first_response_at: string | null; rating: number | null; rating_comment: string; merged_into: string | null;
}
export interface Attachment { id: string; filename: string; size_bytes: number }
export interface Message {
  id: string; author_name: string | null; is_staff: boolean; is_internal: boolean; body: string; created_at: string; edited_at?: string | null; attachments: Attachment[];
}
export interface TicketDetail { ticket: Ticket; messages: Message[]; tags: string[] }
export interface Department { id: string; name: string; description: string; is_active: boolean; sort_order: number }
export interface Canned { id: string; title: string; body: string; department_id: string | null }
export interface Staff { id: string; email: string; display_name: string }
export interface Rule {
  id: string; name: string; department_id: string | null; statuses: string[]; priority: string | null; after_minutes: number;
  set_priority: string | null; set_department: string | null; assign_to: string | null; add_tag: string | null; is_active: boolean;
}
export interface SlaRow {
  department_id: string; department_name: string; sla_first_response_minutes: number | null; tickets: number; responded: number;
  avg_first_response_minutes: number | null; within_sla: number | null; breached_open: number | null; ratings: number; avg_rating: string | null;
}

export const STATUSES = ["open", "answered", "customer_reply", "on_hold", "closed"] as const;
export const PRIORITIES = ["low", "medium", "high", "urgent"] as const;

/** Same normalisation as the server: lowercase, spaces → dashes, dedupe, drop invalid. */
export function parseTags(input: string): string[] {
  const out = new Set<string>();
  for (const raw of input.split(",")) {
    const t = raw.trim().toLowerCase().replace(/ /g, "-");
    if (/^[a-z0-9][a-z0-9_-]{0,39}$/.test(t)) out.add(t);
  }
  return [...out].toSorted();
}

/** Percentage of responded tickets within SLA, or null when no SLA / no data. */
export function slaPercent(r: Pick<SlaRow, "within_sla" | "responded">): number | null {
  if (r.within_sla == null || r.responded === 0) return null;
  return Math.round((r.within_sla / r.responded) * 100);
}

/** Minutes as a compact "1h 5m" / "45m" / "2d 3h" string. */
export function minutes(m: number | null): string {
  if (m == null) return "-";
  if (m < 60) return `${m}m`;
  if (m < 1440) return `${Math.floor(m / 60)}h${m % 60 ? ` ${m % 60}m` : ""}`;
  const h = Math.floor((m % 1440) / 60);
  return `${Math.floor(m / 1440)}d${h ? ` ${h}h` : ""}`;
}

export function priorityVariant(p: string): "destructive" | "default" | "secondary" | "outline" {
  return p === "urgent" ? "destructive" : p === "high" ? "default" : p === "low" ? "outline" : "secondary";
}

/** Ratable by the client: answered at least once, not merged, not yet rated. */
export function canRate(t: Pick<Ticket, "first_response_at" | "merged_into" | "rating">): boolean {
  return t.first_response_at != null && t.merged_into == null && t.rating == null;
}

export type BadgeVariant = "default" | "secondary" | "destructive" | "outline";
export interface TicketStatusRow { slug: string; name: string; color: BadgeVariant; sort_order: number; awaiting_staff: boolean; builtin: boolean }

/** Custom name wins; built-ins fall back to their translation, unknown slugs to the slug. */
export function statusLabel(row: TicketStatusRow | undefined, slug: string, translate: (code: string) => string): string {
  if (row?.name) return row.name;
  if (row?.builtin || (STATUSES as readonly string[]).includes(slug)) return translate(`tickets.status.${slug}`);
  return slug;
}

export function statusSlugValid(s: string): boolean {
  return /^[a-z][a-z0-9_]{1,31}$/.test(s);
}

/** Text sent to the KB suggestion endpoint, or null when too short to search. */
export function suggestText(subject: string, body: string): string | null {
  const s = `${subject} ${body.slice(0, 200)}`.trim();
  return /[\p{L}\p{N}]{3,}/u.test(s) ? s.slice(0, 300) : null;
}

export interface CsatSummary { ratings: number; avg_rating: string | null; csat: string | null; rated_tickets_share: string | null; r1: number; r2: number; r3: number; r4: number; r5: number }
export interface CsatGroup { id: string | null; name: string; ratings: number; avg_rating: string | null; csat: string | null }
export interface CsatComment { ticket_id: string; number: number; subject: string; customer_name: string; staff_name: string | null; rating: number; comment: string; rated_at: string }
export interface CsatReport { summary: CsatSummary; by_department: CsatGroup[]; by_staff: CsatGroup[]; comments: CsatComment[] }

/** [stars, count, percent] from 5 down to 1; percents are whole numbers. */
export function distribution(s: CsatSummary): [number, number, number][] {
  const counts = [s.r5, s.r4, s.r3, s.r2, s.r1];
  return counts.map((n, i) => [5 - i, n, s.ratings ? Math.round((100 * n) / s.ratings) : 0]);
}

/** SLA policy (department × priority); null scope fields mean "any". */
export interface SlaPolicy {
  id: string; department_id: string | null; priority: string | null; first_response_minutes: number | null; resolution_minutes: number | null;
  warn_percent: number; escalate_priority: string | null; escalate_to: string | null; escalate_department: string | null; updated_at: string;
}
export type SlaPolicyDraft = Omit<SlaPolicy, "id" | "updated_at">;
export type SlaState = "ok" | "warning" | "breached" | "met" | "missed";
export interface SlaTarget { due_at: string; state: SlaState }
export interface TicketSlaInfo { policy_id: string | null; first_response: SlaTarget | null; resolution: SlaTarget | null; paused: boolean }
export interface SlaAtRisk {
  ticket_id: string; number: number; subject: string; priority: string; status: string; assigned_to: string | null; department_id: string | null;
  fr_due: string | null; res_due: string | null; state: "warning" | "breached";
}

export const blankSlaPolicy: SlaPolicyDraft = {
  department_id: null, priority: null, first_response_minutes: 240, resolution_minutes: null, warn_percent: 80,
  escalate_priority: null, escalate_to: null, escalate_department: null,
};

const MAX_SLA = 525_600;
const okMinutes = (m: number | null) => m == null || (Number.isInteger(m) && m >= 1 && m <= MAX_SLA);

/** Mirrors the server: at least one target, 1..525600 minutes, resolution ≥ first response, warn 1..99 %. */
export function slaPolicyValid(d: SlaPolicyDraft): boolean {
  if (d.first_response_minutes == null && d.resolution_minutes == null) return false;
  if (!okMinutes(d.first_response_minutes) || !okMinutes(d.resolution_minutes)) return false;
  if (!Number.isInteger(d.warn_percent) || d.warn_percent < 1 || d.warn_percent > 99) return false;
  if (d.first_response_minutes != null && d.resolution_minutes != null && d.resolution_minutes < d.first_response_minutes) return false;
  return true;
}

/** "" → null, otherwise the integer (NaN stays NaN so validation fails). */
export function minutesInput(v: string): number | null {
  return v.trim() === "" ? null : Number(v);
}

/** Specificity rank, matching the server's tie-break: department+priority > department > priority > catch-all. */
export function slaSpecificity(p: Pick<SlaPolicy, "department_id" | "priority">): number {
  return (p.department_id ? 2 : 0) + (p.priority ? 1 : 0);
}

export function slaStateVariant(s: SlaState): "destructive" | "default" | "secondary" | "outline" {
  if (s === "breached" || s === "missed") return "destructive";
  if (s === "warning") return "default";
  return s === "met" ? "secondary" : "outline";
}

/** Signed minutes until `due` (negative when overdue). */
export function minutesUntil(due: string, now: number = Date.now()): number {
  return Math.round((new Date(due).getTime() - now) / 60_000);
}

export type AssignMode = "keep" | "self" | "user" | "unassign";
export interface TicketMacro {
  id: string; name: string; description: string; department_id: string | null; reply_body: string; reply_internal: boolean;
  set_status: string | null; set_priority: string | null; assign_mode: AssignMode; assign_to: string | null; set_department: string | null;
  add_tags: string[]; is_active: boolean; sort_order: number; uses: number; updated_at: string;
}
export type MacroDraft = Omit<TicketMacro, "id" | "uses" | "updated_at">;
export interface OfferedMacro extends TicketMacro { rendered_body: string }

export const blankMacro: MacroDraft = {
  name: "", description: "", department_id: null, reply_body: "", reply_internal: false, set_status: null, set_priority: null,
  assign_mode: "keep", assign_to: null, set_department: null, add_tags: [], is_active: true, sort_order: 0,
};

export const MACRO_PLACEHOLDERS = ["customer_name", "customer_first_name", "customer_email", "ticket_number", "ticket_subject", "department", "staff_name"] as const;

/** Mirrors the server: a name, and at least one action. */
export function macroValid(d: MacroDraft): boolean {
  const name = d.name.trim();
  if (!name || name.length > 100 || d.description.length > 500 || d.reply_body.length > 20000) return false;
  if (d.assign_mode === "user" && !d.assign_to) return false;
  return macroActions(d).length > 0;
}

/** What a macro does, as i18n codes (with params) in the order it runs. */
export function macroActions(d: Pick<MacroDraft, "reply_body" | "reply_internal" | "set_status" | "set_priority" | "assign_mode" | "set_department" | "add_tags">): string[] {
  const out: string[] = [];
  if (d.reply_body.trim()) out.push(d.reply_internal ? "macros.do.note" : "macros.do.reply");
  if (d.set_status) out.push("macros.do.status");
  if (d.set_priority) out.push("macros.do.priority");
  if (d.assign_mode !== "keep") out.push(`macros.do.assign_${d.assign_mode}`);
  if (d.set_department) out.push("macros.do.department");
  if (d.add_tags.length) out.push("macros.do.tags");
  return out;
}
