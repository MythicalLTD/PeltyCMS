/** Admin tools: global search, system health, My Notes, notification rules. */
export interface SearchResults {
  customers: { id: string; name: string; organization: string; email: string; status: string }[];
  invoices: { id: string; number: string | null; status: string; total: string; currency: string; customer: string }[];
  services: { id: string; product: string; status: string; customer: string }[];
  tickets: { id: string; number: number; subject: string; status: string }[];
  domains: { id: string; name: string; status: string; expires_on: string | null }[];
  orders: { id: string; number: number; status: string; total: string; currency: string }[];
}
export type SearchGroup = keyof SearchResults;
export const SEARCH_GROUPS: SearchGroup[] = ["customers", "invoices", "services", "tickets", "domains", "orders"];

/** Admin route a search hit opens. */
export function searchHref(group: SearchGroup, hit: { id: string; email?: string; product?: string }): string {
  switch (group) {
    case "customers": return `/admin/customers/${hit.id}`;
    case "invoices": return `/admin/invoices/${hit.id}`;
    case "services": return `/admin/services?q=${encodeURIComponent(hit.id)}`;
    case "tickets": return `/admin/tickets/${hit.id}`;
    case "domains": return `/admin/domains/${hit.id}`;
    case "orders": return `/admin/orders/${hit.id}`;
  }
}

/** Search only runs for 2..100 chars, matching the server rule. */
export function searchable(q: string): boolean {
  const n = [...q.trim()].length;
  return n >= 2 && q.trim().length <= 100;
}

export interface HealthCheck { key: string; status: "ok" | "warning" | "error"; value?: number }
export interface SystemHealth { app_version: string; checks: HealthCheck[]; metrics: Record<string, number | string> }

export function overallStatus(checks: HealthCheck[]): HealthCheck["status"] {
  if (checks.some((c) => c.status === "error")) return "error";
  if (checks.some((c) => c.status === "warning")) return "warning";
  return "ok";
}

export const NOTIFY_PROVIDERS = ["slack", "discord", "teams", "webhook", "telegram", "sms_webhook", "sms_email"] as const;
export interface NotificationRule {
  id: string; name: string; provider: string; url_masked: string; target?: string; events: string[]; enabled: boolean;
  signed?: boolean; stats?: { delivered: number; failed: number; pending: number };
}
export interface NotificationDelivery { id: string; event: string; status: "pending" | "delivered" | "failed"; attempts: number; response_status: number | null; last_error: string | null; created_at: string }
/** Which inputs a provider needs: an https URL, a target (chat/phone/address), or both. */
export function providerFields(p: string): { url: boolean; target: boolean } {
  if (p === "sms_email") return { url: false, target: true };
  if (p === "telegram" || p === "sms_webhook") return { url: true, target: true };
  return { url: true, target: false };
}
