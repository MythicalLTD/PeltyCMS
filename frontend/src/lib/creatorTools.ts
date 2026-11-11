/** Creator tools: discount codes, bundles, wishlist, messages, listing analytics. */

export interface CreatorCoupon {
  id: string; code: string; description: string; kind: "percent" | "fixed"; value: string; currency: string | null;
  max_uses: number | null; max_uses_per_customer: number | null; uses: number; starts_at: string | null; expires_at: string | null;
  active: boolean; product_ids: string[];
}
export interface CouponDraft {
  id?: string; code: string; description: string; kind: "percent" | "fixed"; value: string; currency: string;
  max_uses: string; max_uses_per_customer: string; expires_at: string; active: boolean; product_ids: string[];
}
export interface BundleItem { product_id: string; product_name: string; cycle: string }
export interface CreatorBundle { id: string; name: string; description: string; active: boolean; discount_percent: string; items: BundleItem[] }
export interface BundleDraft { id?: string; name: string; description: string; discount_percent: string; active: boolean; product_ids: string[] }
export interface WishlistItem { product_id: string; name: string; image_url: string; creator: string | null; creator_slug: string | null; available: boolean; created_at: string }
export interface Conversation {
  id: string; creator_id: string; creator: string; creator_slug: string; customer_id: string; buyer: string;
  product_id: string | null; product: string | null; verified_buyer: boolean; subject: string; status: "open" | "closed";
  buyer_unread: number; creator_unread: number; last_message_at: string;
}
export interface Message { id: string; from_creator: boolean; body: string; created_at: string }
export interface Inbox { items: Conversation[]; unread: number; creator_id: string | null }
export interface Thread { conversation: Conversation; messages: Message[]; as_creator: boolean }
export interface DayStats { day: string; views: number; sales: number; revenue: string; earnings: string }
export interface ListingAnalytics {
  product_id: string; name: string; currency: string; days: number; series: DayStats[];
  totals: { views: number; sales: number; revenue: string; earnings: string; conversion: string; wishlists: number };
}

/** Mirrors the server: upper-case, prefixed with the creator's slug once. */
export function couponPreview(prefix: string, code: string): string {
  const c = code.trim().toUpperCase();
  return c.startsWith(prefix) ? c : `${prefix}${c}`;
}

const posInt = (s: string) => s === "" || (/^\d+$/.test(s) && Number(s) > 0);

export function couponValid(d: CouponDraft, prefix: string): boolean {
  const code = couponPreview(prefix, d.code);
  const v = Number(d.value);
  return (
    d.code.trim().length > 0 && code.length <= 40 && /^[A-Z0-9_-]+$/.test(code) &&
    Number.isFinite(v) && v > 0 && (d.kind === "fixed" ? /^[A-Z]{3}$/.test(d.currency) : v <= 100) &&
    posInt(d.max_uses) && posInt(d.max_uses_per_customer) && d.product_ids.length > 0
  );
}

/** Request body for the coupon API from a form draft. */
export function couponBody(d: CouponDraft) {
  return {
    code: d.code.trim(), description: d.description.trim(), kind: d.kind, value: d.value.trim(),
    currency: d.kind === "fixed" ? d.currency : null,
    max_uses: d.max_uses ? Number(d.max_uses) : null,
    max_uses_per_customer: d.max_uses_per_customer ? Number(d.max_uses_per_customer) : null,
    expires_at: d.expires_at ? new Date(`${d.expires_at}T23:59:59Z`).toISOString() : null,
    active: d.active, product_ids: d.product_ids,
  };
}

export function couponDraft(c?: CreatorCoupon, prefix = ""): CouponDraft {
  if (!c) return { code: "", description: "", kind: "percent", value: "", currency: "EUR", max_uses: "", max_uses_per_customer: "1", expires_at: "", active: true, product_ids: [] };
  return {
    id: c.id, code: c.code.startsWith(prefix) ? c.code.slice(prefix.length) : c.code, description: c.description, kind: c.kind, value: String(Number(c.value)),
    currency: c.currency ?? "EUR", max_uses: c.max_uses?.toString() ?? "", max_uses_per_customer: c.max_uses_per_customer?.toString() ?? "",
    expires_at: c.expires_at ? c.expires_at.slice(0, 10) : "", active: c.active, product_ids: c.product_ids,
  };
}

export function bundleValid(d: BundleDraft): boolean {
  const p = Number(d.discount_percent);
  return d.name.trim().length > 0 && d.name.length <= 200 && d.product_ids.length >= 2 && d.product_ids.length <= 10 && Number.isFinite(p) && p >= 0 && p <= 100 && d.discount_percent.trim() !== "";
}

/** Bundle price from the item prices and the discount (rounded to cents). */
export function bundlePrice(prices: number[], percent: number): { full: number; price: number; saved: number } {
  const full = prices.reduce((a, b) => a + b, 0);
  const saved = Math.round(full * percent) / 100;
  return { full, price: Math.round((full - saved) * 100) / 100, saved };
}

export function toggle<T>(xs: T[], x: T): T[] {
  return xs.includes(x) ? xs.filter((y) => y !== x) : [...xs, x];
}

/** Unread count for the side the viewer is on. */
export function unreadFor(c: Conversation, creatorId: string | null): number {
  return creatorId && c.creator_id === creatorId ? c.creator_unread : c.buyer_unread;
}
