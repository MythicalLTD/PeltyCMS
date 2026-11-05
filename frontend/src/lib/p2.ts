export interface Affiliate { id: string; user_id: string; email: string; code: string; commission_percent: string; status: string; referrals: number; created_at: string }
export interface Balance { currency: string; earned: string; withdrawn: string; available: string }
export interface Commission { id: string; invoice_id: string; invoice_number: string | null; currency: string; amount: string; status: string; created_at: string }
export interface Payout { id: string; affiliate_id: string; email: string; currency: string; amount: string; status: string; note: string; created_at: string }
export type AffiliateDashboard = { enrolled: false } | { enrolled: true; affiliate: Affiliate; link: string; balances: Balance[]; commissions: Commission[]; payouts: Payout[] };
export interface QuoteItem { description: string; quantity: string; unit_price: string; tax_rate: string; amount?: string }
export interface Quote { id: string; number: string; customer_id: string; customer_email: string; currency: string; subject: string; notes: string; status: string; valid_until: string; invoice_id: string | null; created_at: string; subtotal: string; tax_total: string; total: string; items: QuoteItem[] }
export interface Download { id: string; title: string; description: string; filename: string; content_type: string; size_bytes: number; product_ids: string[]; created_at: string }

/** Human file size (1024 base, one decimal). */
export function fileSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB"];
  let v = n / 1024, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(1)} ${u[i]}`;
}

/** Full referral URL for the current origin. */
export function referralUrl(link: string, origin: string): string {
  return `${origin.replace(/\/$/, "")}${link}`;
}
