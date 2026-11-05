/** Accounting export helpers. Money stays as decimal strings from the API. */
export interface TaxRow { period: string; country: string; currency: string; rate: string; reverse_charge: boolean; documents: number; net: string; tax: string; gross: string }
export interface TaxTotal { currency: string; net: string; tax: string; gross: string }
export interface TaxReport { from: string; to: string; rows: TaxRow[]; totals: TaxTotal[] }

export type ExportKind = "tax" | "invoices" | "payments";

/** Same-origin download URL (session cookie authenticates the GET). */
export function exportUrl(kind: ExportKind, from: string, to: string): string {
  const q = new URLSearchParams({ from, to }).toString();
  const path = kind === "tax" ? "/admin/reports/tax.csv" : `/admin/exports/${kind}.csv`;
  return `/api/v1${path}?${q}`;
}

/** "24.0000" -> "24", "9.50" -> "9.5", display only, no float conversion. */
export function trimRate(rate: string): string {
  if (!rate.includes(".")) return rate;
  return rate.replace(/0+$/, "").replace(/\.$/, "");
}
