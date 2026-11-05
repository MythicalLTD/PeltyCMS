/** Customer credit balance helpers. Amounts are decimal strings; never parsed to floats for comparison. */
export interface CreditBalance { currency: string; amount: string }
export interface CreditEntry { id: string; currency: string; amount: string; kind: string; invoice_id: string | null; note: string; created_at: string }
export interface CreditState { balances: CreditBalance[]; entries: CreditEntry[] }

const DEC = /^-?\d+(\.\d+)?$/;

/** Compare two decimal strings exactly: -1, 0 or 1. */
export function cmpDec(a: string, b: string): number {
  if (!DEC.test(a) || !DEC.test(b)) throw new Error("not a decimal");
  const neg = (s: string) => s.startsWith("-");
  const norm = (s: string) => {
    const [i = "", f = ""] = s.replace(/^-/, "").split(".");
    return { i: i.replace(/^0+(?=\d)/, ""), f: f.replace(/0+$/, "") };
  };
  const na = norm(a), nb = norm(b);
  const zero = (n: { i: string; f: string }) => n.i === "0" && n.f === "";
  const sa = zero(na) ? 0 : neg(a) ? -1 : 1, sb = zero(nb) ? 0 : neg(b) ? -1 : 1;
  if (sa !== sb) return sa < sb ? -1 : 1;
  if (sa === 0) return 0;
  let c = na.i.length !== nb.i.length ? (na.i.length < nb.i.length ? -1 : 1) : na.i < nb.i ? -1 : na.i > nb.i ? 1 : 0;
  if (c === 0) {
    const w = Math.max(na.f.length, nb.f.length);
    const fa = na.f.padEnd(w, "0"), fb = nb.f.padEnd(w, "0");
    c = fa < fb ? -1 : fa > fb ? 1 : 0;
  }
  return sa < 0 ? -c : c;
}

/** Available credit in a currency ("0" when none). */
export function creditIn(state: CreditState | undefined, currency: string): string {
  return state?.balances.find((b) => b.currency === currency)?.amount ?? "0";
}

/** What applying credit would pay now: the smaller of credit and the invoice balance. */
export function applicable(credit: string, balance: string): string {
  if (cmpDec(credit, "0") <= 0 || cmpDec(balance, "0") <= 0) return "0";
  return cmpDec(credit, balance) < 0 ? credit : balance;
}

/** A signed staff adjustment amount: non-zero, at most `exp` decimals. */
export function isAdjustment(s: string, exp = 2): boolean {
  const m = /^-?\d+(?:\.(\d+))?$/.exec(s.trim());
  return !!m && (m[1]?.length ?? 0) <= exp && cmpDec(s.trim(), "0") !== 0;
}

export const pdfUrl = (scope: "client" | "admin", id: string) => `/api/v1/${scope}/invoices/${id}/pdf`;
