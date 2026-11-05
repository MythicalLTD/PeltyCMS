export interface Tld {
  tld: string;
  currency: string;
  register_price: string;
  renew_price: string;
  transfer_price: string;
  min_years: number;
  max_years: number;
  registrar: string;
  active: boolean;
  grace_days: number;
  grace_fee: string;
  redemption_days: number;
  redemption_fee: string;
  id_protection_price: string;
  dns_management_price: string;
  epp_required: boolean;
  tld_group: "" | "hot" | "new" | "sale";
  sort_order: number;
}

export interface Contact {
  name: string;
  organization?: string;
  email: string;
  phone?: string;
  address?: string;
  city?: string;
  postal_code?: string;
  country?: string;
}

export interface Domain {
  id: string;
  customer_id: string;
  name: string;
  tld: string;
  kind: "register" | "transfer";
  status: string;
  registrar: string;
  years: number;
  registered_on: string | null;
  expires_on: string | null;
  auto_renew: boolean;
  nameservers: string[];
  contact: Partial<Contact>;
  privacy: boolean;
  invoice_id: string | null;
  last_error: string | null;
  created_at: string;
  dns_management?: boolean;
  registrar_lock?: boolean;
  admin_notes?: string;
  synced_at?: string | null;
  premium?: boolean;
  premium_renew_price?: string | null;
  extra_fields?: Record<string, string | boolean>;
}

export interface Renewal { id: string; years: number; for_expiry: string; invoice_id: string | null; applied: boolean }
export interface RegistrantVerification { domain_id: string; email: string; status: "pending" | "verified" | "overdue"; deadline: string; sent_at: string; reminders: number; verified_at: string | null }
export interface DomainDetail { domain: Domain; renewals: Renewal[]; registrant_verification?: RegistrantVerification | null }
export interface CheckResult { name: string; tld: string; supported: boolean; available: boolean; pricing?: Tld; premium?: { register_price: string; renew_price: string } | null; premium_error?: string | null }

export interface TldField { id?: string; tld: string; key: string; label: string; kind: "text" | "select" | "checkbox"; options: string[]; required: boolean; max_length: number; sort_order: number }

/** TLD pricing with premium register/renew prices applied (transfers keep standard pricing). */
export function effectivePricing(p: Tld, premium?: CheckResult["premium"]): Tld {
  return premium ? { ...p, register_price: premium.register_price, renew_price: premium.renew_price } : p;
}

/** Keys of required additional fields that are still empty. */
export function missingFields(fields: TldField[], values: Record<string, string | boolean>): string[] {
  return fields.filter((f) => f.required && (f.kind === "checkbox" ? values[f.key] !== true : !String(values[f.key] ?? "").trim())).map((f) => f.key);
}

export const DOMAIN_STATUSES = ["pending", "active", "grace", "redemption", "expired", "transferring", "transferred_away", "cancelled", "fraud", "failed"] as const;
export const TLD_DEFAULTS = { grace_days: 30, grace_fee: "0.00", redemption_days: 30, redemption_fee: "0.00", id_protection_price: "0.00", dns_management_price: "0.00", epp_required: true, tld_group: "" as const, sort_order: 0 };
export const RENEWABLE = ["active", "grace", "redemption", "expired"];
export const TLD_GROUPS = ["", "hot", "new", "sale"] as const;

export interface DnsRecord { id?: string; rtype: string; host: string; value: string; ttl: number; priority?: number | null }
export const DNS_TYPES = ["A", "AAAA", "CNAME", "MX", "TXT", "SRV", "CAA", "NS", "URL", "FRAME"] as const;
export const needsPriority = (t: string) => t === "MX" || t === "SRV";

export interface PublicCheck { name: string; supported: boolean; available: boolean; pricing?: Tld | null }
export interface WhoisInfo { registered: boolean; registrar?: string | null; created?: string | null; expires?: string | null; status: string[]; nameservers: string[] }

const decimalToInt = (x: string, s: number) => {
  const neg = x.trim().startsWith("-");
  const [i, f = ""] = x.trim().replace(/^[-+]/, "").split(".");
  const v = BigInt((i || "0") + f.padEnd(s, "0"));
  return neg ? -v : v;
};

/** Exact add-on sum for one year: base + ID protection + DNS (string decimals). */
export function addDecimal(a: string, b: string): string {
  const sa = (a.split(".")[1] ?? "").length, sb = (b.split(".")[1] ?? "").length, sc = Math.max(sa, sb);
  const v = decimalToInt(a, sc) + decimalToInt(b, sc);
  const neg = v < 0n; let s = (neg ? -v : v).toString().padStart(sc + 1, "0");
  if (sc > 0) s = `${s.slice(0, -sc)}.${s.slice(-sc)}`;
  return (neg ? "-" : "") + s;
}

/** Exact decimal × integer on strings (no float rounding). */
export function mulDecimal(amount: string, n: number): string {
  const neg = amount.trim().startsWith("-");
  const [i, f = ""] = amount.trim().replace(/^[-+]/, "").split(".");
  const scale = f.length;
  const v = BigInt((i || "0") + f) * BigInt(Math.trunc(n));
  let s = v.toString().padStart(scale + 1, "0");
  if (scale > 0) s = `${s.slice(0, -scale)}.${s.slice(-scale)}`;
  return (neg && v !== 0n ? "-" : "") + s;
}

/** Order price: transfers are a flat one-year fee, registrations per year. */
export function orderPrice(t: Tld, kind: "register" | "transfer", years: number): string {
  return kind === "transfer" ? t.transfer_price : mulDecimal(t.register_price, years);
}

/** Order total incl. paid add-ons (per year; transfers count one year). */
export function orderTotal(t: Tld, kind: "register" | "transfer", years: number, privacy: boolean, dns: boolean): string {
  const n = kind === "transfer" ? 1 : years;
  let total = orderPrice(t, kind, years);
  if (privacy && t.id_protection_price) total = addDecimal(total, mulDecimal(t.id_protection_price, n));
  if (dns && t.dns_management_price) total = addDecimal(total, mulDecimal(t.dns_management_price, n));
  return total;
}

/** One nameserver per line or comma-separated; lowercase, deduped. */
export function parseNameservers(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/[\s,]+/)) {
    const s = raw.trim().toLowerCase().replace(/\.$/, "");
    if (s && !out.includes(s)) out.push(s);
  }
  return out;
}

/** Whole days from `today` (YYYY-MM-DD) until `date`; null when unknown. */
export function daysUntil(date: string | null, today: string): number | null {
  if (!date) return null;
  const ms = Date.parse(`${date}T00:00:00Z`) - Date.parse(`${today}T00:00:00Z`);
  return Number.isFinite(ms) ? Math.round(ms / 86_400_000) : null;
}

export function statusVariant(s: string): "default" | "secondary" | "destructive" | "outline" {
  if (s === "active") return "default";
  if (s === "expired" || s === "failed") return "destructive";
  if (s === "cancelled") return "outline";
  return "secondary";
}

export function yearsRange(t: Tld): number[] {
  const out: number[] = [];
  for (let y = t.min_years; y <= t.max_years; y++) out.push(y);
  return out;
}
