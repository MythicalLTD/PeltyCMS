export interface VendorItem {
  id: string; server_id: string; server_name: string; module: string; sku: string; name: string; description: string;
  cost: string; currency: string; cycle: string; available: boolean; product_id: string | null; product_name: string | null;
  price: string | null; markup_percent: string | null; synced_at: string;
}

const SCALE = 4;
/** Decimal string → integer in 1e-4 units (exact; null when not a plain decimal). */
export function toUnits(s: string): bigint | null {
  const m = /^(-?)(\d+)(?:\.(\d+))?$/.exec(s.trim());
  if (!m) return null;
  const frac = (m[3] ?? "").padEnd(SCALE, "0");
  if (frac.length > SCALE && /[1-9]/.test(frac.slice(SCALE))) return null;
  const v = BigInt(m[2] + frac.slice(0, SCALE));
  return m[1] ? -v : v;
}

function fromUnits(v: bigint, dp: number): string {
  const neg = v < 0n;
  const a = neg ? -v : v;
  const div = 10n ** BigInt(SCALE - dp);
  const r = (a + div / 2n) / div; // half away from zero
  const s = r.toString().padStart(dp + 1, "0");
  const out = dp ? `${s.slice(0, -dp)}.${s.slice(-dp)}` : s;
  return neg && r !== 0n ? `-${out}` : out;
}

/** Sale price for a markup, mirroring the server (half-up to `dp`). */
export function salePrice(cost: string, markup: string, dp = 2): string | null {
  const c = toUnits(cost);
  const m = toUnits(markup);
  if (c === null || m === null || m < -1_000_000n || m > 100_000_000n) return null;
  // m is percent in 1e-4 units: cost × (1e6 + m) / 1e6
  const num = c * (1_000_000n + m);
  const q = num / 1_000_000n;
  const rem = num % 1_000_000n;
  const rounded = rem * 2n >= 1_000_000n ? q + 1n : q;
  const v = fromUnits(rounded < 0n ? 0n : rounded, dp);
  return v;
}

/** Margin (price − cost) and percent of price; null when unknown. */
export function margin(cost: string, price: string | null, dp = 2): { amount: string; percent: string } | null {
  if (price === null) return null;
  const c = toUnits(cost);
  const p = toUnits(price);
  if (c === null || p === null) return null;
  const pct = p === 0n ? "0.0" : fromUnits(((p - c) * 1_000_000n) / p, 1).replace(/^(-?\d+\.\d).*/, "$1");
  return { amount: fromUnits(p - c, dp), percent: pct };
}
