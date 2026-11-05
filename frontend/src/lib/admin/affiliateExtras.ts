export interface ProductRate { product_id: string; product_name: string; kind: "percent" | "fixed"; value: string; currency: string | null }
export interface Tier { id: string; name: string; min_referrals: number; commission_percent: string }
export interface Material { id: string; kind: "banner" | "text"; title: string; body: string; image_url: string; sort_order: number; active: boolean; created_at: string }

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Copy-paste snippet for one material with the affiliate's link filled in. */
export function materialSnippet(m: Pick<Material, "kind" | "title" | "body" | "image_url">, url: string): string {
  if (m.kind === "banner") return `<a href="${esc(url)}"><img src="${esc(m.image_url)}" alt="${esc(m.title)}" /></a>`;
  return m.body.includes("{link}") ? m.body.split("{link}").join(url) : `${m.body}\n${url}`.trim();
}

/** Next tier above the current referral count, if any. */
export function nextTier(tiers: Tier[], referrals: number): Tier | null {
  return [...tiers].sort((a, b) => a.min_referrals - b.min_referrals).find((t) => t.min_referrals > referrals) ?? null;
}
