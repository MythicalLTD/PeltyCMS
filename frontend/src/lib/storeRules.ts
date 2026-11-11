/** Game store rules (module `gaming_storefront`): package limits, required
 * packages, pay what you want, gifting, the ban list and currency detection. */
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useModules } from "@/lib/modules";

export const RULE_PERIODS = ["all", "day", "week", "month"] as const;
export type RulePeriod = (typeof RULE_PERIODS)[number];

export interface PackageRule {
  product_id: string; user_limit: number | null; user_period: RulePeriod; global_limit: number | null; global_period: RulePeriod;
  required_product_ids: string[]; pay_what_you_want: boolean; giftable: boolean;
}
export interface PublicRule {
  product_id: string; user_limit: number | null; user_period: RulePeriod; user_left: number | null; global_limit: number | null; global_period: RulePeriod;
  global_left: number | null; required: { id: string; name: string }[]; requirement_met: boolean | null; pay_what_you_want: boolean; giftable: boolean;
}
export interface StoreBan { id: string; kind: "username" | "email"; value: string; reason: string; created_at: string }

/** Form state: limits as text so the fields can be empty. */
export interface RuleDraft { product_id: string; user_limit: string; user_period: RulePeriod; global_limit: string; global_period: RulePeriod; required_product_ids: string[]; pay_what_you_want: boolean; giftable: boolean }

export function ruleDraft(productId: string, r?: PackageRule): RuleDraft {
  return {
    product_id: productId, user_limit: r?.user_limit?.toString() ?? "", user_period: r?.user_period ?? "all",
    global_limit: r?.global_limit?.toString() ?? "", global_period: r?.global_period ?? "all",
    required_product_ids: r?.required_product_ids ?? [], pay_what_you_want: r?.pay_what_you_want ?? false, giftable: r?.giftable ?? true,
  };
}

const limitOk = (s: string, max: number) => s === "" || (/^\d+$/.test(s) && Number(s) >= 1 && Number(s) <= max);

export function ruleValid(d: RuleDraft): boolean {
  return limitOk(d.user_limit, 100_000) && limitOk(d.global_limit, 10_000_000) && d.required_product_ids.length <= 10 && !d.required_product_ids.includes(d.product_id);
}

export function ruleBody(d: RuleDraft): PackageRule {
  return {
    product_id: d.product_id, user_limit: d.user_limit ? Number(d.user_limit) : null, user_period: d.user_period,
    global_limit: d.global_limit ? Number(d.global_limit) : null, global_period: d.global_period,
    required_product_ids: d.required_product_ids, pay_what_you_want: d.pay_what_you_want, giftable: d.giftable,
  };
}

/** Mirrors the server: at least the price, at most 100× (min 1, cap 100000), 2 decimals. */
export function payAmountValid(text: string, listed: number): boolean {
  if (!/^\d+(\.\d{1,2})?$/.test(text.trim())) return false;
  const v = Number(text);
  const max = Math.min(Math.max(listed * 100, 1), 100_000);
  return v >= listed && v <= max;
}

/** Mirrors `community::username_ok` (what the delivery can put in a command). */
export function giftNameValid(name: string): boolean {
  const n = name.trim();
  return n.length >= 1 && n.length <= 64 && /^[\p{L}\p{N}_.\- ]+$/u.test(n) && !n.includes("  ");
}

export function banValueValid(kind: StoreBan["kind"], v: string): boolean {
  const s = v.trim();
  return kind === "email" ? s.length <= 254 && /^[^\s@]+@[^\s@]+$/.test(s) : giftNameValid(s);
}

/** Currency of a region (only the common store currencies). */
const REGION_CURRENCY: Record<string, string> = {
  US: "USD", GB: "GBP", CA: "CAD", AU: "AUD", NZ: "NZD", CH: "CHF", SE: "SEK", NO: "NOK", DK: "DKK", PL: "PLN", CZ: "CZK", HU: "HUF", RO: "RON",
  BG: "BGN", TR: "TRY", BR: "BRL", MX: "MXN", JP: "JPY", IN: "INR", ZA: "ZAR", SG: "SGD", HK: "HKD", KR: "KRW", UA: "UAH", IL: "ILS", AE: "AED",
  AT: "EUR", BE: "EUR", CY: "EUR", DE: "EUR", EE: "EUR", ES: "EUR", FI: "EUR", FR: "EUR", GR: "EUR", HR: "EUR", IE: "EUR", IT: "EUR", LT: "EUR",
  LU: "EUR", LV: "EUR", MT: "EUR", NL: "EUR", PT: "EUR", SI: "EUR", SK: "EUR",
};

/** First browser language whose region maps to a currency the store sells in. */
export function detectCurrency(languages: readonly string[], options: readonly string[]): string | null {
  for (const l of languages) {
    const region = l.split(/[-_]/)[1]?.toUpperCase();
    const c = region ? REGION_CURRENCY[region] : undefined;
    if (c && options.includes(c)) return c;
  }
  return null;
}

/** Enabled currencies that at least one catalog product is priced in (auto-detect only picks these). */
export function pricedCurrencies(enabled: readonly string[], products: readonly { prices: readonly { currency: string }[] }[]): string[] {
  const sold = new Set(products.flatMap((p) => p.prices.map((x) => x.currency)));
  return enabled.filter((c) => sold.has(c));
}

/** Public rules for the store (empty while the module is off). */
export function usePackageRules() {
  const { enabled } = useModules();
  const on = enabled.has("gaming_storefront");
  const q = useQuery({ queryKey: ["public", "package-rules"], queryFn: () => api<{ items: PublicRule[] }>("/public/storefront/package-rules"), enabled: on, staleTime: 30_000 });
  const byId = new Map((q.data?.items ?? []).map((r) => [r.product_id, r]));
  return (id: string) => byId.get(id);
}
