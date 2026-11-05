import type { TranslationKey } from "@/lib/i18n-core";
/** Billing types and money display. Amounts arrive as exact decimal strings. */
export interface Invoice {
  id: string; number: string | null; kind: string; customer_id: string; currency: string; status: string;
  subtotal: string; tax_total: string; total: string; amount_paid: string;
  issued_at: string | null; due_at: string | null; paid_at: string | null; finalized_at?: string | null; created_at: string;
  tax_inclusive?: boolean; reverse_charge?: boolean; tax_breakdown?: TaxPart[]; customer_snapshot?: Record<string, unknown> | null;
}
export interface TaxPart { name: string; rate: string; compound: boolean; amount: string }
export interface InvoiceItem { id: string; description: string; quantity: string; unit_price: string; tax_rate: string; amount: string; tax_amount: string }
export interface Payment { id: string; gateway: string; amount: string; currency: string; status: string; applied_at: string | null; created_at: string }
export interface InvoiceDetail {
  invoice: Invoice;
  items: InvoiceItem[];
  payments: Payment[];
  balance: string;
  debit_notes?: Invoice[];
  related?: { id: string; number: string | null; kind: string } | null;
  note_reason?: string;
  client_notes?: string;
  merged_into?: string | null;
  split_from?: string | null;
}
export interface Service {
  id: string; customer_id: string; product_id: string; product_name: string; status: string; cycle: string; currency: string;
  recurring_amount: string; next_due_date: string | null; suspend_reason: string; created_at: string;
}
export interface Order { id: string; number: number; customer_id: string; status: string; currency: string; total: string; invoice_id: string | null; invoice_status?: string | null; created_at: string }
export interface Customer { id: string; owner_user_id?: string | null; owner_avatar_version?: number | null; email: string; organization: string; first_name: string; last_name: string; country: string; currency: string; status: string; notes?: string; created_at: string; vat_number?: string; tax_exempt?: boolean }
export interface Job { id: number; kind: string; status: string; attempts: number; max_attempts: number; run_at: string; last_error: string | null; created_at: string }
export interface Product { id: string; name: string; slug: string; status: string; group_id?: string; description?: string; module: string | null; stock_enabled?: boolean; stock_qty?: number; allow_backorder?: boolean; prices: { currency: string; cycle: string; amount: string; setup_fee: string }[]; options?: import("@/pages/client/services/Order").ConfigOption[] }

/**
 * Format a decimal string as currency. The string is passed to Intl as-is
 * (string input keeps full precision; Intl applies the currency's own
 * minor-unit digits, e.g. 0 for JPY, 3 for KWD).
 */
export function formatMoney(amount: string, currency: string, lang: string): string {
  try {
    return new Intl.NumberFormat(lang, { style: "currency", currency }).format(amount as unknown as number);
  } catch {
    return `${amount} ${currency}`;
  }
}

export { formatDate } from "../format";

export type Tone = "neutral" | "success" | "warning" | "danger" | "primary";
export function statusTone(s: string): Tone {
  if (["paid", "active", "succeeded"].includes(s)) return "success";
  if (["unpaid", "pending", "queued", "running", "suspended"].includes(s)) return "warning";
  if (["fraud", "dead", "failed", "terminated"].includes(s)) return "danger";
  return "neutral";
}

/** An order accepted by staff (or auto-approved) before its invoice was paid. */
export const awaitingPayment = (orderStatus: string, invoiceStatus: string | null | undefined) =>
  orderStatus === "active" && (invoiceStatus === "unpaid" || invoiceStatus === "overdue");

/** Display label for an invoice number: proforma invoices (issued, not yet numbered) vs drafts. */
export function invoiceLabel(i: { number: string | null; status?: string; id?: string }, t: (k: TranslationKey, v?: Record<string, string | number>) => string): string {
  if (i.number) return i.number;
  if (i.status && i.status !== "draft") return t("billing.proforma_invoice", { id: (i.id ?? "").slice(0, 8) });
  return t("billing.draft_invoice");
}
