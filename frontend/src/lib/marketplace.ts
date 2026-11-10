/** Creator marketplace types and helpers (module `creator_marketplace`). */
export type PayoutMethod = "manual" | "stripe_connect" | "paypal";
export type PayoutSchedule = "manual" | "weekly" | "monthly";
export interface MarketConfig {
  mode: "multi_vendor" | "admin_only"; commission_percent: string; hold_days: number; min_payout: string; auto_approve_creators: boolean; auto_approve_listings: boolean;
  payout_schedule: PayoutSchedule; payout_weekday: number; payout_monthday: number; payout_approval: boolean; payout_methods: PayoutMethod[]; reverse_transfers_on_refund: boolean;
}
export interface Creator {
  id: string; user_id: string; customer_id: string; slug: string; display_name: string; bio: string; website: string; avatar_url: string;
  status: string; commission_percent: string | null; payout_method: string; payout_details: Record<string, string>; created_at: string;
  stripe_account_id: string; stripe_status: StripeStatus; stripe_status_detail: string; paypal_email: string; payout_method_locked: boolean;
}
export type StripeStatus = "none" | "onboarding" | "restricted" | "enabled" | "disconnected";
export interface CreatorRow extends Creator { listings: number; sales: number }
export interface Listing {
  product_id: string; creator_id: string; creator: string; creator_slug: string; name: string; slug: string; description: string; image_url: string;
  product_status: string; status: string; reject_reason: string; submitted_at: string; sales: number;
}
export interface Balance { currency: string; available: string; pending: string; gross: string; commission: string }
export interface LedgerEntry { id: string; kind: string; product_id: string | null; product: string | null; currency: string; gross: string; commission: string; amount: string; available_at: string; note: string; created_at: string }
export interface Payout { id: string; creator_id: string; creator: string; currency: string; amount: string; method: string; status: string; provider_ref: string; note: string; created_at: string; processed_at: string | null; batch_id?: string; reversed?: string; auto?: boolean }
export interface Dashboard {
  creator: Creator | null; mode: string; commission_percent?: string; hold_days?: number; min_payout?: string;
  balances?: Balance[]; products?: { product_id: string; name: string; currency: string; sales: number; net: string }[];
  payout?: PayoutInfo;
}
/** Payout state on the creator dashboard. */
export interface PayoutInfo {
  method: PayoutMethod; locked: boolean; methods: PayoutMethod[]; paypal_email: string;
  stripe: { available: boolean; test_mode: boolean; connected: boolean; status: StripeStatus; detail: string };
  paypal: { available: boolean; test_mode: boolean };
  schedule: PayoutSchedule; weekday: number; monthday: number; approval: boolean;
  in_transit: { currency: string; amount: string }[];
}
export interface ProviderState { configured: boolean; live_refused: boolean; test_mode: boolean; webhook_ready: boolean; webhook_url: string; env: string[] }
/** `GET /admin/creator-payouts/overview`. */
export interface PayoutOverview {
  stripe: ProviderState; paypal: ProviderState; live_env: string; config: MarketConfig;
  totals: { status: string; currency: string; count: number; amount: string }[];
}

export const PAYOUT_METHODS = ["manual", "stripe_connect", "paypal"] as const satisfies readonly PayoutMethod[];
export const PAYOUT_SCHEDULES = ["manual", "weekly", "monthly"] as const satisfies readonly PayoutSchedule[];

/** Loose e-mail check matching the server's PayPal receiver rule. */
export function paypalEmailValid(e: string): boolean {
  const v = e.trim();
  const at = v.indexOf("@");
  if (at < 1 || v.length > 254 || /[\s,;]/.test(v)) return false;
  const domain = v.slice(at + 1);
  return domain.includes(".") && !domain.startsWith(".") && !domain.endsWith(".") && !domain.includes("@");
}

/** Whether a Stripe status lets payouts go out. */
export function stripeReady(s: StripeStatus | undefined): boolean {
  return s === "enabled";
}

/** Sum of a currency's open payouts ("in transit"). */
export function inTransit(info: PayoutInfo | undefined, currency: string): string {
  return info?.in_transit.find((x) => x.currency === currency)?.amount ?? "0";
}

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
    payout_schedule: (PAYOUT_SCHEDULES as readonly string[]).includes(String(s.payout_schedule)) ? (s.payout_schedule as PayoutSchedule) : "manual",
    payout_weekday: clampInt(s.payout_weekday, 1, 7, 1),
    payout_monthday: clampInt(s.payout_monthday, 1, 28, 1),
    payout_approval: s.payout_approval !== false,
    payout_methods: Array.isArray(s.payout_methods) ? PAYOUT_METHODS.filter((m) => (s.payout_methods as unknown[]).includes(m)) : [...PAYOUT_METHODS],
    reverse_transfers_on_refund: s.reverse_transfers_on_refund !== false,
  };
}

/** creator slug from a display name. */
export function creatorSlug(name: string): string {
  return name.normalize("NFKD").replace(/\p{M}+/gu, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40).replace(/-+$/, "");
}

const DECIMAL = /^\d{1,12}(\.\d{1,4})?$/;
/** Non-negative decimal as typed in a settings field. */
export function validAmount(v: string): boolean {
  return DECIMAL.test(v.trim());
}
/** Decimal between 0 and 100. */
export function validPercent(v: string): boolean {
  return validAmount(v) && Number(v) <= 100;
}

function clampInt(v: unknown, min: number, max: number, fallback: number): number {
  return typeof v === "number" && Number.isInteger(v) ? Math.min(max, Math.max(min, v)) : fallback;
}

export const GALLERY_MAX = 12;
/** Mirrors the server rule: ≤ 12 images, https / http / site-relative URLs, captions ≤ 200. */
export function galleryValid(items: { url: string; caption: string }[]): boolean {
  const filled = items.filter((x) => x.url.trim());
  return filled.length <= GALLERY_MAX && filled.every((x) => {
    const u = x.url.trim();
    return u.length <= 500 && ((u.startsWith("/") && !u.startsWith("//")) || u.startsWith("https://") || u.startsWith("http://")) && !/\s/.test(u) && x.caption.length <= 200;
  });
}

/** Move one item up (-1) or down (+1). */
export function moveItem<T>(xs: T[], i: number, by: -1 | 1): T[] {
  const j = i + by;
  if (j < 0 || j >= xs.length) return xs;
  const out = [...xs];
  const [moved] = out.splice(i, 1);
  out.splice(j, 0, moved as T);
  return out;
}
