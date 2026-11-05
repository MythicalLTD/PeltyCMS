export interface UsageMetric { id: string; product_id: string; key: string; name: string; unit: string; unit_price: string; included_quantity: string; active: boolean; source_key: string; source_divisor: string; aggregate: "sum" | "max" }
export interface MetricDraft { product_id: string; key: string; name: string; unit: string; unit_price: string; included_quantity: string; active: boolean; source_key: string; source_divisor: string; aggregate: "sum" | "max" }
export interface UsageLine { key: string; name: string; unit: string; quantity: string; included: string; billable: string; unit_price: string; charge: string }
export interface UsagePeriod { period: { id: string; period_start: string; period_end: string; status: "open" | "closed" | "billed"; invoice_id: string | null; total: string | null }; metrics: UsageLine[] }

/** Dotted path into the server module usage JSON, e.g. `bw.gb`. */
export const SOURCE_RE = /^[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)*$/;
export const KEY_RE = /^[a-z][a-z0-9_]{0,39}$/;
const dec = (s: string, scale: number) => new RegExp(`^\\d{1,12}(\\.\\d{1,${scale}})?$`).test(s.trim());

/** POST/PUT body for a metric, or null if invalid. */
export function metricPayload(d: MetricDraft) {
  if (!d.product_id || !KEY_RE.test(d.key.trim()) || !d.name.trim() || !d.unit.trim()) return null;
  if (!dec(d.unit_price, 8) || !dec(d.included_quantity || "0", 6)) return null;
  const src = d.source_key.trim();
  const div = (d.source_divisor || "1").trim();
  if (src && !SOURCE_RE.test(src)) return null;
  if (!dec(div, 6) || Number(div) <= 0) return null;
  return { product_id: d.product_id, key: d.key.trim(), name: d.name.trim(), unit: d.unit.trim(), unit_price: d.unit_price.trim(), included_quantity: (d.included_quantity || "0").trim(), active: d.active, source_key: src, source_divisor: div, aggregate: d.aggregate };
}

/** Valid record quantity: positive, max 6 decimals. */
export const validQuantity = (q: string) => dec(q, 6) && Number(q) > 0;

/** Strip insignificant trailing zeros from a decimal string ("1.500000" -> "1.5"). */
export function trimDecimal(s: string) {
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}

export const periodVariant = (s: string) => (s === "open" ? "default" : s === "billed" ? "secondary" : "outline") as "default" | "secondary" | "outline";
