import type { Invoice } from "@/lib/billing/billing";
import { cmpDec } from "@/lib/billing/credit";

export interface Refund { id: string; payment_id: string; invoice_id: string; gateway: string; amount: string; currency: string; status: "pending" | "succeeded" | "failed"; reason: string; error: string | null; credit_note_id: string | null; reversal?: boolean; created_at: string }
export interface RefundListing { refunds: Refund[]; credit_notes: Invoice[]; refundable: Record<string, string> }

/** Validate a refund amount against what is left ("" = full remainder). Returns an i18n key or null. */
export function refundError(amount: string, left: string, exp = 2): string | null {
  const a = amount.trim();
  if (a === "") return cmpDec(left, "0") > 0 ? null : "error.payment_fully_refunded";
  const m = /^\d+(?:\.(\d+))?$/.exec(a);
  if (!m || (m[1]?.length ?? 0) > exp || cmpDec(a, "0") <= 0) return "error.invalid_amount";
  if (cmpDec(a, left) > 0) return "error.refund_exceeds_payment";
  return null;
}

/** Sum of succeeded refunds as an exact decimal string (fixed `exp` places). */
export function refundedTotal(list: Refund[], exp = 2): string {
  const scale = 10n ** BigInt(exp);
  let sum = 0n;
  for (const r of list) {
    if (r.status !== "succeeded") continue;
    const [i = "0", f = ""] = r.amount.split(".");
    sum += BigInt(i) * scale + BigInt((f + "0".repeat(exp)).slice(0, exp) || "0");
  }
  const s = sum.toString().padStart(exp + 1, "0");
  return exp ? `${s.slice(0, -exp)}.${s.slice(-exp)}` : s;
}
