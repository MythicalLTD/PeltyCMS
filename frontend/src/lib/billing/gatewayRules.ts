export interface CurrencyLimit { min: string | null; max: string | null; fee_fixed: string }
export interface GatewayRules { fee_percent: string; currencies: string[]; limits: Record<string, CurrencyLimit> }
export interface LimitRow { currency: string; min: string; max: string; fee_fixed: string }
export interface RuleForm { fee_percent: string; currencies: string; limits: LimitRow[] }

export function fromRules(r: GatewayRules): RuleForm {
  return {
    fee_percent: String(r.fee_percent ?? "0"),
    currencies: (r.currencies ?? []).join(", "),
    limits: Object.entries(r.limits ?? {}).map(([currency, l]) => ({ currency, min: l.min ?? "", max: l.max ?? "", fee_fixed: String(l.fee_fixed ?? "") })),
  };
}

const optionalAmount = (v: string) => (v.trim() === "" ? null : v.trim());

/** Form → API body. Amounts stay strings (exact decimals, parsed server-side). */
export function toRules(f: RuleForm): GatewayRules {
  const limits: Record<string, CurrencyLimit> = {};
  for (const l of f.limits) {
    const c = l.currency.trim().toUpperCase();
    if (c) limits[c] = { min: optionalAmount(l.min), max: optionalAmount(l.max), fee_fixed: l.fee_fixed.trim() || "0" };
  }
  return {
    fee_percent: f.fee_percent.trim() || "0",
    currencies: f.currencies.split(",").map((c) => c.trim().toUpperCase()).filter(Boolean),
    limits,
  };
}
