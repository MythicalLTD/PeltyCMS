export interface LegalEntity {
  id: string; name: string; address: string; tax_id: string; bank_name: string; iban: string; swift: string;
  footer: string; terms: string; invoice_prefix: string; credit_prefix: string; number_format: string;
  due_days: number | null; is_default: boolean; created_at: string; next_number: string;
}
export type EntityDraft = Omit<LegalEntity, "id" | "is_default" | "created_at" | "next_number" | "due_days"> & { due_days: string };

export const emptyEntity: EntityDraft = { name: "", address: "", tax_id: "", bank_name: "", iban: "", swift: "", footer: "", terms: "", invoice_prefix: "", credit_prefix: "", number_format: "{prefix}-{year}-{seq:5}", due_days: "" };

/** Client-side mirror of the server's number formatter (preview only). */
export function formatNumber(fmt: string, prefix: string, year: number, seq: number): string | null {
  let hasSeq = false;
  let bad = false;
  const out = fmt.replace(/\{([^}]*)\}/g, (_, tok: string) => {
    if (tok === "prefix") return prefix;
    if (tok === "year") return String(year).padStart(4, "0");
    if (tok === "yy") return String(((year % 100) + 100) % 100).padStart(2, "0");
    if (tok === "seq") { hasSeq = true; return String(seq); }
    const m = /^seq:(\d+)$/.exec(tok);
    const n = m ? Number(m[1]) : 0;
    if (n < 1 || n > 12) { bad = true; return ""; }
    hasSeq = true;
    return String(seq).padStart(n, "0");
  });
  if (bad || !hasSeq || /[{}\s]/.test(out) || out.length > 64) return null;
  return out;
}

const PREFIX = /^[A-Za-z0-9_/-]{1,12}$/;

/** Payload or null when invalid. */
export function entityPayload(d: EntityDraft) {
  if (!d.name.trim()) return null;
  if (!PREFIX.test(d.invoice_prefix.trim()) || !PREFIX.test(d.credit_prefix.trim())) return null;
  if (d.invoice_prefix.trim().toLowerCase() === d.credit_prefix.trim().toLowerCase()) return null;
  const fmt = d.number_format.trim() || emptyEntity.number_format;
  if (!fmt.includes("{prefix}") || !formatNumber(fmt, "P", 2026, 1)) return null;
  const due = d.due_days.trim();
  if (due && (!/^\d{1,3}$/.test(due) || Number(due) > 365)) return null;
  return { ...d, number_format: fmt, invoice_prefix: d.invoice_prefix.trim(), credit_prefix: d.credit_prefix.trim(), due_days: due ? Number(due) : null };
}
