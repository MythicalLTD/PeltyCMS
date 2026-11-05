export interface MoneyPoint { month: string; currency: string; amount: string; count: number }
export interface ChurnPoint { month: string; customers_start: number; churned: number; gained: number; churn_rate: string; new_signups: number }
export interface Report { from: string; to: string; revenue: MoneyPoint[]; refunds: MoneyPoint[]; mrr: MoneyPoint[]; churn: ChurnPoint[] }

/** `YYYY-MM` months from..to inclusive. */
export function monthsBetween(from: string, to: string): string[] {
  const [fy = 0, fm = 1] = from.split("-").map(Number);
  const [ty = 0, tm = 1] = to.split("-").map(Number);
  const out: string[] = [];
  for (let i = fy * 12 + fm - 1; i <= ty * 12 + tm - 1 && out.length < 120; i++) {
    out.push(`${Math.floor(i / 12)}-${String((i % 12) + 1).padStart(2, "0")}`);
  }
  return out;
}

/** The last `n` months ending with the month of `now` (UTC). */
export function lastMonths(n: number, now = new Date()): { from: string; to: string } {
  const f = (d: Date) => `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
  return { from: f(new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - (n - 1), 1))), to: f(now) };
}

export function currenciesOf(r: Report): string[] {
  return [...new Set([...r.revenue, ...r.refunds, ...r.mrr].map((p) => p.currency))].sort();
}

/** Zero-filled series for one currency (amounts as numbers for charting only). */
export function series(points: MoneyPoint[], months: string[], currency: string): number[] {
  return months.map((m) => Number(points.find((p) => p.month === m && p.currency === currency)?.amount ?? 0));
}

/** Exact decimal sum of `amount` strings for one currency (avoids float drift). */
export function sumExact(points: MoneyPoint[], currency: string): string {
  let units = 0n;
  for (const p of points) {
    if (p.currency !== currency) continue;
    const [i, f = ""] = p.amount.replace("-", "").split(".");
    const v = BigInt(i + f.padEnd(4, "0").slice(0, 4));
    units += p.amount.startsWith("-") ? -v : v;
  }
  const neg = units < 0n;
  const abs = neg ? -units : units;
  const s = abs.toString().padStart(5, "0");
  return `${neg ? "-" : ""}${s.slice(0, -4)}.${s.slice(-4)}`;
}
