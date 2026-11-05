export type ImportKind = "clients" | "services" | "invoices";
export const IMPORT_KINDS: ImportKind[] = ["clients", "services", "invoices"];
export interface ImportField { key: string; required: boolean }
export interface Analysis { headers: string[]; preview: string[][]; rows: number; fields: ImportField[]; mapping: Record<string, string> }
export interface ImportReport { total: number; created: number; skipped: number; error_count: number; committed: boolean; errors: { row: number; field: string | null; code: string }[] }

/** Required fields that have no column picked. */
export function missingRequired(fields: ImportField[], mapping: Record<string, string>): string[] {
  return fields.filter((f) => f.required && !mapping[f.key]).map((f) => f.key);
}

/** Drop unmapped entries so the API only sees real choices. */
export function cleanMapping(mapping: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(mapping).filter(([, v]) => v));
}
