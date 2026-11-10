/** Creator marketplace types and helpers (module `creator_marketplace`). */
export interface MarketConfig { mode: "multi_vendor" | "admin_only"; commission_percent: string; hold_days: number; min_payout: string; auto_approve_creators: boolean; auto_approve_listings: boolean }
export interface Creator {
  id: string; user_id: string; customer_id: string; slug: string; display_name: string; bio: string; website: string; avatar_url: string;
  status: string; commission_percent: string | null; payout_method: string; payout_details: Record<string, string>; created_at: string;
}
export interface CreatorRow extends Creator { listings: number; sales: number }
export interface Listing {
  product_id: string; creator_id: string; creator: string; creator_slug: string; name: string; slug: string; description: string; image_url: string;
  product_status: string; status: string; reject_reason: string; submitted_at: string; sales: number;
}
export interface Balance { currency: string; available: string; pending: string; gross: string; commission: string }
export interface LedgerEntry { id: string; kind: string; product_id: string | null; product: string | null; currency: string; gross: string; commission: string; amount: string; available_at: string; note: string; created_at: string }
export interface Payout { id: string; creator_id: string; creator: string; currency: string; amount: string; method: string; status: string; provider_ref: string; note: string; created_at: string; processed_at: string | null }
export interface Dashboard {
  creator: Creator | null; mode: string; commission_percent?: string; hold_days?: number; min_payout?: string;
  balances?: Balance[]; products?: { product_id: string; name: string; currency: string; sales: number; net: string }[];
}

export const PAYOUT_METHODS = ["manual", "stripe_connect", "paypal"] as const;

/** What a creator keeps from a price at a commission (2 decimals, half-up). */
export function creatorNet(price: string, commissionPercent: string): string {
  const p = Number(price);
  const c = Number(commissionPercent);
  if (!Number.isFinite(p) || !Number.isFinite(c)) return "0.00";
  const fee = Math.round(p * c) / 100;
  return (p - fee).toFixed(2);
}

/** Read the lenient settings object the module stores. */
export function marketConfig(settings: Record<string, unknown> | undefined): MarketConfig {
  const s = settings ?? {};
  return {
    mode: s.mode === "admin_only" ? "admin_only" : "multi_vendor",
    commission_percent: String(s.commission_percent ?? "10"),
    hold_days: typeof s.hold_days === "number" ? s.hold_days : 14,
    min_payout: String(s.min_payout ?? "10"),
    auto_approve_creators: s.auto_approve_creators === true,
    auto_approve_listings: s.auto_approve_listings === true,
  };
}

/** creator slug from a display name. */
export function creatorSlug(name: string): string {
  return name.toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40);
}
