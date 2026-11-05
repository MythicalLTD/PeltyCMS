export interface ClientSummary {
  customer: { id: string; email: string; first_name: string; last_name: string; organization: string; status: string; currency: string; phone: string; address_line1: string; address_line2: string; city: string; region: string; postal_code: string; country: string; tax_id: string; created_at: string };
  stats: { services_active: number; services_total: number; domains_total: number; invoices_unpaid: number; amount_due: string; amount_paid: string; tickets_open: number; credit: string };
  contacts: { id: string; name: string; email: string; phone: string; is_billing: boolean }[];
  services: { id: string; product: string; status: string; cycle: string; amount: string; currency: string; next_due_date: string | null }[];
  domains: { id: string; name: string; status: string; expires_on: string | null; auto_renew: boolean }[];
  invoices: { id: string; number: string | null; kind: string; status: string; total: string; amount_paid: string; currency: string; issued_at: string | null; due_at: string | null }[];
  transactions: { id: string; gateway: string; provider_ref: string | null; amount: string; currency: string; status: string; created_at: string }[];
  credit_ledger: { id: string; amount: string; currency: string; kind: string; note: string; created_at: string }[];
  tickets: { id: string; number: number; subject: string; status: string; priority: string; last_reply_at: string }[];
  notes: { id: string; body: string; sticky: boolean; created_at: string; author: string | null }[];
  log: { id: string; action: string; target: string | null; ip: string | null; created_at: string; actor: string | null }[];
}
export const CLIENT_TABS = ["summary", "contacts", "services", "domains", "invoices", "transactions", "tickets", "notes", "emails", "log"] as const;
export function displayName(c: { first_name: string; last_name: string; organization: string; email: string }): string {
  return `${c.first_name} ${c.last_name}`.trim() || c.organization || c.email;
}
