/** E-invoicing (UBL) and debit-note helpers. */
export const ublUrl = (scope: "client" | "admin", id: string) => `/api/v1/${scope}/invoices/${id}/ubl`;

export interface EInvoiceSettings {
  profile: "peppol" | "cius_ro";
  seller_name: string; seller_vat_id: string; seller_company_id: string; seller_legal_info: string;
  seller_street: string; seller_city: string; seller_subdivision: string; seller_postal_code: string; seller_country: string;
  seller_email: string; seller_phone: string; endpoint_id: string; endpoint_scheme: string; iban: string; bic: string; payment_means_code: string;
}

export const EMPTY_EINVOICE: EInvoiceSettings = {
  profile: "peppol", seller_name: "", seller_vat_id: "", seller_company_id: "", seller_legal_info: "", seller_street: "", seller_city: "",
  seller_subdivision: "", seller_postal_code: "", seller_country: "", seller_email: "", seller_phone: "", endpoint_id: "", endpoint_scheme: "",
  iban: "", bic: "", payment_means_code: "30",
};

export interface DebitLine { description: string; quantity: string; unit_price: string }

const POSITIVE = /^(?:\d+)(?:\.\d+)?$/;

/** Validate debit-note lines; returns an i18n key or null. */
export function checkDebitLines(lines: DebitLine[]): "debit.invalid_lines" | "debit.invalid_amount" | null {
  if (lines.length === 0 || lines.length > 50) return "debit.invalid_lines";
  for (const l of lines) {
    if (!l.description.trim()) return "debit.invalid_lines";
    const q = l.quantity.trim(), p = l.unit_price.trim();
    if (!POSITIVE.test(q) || !POSITIVE.test(p) || Number(q) <= 0 || Number(p) <= 0) return "debit.invalid_amount";
  }
  return null;
}

/** Client-side estimate (net, before tax) of a debit note for display only. */
export function debitNet(lines: DebitLine[]): string {
  const cents = lines.reduce((s, l) => {
    const q = Number(l.quantity), p = Number(l.unit_price);
    return Number.isFinite(q * p) ? s + Math.round(q * p * 100) : s;
  }, 0);
  return (cents / 100).toFixed(2);
}

/** Title key for an invoice-like document by kind. */
export const docTitleKey = (kind: string): "pdf.credit_note" | "pdf.debit_note" | "billing.invoice" => kind === "credit_note" ? "pdf.credit_note" : kind === "debit_note" ? "pdf.debit_note" : "billing.invoice";
