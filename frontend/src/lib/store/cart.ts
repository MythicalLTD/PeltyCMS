export interface CartLine { product_id: string; cycle: string; config: Record<string, string | number> }
export interface Bundle { id: string; name: string; description: string; active: boolean; discount_percent: string; items: { product_id: string; product_name: string; cycle: string }[] }
export interface Recommendations { upsell: string[]; cross_sell: string[] }

/** Cart indices covering every bundle item (mirrors the server check). */
export function matchBundle(b: Bundle, cart: CartLine[]): number[] | null {
  const used = new Set<number>();
  const out: number[] = [];
  for (const bi of b.items) {
    const i = cart.findIndex((c, j) => !used.has(j) && c.product_id === bi.product_id && c.cycle === bi.cycle);
    if (i < 0) return null;
    used.add(i);
    out.push(i);
  }
  return out;
}

/** Estimated bundle discount given each line's recurring amount after promo. */
export function bundleDiscount(b: Bundle, cart: CartLine[], recurring: number[]): number {
  const idx = matchBundle(b, cart);
  if (!idx) return 0;
  const pct = Number(b.discount_percent);
  return idx.reduce((a, i) => a + Math.round((recurring[i] ?? 0) * pct) / 100, 0);
}

/** Suggestions for the cart: cross-sells not already in it, max `n`. */
export function crossSells(recs: Recommendations | undefined, cart: CartLine[], current: string | null, n = 3): string[] {
  const inCart = new Set(cart.map((c) => c.product_id));
  return (recs?.cross_sell ?? []).filter((id) => !inCart.has(id) && id !== current).slice(0, n);
}
