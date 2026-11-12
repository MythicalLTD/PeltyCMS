import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "../api";
import { newUuid } from "../ids";

export interface StoreLine { product_id: string; cycle: string; config: Record<string, string | number>; addons: string[]; pay_amount?: string; gift_to?: string }
export interface StoreCart { id: string; email: string | null; currency: string; country: string; items: StoreLine[]; coupon: string | null; creator_code?: string | null; order_id: string | null }
export interface TaxPart { name: string; rate: string; compound: boolean; amount: string }
export interface PreviewLine {
  product_id: string; product_name: string; cycle: string; amount: string; setup_fee: string; labels: string[]; discount: string; sale_discount?: string; sale?: string | null; creator_discount?: string; bundle_discount?: string; upgrade_credit?: string; upgrade_from?: string | null;
  addons: { addon_id: string; name: string; cycle: string; amount: string; setup_fee: string }[];
}
export interface CartPreview {
  currency: string; lines: PreviewLine[]; subtotal: string; discount: string; group_discount: string; sale_discount?: string; creator_discount?: string; bundle?: string | null; bundle_discount?: string; bundle_owned?: number; bundle_size?: number; upgrade_credit?: string; tax: string; taxes: TaxPart[];
  total: string; inclusive: boolean; coupon: string | null; coupon_error: string | null; creator_code?: string | null; creator_error?: string | null;
}
export interface CartView { cart: StoreCart; preview: CartPreview | null; error: string | null }
export interface Addon { id: string; name: string; description: string; prices: { currency: string; cycle: string; amount: string; setup_fee: string }[] }
export interface OrderField { id: string; name: string; description: string; kind: string; choices: string[]; pattern: string; required: boolean; identity_key?: string | null; module_key?: string | null; field_key?: string | null }
export interface Extras { addons: Addon[]; fields: OrderField[] }
export interface Price { currency: string; cycle: string; amount: string; setup_fee: string }

const KEY = "pelty.cart";
/** Id of the last cart the server accepted ("" = the current id was never saved). */
const SAVED = "pelty.cart.saved";

/** Ask the server for this cart only if it may exist there: a shared `?c=` link,
 *  the last saved id, or an id from before this marker existed. Otherwise every
 *  page view of a new visitor logged a 404 and minted a new id. */
export function mayExistOnServer(storage: Pick<Storage, "getItem">, id: string, search: string): boolean {
  if (new URLSearchParams(search).get("c") === id) return true;
  const saved = storage.getItem(SAVED);
  return saved === null || saved === id;
}

/** Cart token: `?c=` (recovery link) wins, then the stored one, else a new v4 UUID. */
export function cartIdFrom(storage: Pick<Storage, "getItem" | "setItem">, search: string, gen: () => string = newUuid): string {
  const re = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  const fromUrl = new URLSearchParams(search).get("c");
  const stored = storage.getItem(KEY);
  const id = fromUrl && re.test(fromUrl) ? fromUrl : stored && re.test(stored) ? stored : gen();
  if (id !== stored) storage.setItem(KEY, id);
  return id;
}

/** Forget the cart after checkout so the next visit starts empty. */
export function resetCartId(storage: Pick<Storage, "setItem">, gen: () => string = newUuid) {
  storage.setItem(KEY, gen());
  storage.setItem(SAVED, "");
}

/** Cheapest price in a currency ("from" price on product cards). */
export function fromPrice(prices: Price[], currency: string): Price | undefined {
  return prices.filter((p) => p.currency === currency).toSorted((a, b) => Number(a.amount) - Number(b.amount))[0];
}

/** Addon price for a cycle; one-time addons fall back to their only price. */
export function addonPrice(a: Addon, currency: string, cycle: string) {
  return a.prices.find((p) => p.currency === currency && p.cycle === cycle) ?? a.prices.find((p) => p.currency === currency && p.cycle === "one_time");
}

/** Client-side check of custom order fields; the server validates again. */
export function fieldErrors(fields: OrderField[], values: Record<string, string>): Record<string, "required" | "pattern" | "email" | "number"> {
  const out: Record<string, "required" | "pattern" | "email" | "number"> = {};
  for (const f of fields) {
    const raw = values[f.id] ?? "";
    const v = f.kind === "password" ? raw : raw.trim();
    if (!v.trim() || (f.kind === "checkbox" && v === "false")) { if (f.required) out[f.id] = "required"; continue; }
    if (f.kind === "email" && !emailOk(v)) { out[f.id] = "email"; continue; }
    if (f.kind === "number" && !/^[-+]?(\d+\.?\d*|\.\d+)$/.test(v)) { out[f.id] = "number"; continue; }
    if (f.pattern) {
      try { if (!new RegExp(`^(?:${f.pattern})$`).test(v)) out[f.id] = "pattern"; } catch { /* server decides */ }
    }
  }
  return out;
}

/** Same shape check as the server: `local@domain.tld`, no spaces. */
export function emailOk(v: string): boolean {
  const at = v.indexOf("@");
  if (at <= 0 || at !== v.lastIndexOf("@") || at > 64 || v.length > 254 || /\s/.test(v)) return false;
  const d = v.slice(at + 1);
  return d.includes(".") && !d.startsWith(".") && !d.endsWith(".") && !d.includes("..");
}

/** Answers the dynamic checkout form may see: passwords stay on the page until the order is placed. */
export function selectedForConfig(fields: OrderField[], values: Record<string, string>): Record<string, string> {
  const secret = new Set(fields.filter((f) => f.kind === "password").map((f) => f.id));
  return Object.fromEntries(Object.entries(values).filter(([k]) => !secret.has(k)));
}

/** Input type / autofill hints for a checkout field. */
export function inputProps(f: OrderField): { type: string; autoComplete?: string; inputMode?: "decimal" | "email" | "numeric" } {
  const key = f.module_key ?? f.field_key ?? "";
  switch (f.kind) {
    case "email": return { type: "email", autoComplete: "email", inputMode: "email" };
    case "password": return { type: "password", autoComplete: "new-password" };
    case "number": return { type: "text", inputMode: "decimal" };
    default:
      if (f.identity_key === "steamid" || f.identity_key === "discord_id") return { type: "text", inputMode: "numeric", autoComplete: "off" };
      if (key.includes("username") || f.identity_key === "username") return { type: "text", autoComplete: "username" };
      return { type: "text" };
  }
}

export const lineCount = (c: StoreCart | null | undefined) => c?.items.length ?? 0;

export type CartSave = Omit<StoreCart, "id" | "order_id"> & { locale: string };

/** The PUT body the server accepts. The server rejects unknown fields, so a
 * cart read back from the API (id, customer_id, updated_at, ...) must be
 * reduced to exactly these keys. */
export function cartBody(c: CartSave) {
  return { currency: c.currency, country: c.country ?? "", items: c.items, coupon: c.coupon?.trim() || null, creator_code: c.creator_code?.trim() || null, email: c.email || null, locale: c.locale };
}

/** Ids minted in this tab: a 404 for them just means "not saved yet". */
const fresh = new Set<string>();

/** Server says the stored cart is gone/unusable (404/422/403/410): forget it, start a fresh one. */
export function isStaleCartError(e: unknown) {
  return e instanceof ApiError && [403, 404, 410, 422].includes(e.status);
}

export function dropStaleCart(storage: Pick<Storage, "setItem">, gen: () => string = newUuid) {
  const id = gen();
  fresh.add(id);
  storage.setItem(KEY, id);
  storage.setItem(SAVED, "");
  return id;
}

/** Shared cart state for the public store: load, save, and the current id. */
export function useStoreCart(search = "") {
  const qc = useQueryClient();
  const id = cartIdFrom(localStorage, search, () => { const g = newUuid(); fresh.add(g); localStorage.setItem(SAVED, ""); return g; });
  const q = useQuery({
    queryKey: ["store", "cart", id],
    queryFn: async () => {
      if (!mayExistOnServer(localStorage, id, search)) return null;
      try { return await api<CartView>(`/store/carts/${id}`); } catch (e) {
        if (!isStaleCartError(e)) throw e;
        if (!fresh.has(id)) { dropStaleCart(localStorage); qc.removeQueries({ queryKey: ["store", "cart", id] }); }
        return null;
      }
    },
  });
  const save = useMutation({
    mutationFn: (c: CartSave) => api<CartView>(`/store/carts/${id}`, { method: "PUT", body: cartBody(c) }),
    onSuccess: (v) => { fresh.delete(id); localStorage.setItem(SAVED, id); qc.setQueryData(["store", "cart", id], v); },
    onError: (e) => { if (isStaleCartError(e) && (e as ApiError).status !== 422) { dropStaleCart(localStorage); qc.invalidateQueries({ queryKey: ["store", "cart"] }); } },
  });
  return { id, view: q.isError ? null : q.data ?? null, loading: q.isLoading, save };
}

/** Progress on the order status page: 0 placed, 1 paid, 2 approved, 3 services active; -1 cancelled/fraud. */
export function orderStage(status: string, invoiceStatus: string | null | undefined, services: { status: string }[]): number {
  if (status === "cancelled" || status === "fraud") return -1;
  const paid = invoiceStatus === "paid";
  const active = services.length > 0 && services.every((s) => s.status === "active");
  if (active) return 3;
  if (status === "active" || status === "approved") return paid ? 2 : 0;
  return paid ? 1 : 0;
}
