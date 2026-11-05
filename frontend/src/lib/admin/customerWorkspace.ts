import type { ClientSummary } from "./clientSummary";

export interface WorkspaceAccess {
  finance: boolean; services: boolean; support: boolean; operations: boolean; audit: boolean; manage_customer: boolean;
}
export interface SectionMeta { total: number | null; returned: number; truncated: boolean; accessible: boolean }
export interface CustomerWorkspace extends Omit<ClientSummary, "stats"> {
  access: WorkspaceAccess;
  counts: { services_active: number | null; services_total: number | null; domains_total: number | null; tickets_open: number | null };
  balances: { currency: string; amount_due: string; amount_paid: string; credit: string; invoices_unpaid: number }[];
  attention: { kind: "invoice" | "ticket" | "job"; count: number }[];
  limits: { records: number };
  totals: Record<string, number | null>;
  meta: Record<string, SectionMeta>;
  refunds: { id: string; payment_id: string; invoice_id: string; amount: string; currency: string; status: string; to_credit: boolean; created_at: string }[];
  chats: { id: string; status: string; created_at: string; last_message_at: string | null; closed_at: string | null }[];
  emails: { id: string; job_id: number | null; status: string; template: string | null; subject: string | null; created_at: string }[];
  failures: { id: string; kind: string; status: string; service_id: string; created_at: string }[];
}
export const WORKSPACE_TABS = ["overview", "summary", "contacts", "services", "domains", "invoices", "transactions", "tickets", "chats", "emails", "notes", "log", "operations"] as const;
export type WorkspaceTab = typeof WORKSPACE_TABS[number];
export const ACTIVITY_KINDS = ["all", "invoice", "payment", "refund", "credit", "service", "domain", "ticket", "chat", "email", "job", "note", "audit", "customer"] as const;
export type ActivityKind = typeof ACTIVITY_KINDS[number];
export interface ActivityItem { id: string; kind: ActivityKind; occurred_at: string; title: string; status: string | null; href: string }
export interface ActivityPage { items: ActivityItem[]; next_cursor: string | null }
export const workspaceKey = (id: string) => ["admin", "customer-workspace", id] as const;
export const activityKey = (id: string) => ["admin", "customer-activity", id] as const;

export function effectiveAccess(a: WorkspaceAccess, permissions: readonly string[]): WorkspaceAccess {
  const read = (resource: string) => permissions.includes(`${resource}.manage`) || permissions.includes(`${resource}.read`);
  return {
    finance: a.finance && read("invoices"), services: a.services && read("services"),
    support: a.support && read("tickets"), operations: a.operations && read("services") && permissions.includes("jobs.manage"),
    audit: a.audit && permissions.includes("audit.read"),
    manage_customer: a.manage_customer && permissions.includes("customers.manage"),
  };
}
export function canViewTab(tab: WorkspaceTab, a: WorkspaceAccess): boolean {
  if (["invoices", "transactions"].includes(tab)) return a.finance;
  if (["services", "domains"].includes(tab)) return a.services;
  if (["tickets", "chats"].includes(tab)) return a.support;
  if (tab === "log") return a.audit;
  if (tab === "operations") return a.operations && a.services;
  return true;
}
export function allowedKinds(a: WorkspaceAccess): ActivityKind[] {
  return ACTIVITY_KINDS.filter(k => {
    if (["invoice", "payment", "refund", "credit"].includes(k)) return a.finance;
    if (["service", "domain"].includes(k)) return a.services;
    if (["ticket", "chat"].includes(k)) return a.support;
    if (k === "job") return a.operations && a.services;
    if (k === "audit") return a.audit;
    return true;
  });
}
export function selectedTab(search: URLSearchParams, access: WorkspaceAccess): WorkspaceTab {
  const tab = search.get("tab") as WorkspaceTab;
  return WORKSPACE_TABS.includes(tab) && canViewTab(tab, access) ? tab : "overview";
}
export function tabSearch(search: URLSearchParams, tab: WorkspaceTab): URLSearchParams {
  const next = new URLSearchParams(search);
  next.set("tab", tab);
  return next;
}
export function activityPath(id: string, kind: ActivityKind, before?: string | null): string {
  const query = new URLSearchParams({ kind, limit: "25" });
  if (before) query.set("before", before);
  return `/admin/customers/${encodeURIComponent(id)}/activity?${query}`;
}
export function validNote(body: string): boolean {
  const length = Array.from(body.trim()).length;
  return length > 0 && length <= 10_000;
}
const eventTabs: Partial<Record<ActivityKind, WorkspaceTab>> = { payment: "transactions", refund: "transactions", credit: "transactions", email: "emails", note: "notes", audit: "log", customer: "summary", job: "operations", chat: "chats" };
/** Fill in old root-only server links; retain newer server-provided deep links. */
export function activityHref(id: string, item: ActivityItem): string {
  const root = `/admin/customers/${encodeURIComponent(id)}`;
  if (item.href && item.href !== root) return item.href;
  const resourceId = encodeURIComponent(item.id.slice(item.id.indexOf(":") + 1));
  if (item.kind === "invoice") return `/admin/invoices/${resourceId}`;
  if (item.kind === "domain") return `/admin/domains/${resourceId}`;
  if (item.kind === "ticket") return `/admin/tickets/${resourceId}`;
  if (item.kind === "service") return `/admin/services?q=${resourceId}`;
  return `${root}?tab=${eventTabs[item.kind] ?? "overview"}`;
}
export function genericActivityTitle(item: ActivityItem): boolean {
  return ["Invoice", "Payment", "Refund", "Credit entry", "Customer chat", "Email delivery", "Provisioning failure", "Customer note", "Customer created"].includes(item.title);
}
