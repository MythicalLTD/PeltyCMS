export type FieldKind = "text" | "textarea" | "select" | "checkbox" | "email" | "number";
export interface FormField { key: string; label: string; kind: FieldKind; required: boolean; options: string[] }
export interface OrderForm {
  id: string; slug: string; name: string; layout: string; product_ids: string[]; group_ids: string[];
  steps: string[]; fields: FormField[]; redirect_url: string | null; active: boolean;
}
export type FormDraft = Omit<OrderForm, "id">;
export interface FreeDomain { free_domain: "none" | "once" | "on_renewal"; free_domain_tlds: string[]; free_domain_cycles: string[] }

export const blankForm: FormDraft = { slug: "", name: "", layout: "cards", product_ids: [], group_ids: [], steps: [], fields: [], redirect_url: null, active: true };

/** Split a comma/space separated list, trimming and dropping blanks and duplicates. */
export function splitList(v: string, lower = true): string[] {
  const out = v.split(/[\s,]+/).map((x) => (lower ? x.trim().toLowerCase() : x.trim())).filter(Boolean);
  return [...new Set(out)];
}

/** Normalise TLD input: ".com, RO" -> ["com", "ro"]. */
export const tldList = (v: string) => splitList(v).map((x) => x.replace(/^\.+/, "")).filter(Boolean);

/** API payload for a form: empty redirect becomes null, steps must end with checkout when set. */
export function formPayload(d: FormDraft): FormDraft {
  const steps = d.steps.length > 0 && !d.steps.includes("checkout") ? [...d.steps, "checkout"] : d.steps;
  return { ...d, slug: d.slug.trim().toLowerCase(), redirect_url: d.redirect_url?.trim() || null, steps, fields: d.fields.map((f) => ({ ...f, options: f.kind === "select" ? f.options : [] })) };
}

/** Keys of required fields still missing an answer. */
export function missingAnswers(fields: FormField[], answers: Record<string, unknown>): string[] {
  return fields.filter((f) => f.required && (answers[f.key] === undefined || answers[f.key] === "" || (f.kind === "checkbox" && answers[f.key] !== true))).map((f) => f.key);
}

/** Products a form offers: everything when the form lists none. */
export function formProducts<T extends { id: string }>(form: Pick<OrderForm, "product_ids"> | null, items: T[]): T[] {
  return !form || form.product_ids.length === 0 ? items : items.filter((p) => form.product_ids.includes(p.id));
}
