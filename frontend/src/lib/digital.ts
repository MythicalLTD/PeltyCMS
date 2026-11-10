/** Digital products: settings, versions, licenses (module `digital_products`). */
export interface DigitalSettings {
  product_id?: string; license_enabled: boolean; max_activations: number; bind_domain: boolean; bind_ip: boolean; bind_hwid: boolean;
  license_days: number | null; link_ttl_secs: number; escrow_asset: string;
}
export interface DigitalProductRow { product_id: string; name: string; license_enabled: boolean; max_activations: number; versions: number; licenses: number; latest: string | null }
export interface DigitalVersion { id: string; product_id: string; version: string; changelog: string; filename: string; size_bytes: number; sha256: string; downloads: number; created_at: string }
export interface License {
  id: string; product_id: string; product: string; customer_id: string; service_id: string | null; key_hint: string; status: string;
  max_activations: number; activations: number; expires_at: string | null; note: string; created_at: string;
}
export interface Activation { id: string; domain: string; ip: string; hwid: string; first_seen: string; last_seen: string }
export interface LicenseView { license: License; key: string | null; activations: Activation[] }
export interface Grant { id: string; product_id: string; customer_id: string; service_id: string | null; asset: string; identity: string; status: string; created_at: string }

export const DEFAULT_DIGITAL: DigitalSettings = { license_enabled: true, max_activations: 1, bind_domain: false, bind_ip: false, bind_hwid: false, license_days: null, link_ttl_secs: 900, escrow_asset: "" };

/** "3 / 5", or "3 / ∞" when unlimited (0). */
export function activationLabel(used: number, max: number): string {
  return `${used} / ${max === 0 ? "∞" : max}`;
}

/** Human file size: 1536 → "1.5 KB". */
export function fileSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/** What an activation is bound to, e.g. "example.com · 1.2.3.4". */
export function activationTarget(a: Pick<Activation, "domain" | "ip" | "hwid">): string {
  return [a.domain, a.ip, a.hwid && `HWID ${a.hwid.slice(0, 12)}`].filter(Boolean).join(" · ") || "—";
}
