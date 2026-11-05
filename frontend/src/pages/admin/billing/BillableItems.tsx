import { useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router";
import { toast } from "sonner";
import { ArrowClockwiseIcon, CalendarCheckIcon, PackageIcon, PlusIcon, ReceiptIcon, TrashIcon } from "@phosphor-icons/react";
import { CustomerPicker } from "@/components/admin/CustomerPicker";
import { Field, FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { renewable } from "@/components/billing/RenewNowButton";
import { api, ApiError } from "@/lib/api";
import { formatMoney, type InvoiceDetail, type Product, type Service } from "@/lib/billing/billing";
import { formatDate } from "@/lib/format";
import { fromPrice } from "@/lib/store/storefront";
import { useI18n } from "@/lib/i18n";

export const ACTIONS = ["hold", "next_invoice", "invoice_now", "recurring"] as const;
export const RECUR_CYCLES = ["monthly", "quarterly", "semiannual", "annual"] as const;
export interface BillableItem { id: string; customer_id: string; customer_email: string; description: string; quantity: string; unit_price: string; currency: string; invoice_action: string; recur_cycle: string | null; next_due_date: string | null; recur_remaining: number | null; invoice_id: string | null; invoice_number: string | null; created_at: string }
export interface LineDraft { description: string; quantity: string; unit_price: string; tax_rate?: string }
export interface BillableDraft extends LineDraft { customer_id: string; invoice_action: string; recur_cycle: string; next_due_date: string; recur_times: string }

const num = (s: string) => /^\d+(\.\d{1,4})?$/.test(s.trim());
const taxOk = (s: string | undefined) => !s?.trim() || (/^\d+(\.\d{1,4})?$/.test(s.trim()) && Number(s) >= 0 && Number(s) <= 100);

/** Build the POST body; returns null when the draft is incomplete. */
export function billablePayload(d: BillableDraft) {
  if (!d.customer_id || !d.description.trim() || !num(d.unit_price) || !num(d.quantity) || Number(d.quantity) <= 0) return null;
  const rec = d.invoice_action === "recurring";
  if (rec && (!d.next_due_date || !d.recur_cycle)) return null;
  return {
    customer_id: d.customer_id, description: d.description.trim(), quantity: d.quantity.trim(), unit_price: d.unit_price.trim(), invoice_action: d.invoice_action,
    ...(rec ? { recur_cycle: d.recur_cycle, next_due_date: d.next_due_date, ...(d.recur_times.trim() ? { recur_times: Number(d.recur_times) } : {}) } : {}),
  };
}

/** Validated invoice lines (null if any line is incomplete). Empty tax = auto at finalize. */
export function linesPayload(lines: LineDraft[]) {
  if (lines.length === 0 || lines.some((l) => !l.description.trim() || !num(l.unit_price) || !num(l.quantity) || Number(l.quantity) <= 0 || !taxOk(l.tax_rate))) return null;
  return lines.map((l) => ({
    description: l.description.trim(),
    quantity: l.quantity.trim(),
    unit_price: l.unit_price.trim(),
    ...(l.tax_rate?.trim() ? { tax_rate: l.tax_rate.trim() } : {}),
  }));
}

/** Rough net total for the create dialog (ignores tax rules). */
export function linesNet(lines: LineDraft[]): number {
  return lines.reduce((sum, l) => {
    if (!num(l.quantity) || !num(l.unit_price)) return sum;
    return sum + Number(l.quantity) * Number(l.unit_price);
  }, 0);
}

const errMsg = (e: unknown, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null);

export function LineEditor({ lines, onChange, showTax }: { lines: LineDraft[]; onChange: (l: LineDraft[]) => void; showTax?: boolean }) {
  const { t } = useI18n();
  const set = (i: number, p: Partial<LineDraft>) => onChange(lines.map((l, j) => (j === i ? { ...l, ...p } : l)));
  const cols = showTax ? "sm:grid-cols-[1fr_4.5rem_6.5rem_5rem_auto]" : "sm:grid-cols-[1fr_5rem_7rem_auto]";
  return (
    <div className="grid gap-2">
      <div className={`hidden gap-2 text-xs font-medium text-muted-foreground sm:grid ${cols}`}>
        <span>{t("billable.description")}</span>
        <span>{t("billable.quantity")}</span>
        <span>{t("billable.unit_price")}</span>
        {showTax && <span>{t("billable.tax_rate")}</span>}
        <span className="sr-only">{t("common.delete")}</span>
      </div>
      {lines.map((l, i) => (
        <div key={i} className={`grid grid-cols-[1fr_auto] gap-2 ${cols}`}>
          <Input aria-label={t("billable.description")} placeholder={t("billable.description")} className="col-span-2 sm:col-span-1" value={l.description} onChange={(e) => set(i, { description: e.target.value })} />
          <Input aria-label={t("billable.quantity")} inputMode="decimal" value={l.quantity} onChange={(e) => set(i, { quantity: e.target.value })} />
          <Input aria-label={t("billable.unit_price")} placeholder={t("billable.unit_price")} inputMode="decimal" value={l.unit_price} onChange={(e) => set(i, { unit_price: e.target.value })} />
          {showTax && <Input aria-label={t("billable.tax_rate")} placeholder={t("billable.tax_auto")} inputMode="decimal" value={l.tax_rate ?? ""} onChange={(e) => set(i, { tax_rate: e.target.value })} />}
          <Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={lines.length === 1} onClick={() => onChange(lines.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
        </div>
      ))}
      <Button size="sm" variant="outline" className="justify-self-start" onClick={() => onChange([...lines, emptyLine()])}><PlusIcon aria-hidden />{t("billable.add_line")}</Button>
    </div>
  );
}

const emptyLine = (): LineDraft => ({ description: "", quantity: "1", unit_price: "", tax_rate: "" });

/** Manual invoice for a client: services, catalog lines, or free-form (WHMCS-style). */
export function CreateInvoiceDialog() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [open, setOpen] = useState(false);
  const [cust, setCust] = useState<{ id: string; currency: string }>({ id: "", currency: "EUR" });
  const [lines, setLines] = useState<LineDraft[]>([emptyLine()]);
  const [fin, setFin] = useState(true);
  const [dueDays, setDueDays] = useState("7");
  const [adminNotes, setAdminNotes] = useState("");
  const [clientNotes, setClientNotes] = useState("");
  const [productId, setProductId] = useState("");
  const items = linesPayload(lines);
  const dueOk = !fin || (/^\d+$/.test(dueDays.trim()) && Number(dueDays) >= 0 && Number(dueDays) <= 365);
  const currencies = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies"), enabled: open });
  const products = useQuery({ queryKey: ["admin", "products"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products"), enabled: open });
  const services = useQuery({
    queryKey: ["admin", "services", "invoice", cust.id],
    queryFn: () => api<{ items: Service[] }>(`/admin/services?per_page=50&customer_id=${encodeURIComponent(cust.id)}`),
    enabled: open && !!cust.id,
  });
  const pending = useQuery({
    queryKey: ["admin", "billable", "invoice", cust.id],
    queryFn: () => api<{ items: BillableItem[] }>(`/admin/billable-items?customer_id=${encodeURIComponent(cust.id)}`),
    enabled: open && !!cust.id,
  });
  const priced = (products.data?.items ?? []).filter((p) => p.status === "active" && fromPrice(p.prices, cust.currency));
  const renewables = (services.data?.items ?? []).filter(renewable);
  const pendingItems = (pending.data?.items ?? []).filter((b) => !b.invoice_id);
  const reset = () => {
    setCust({ id: "", currency: "EUR" }); setLines([emptyLine()]); setFin(true); setDueDays("7");
    setAdminNotes(""); setClientNotes(""); setProductId("");
  };
  const pushLine = (next: LineDraft) => {
    const blank = lines.length === 1 && !lines[0]!.description && !lines[0]!.unit_price;
    setLines(blank ? [next] : [...lines, next]);
  };
  const m = useMutation({
    mutationFn: () => api<InvoiceDetail>("/admin/invoices", {
      method: "POST",
      body: {
        customer_id: cust.id,
        currency: cust.currency,
        items,
        finalize: fin,
        ...(fin ? { due_days: Number(dueDays) } : {}),
        ...(adminNotes.trim() ? { admin_notes: adminNotes } : {}),
        ...(clientNotes.trim() ? { client_notes: clientNotes } : {}),
      },
    }),
    onSuccess: (r) => {
      setOpen(false); reset();
      void qc.invalidateQueries({ queryKey: ["admin", "invoices"] });
      void nav(`/admin/invoices/${r.invoice.id}`);
    },
  });
  const renew = useMutation({
    mutationFn: (id: string) => api<{ invoice_id: string }>(`/admin/services/${id}/renew-now`, { method: "POST" }),
    onSuccess: (r) => {
      toast.success(t("renew.created"));
      setOpen(false); reset();
      void qc.invalidateQueries({ queryKey: ["admin"] });
      void nav(`/admin/invoices/${r.invoice_id}`);
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const billPending = useMutation({
    mutationFn: () => api<{ invoice_id: string | null }>(`/admin/customers/${cust.id}/billable-items/invoice`, { method: "POST" }),
    onSuccess: (r) => {
      if (!r.invoice_id) { toast.message(t("billable.uninvoiced")); return; }
      toast.success(t("billable.invoice_now"));
      setOpen(false); reset();
      void qc.invalidateQueries({ queryKey: ["admin"] });
      void nav(`/admin/invoices/${r.invoice_id}`);
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const addProduct = (id: string) => {
    setProductId("");
    const p = priced.find((x) => x.id === id);
    const price = p && fromPrice(p.prices, cust.currency);
    if (!p || !price) return;
    pushLine({ description: p.name, quantity: "1", unit_price: price.amount, tax_rate: "" });
  };
  const addServiceLine = (s: Service) => {
    pushLine({
      description: t("billable.service_line", { product: s.product_name, due: formatDate(s.next_due_date, lang) }),
      quantity: "1",
      unit_price: s.recurring_amount,
      tax_rate: "",
    });
    if (s.currency && s.currency !== cust.currency) setCust({ ...cust, currency: s.currency });
  };
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) { reset(); m.reset(); } }}>
      <DialogTrigger asChild><Button size="sm"><PlusIcon aria-hidden />{t("billable.create_invoice")}</Button></DialogTrigger>
      <DialogContent className="flex max-h-[90dvh] flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl">
        <DialogHeader className="border-b px-6 py-4">
          <DialogTitle>{t("billable.create_invoice")}</DialogTitle>
          <DialogDescription>{t("billable.create_invoice_desc")}</DialogDescription>
        </DialogHeader>
        <div className="grid flex-1 gap-6 overflow-y-auto px-6 py-5">
          <section className="grid gap-3">
            <Label>{t("billable.customer")}</Label>
            <CustomerPicker
              value={cust.id}
              onChange={(id, c) => setCust({ id, currency: c?.currency ?? (cust.currency || "EUR") })}
            />
            {cust.id && (
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="grid gap-1.5">
                  <Label htmlFor="inv-cur">{t("billing.currency")}</Label>
                  <Select value={cust.currency} onValueChange={(currency) => setCust({ ...cust, currency })}>
                    <SelectTrigger id="inv-cur" className="w-full"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {(currencies.data?.items ?? [{ code: cust.currency }]).map((c) => <SelectItem key={c.code} value={c.code}>{c.code}</SelectItem>)}
                    </SelectContent>
                  </Select>
                </div>
                {fin && <Field id="inv-due" label={t("billable.due_days")} inputMode="numeric" value={dueDays} onChange={(e) => setDueDays(e.target.value)} hint={t("billable.due_days_hint")} />}
              </div>
            )}
          </section>

          {cust.id && (renewables.length > 0 || pendingItems.length > 0) && (
            <section className="grid gap-3">
              <div>
                <h3 className="text-sm font-medium">{t("billable.from_services")}</h3>
                <p className="text-sm text-muted-foreground">{t("billable.from_services_hint")}</p>
              </div>
              {renewables.length > 0 && (
                <ul className="divide-y rounded-md border">
                  {renewables.map((s) => (
                    <li key={s.id} className="flex flex-wrap items-center gap-2 px-3 py-2.5">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">{s.product_name}</p>
                        <p className="text-xs text-muted-foreground">
                          {formatMoney(s.recurring_amount, s.currency, lang)} · {tCode(`cycle.${s.cycle}`)}
                          {s.next_due_date ? ` · ${t("billing.due")} ${formatDate(s.next_due_date, lang)}` : ""}
                        </p>
                      </div>
                      <Button type="button" size="sm" variant="ghost" onClick={() => addServiceLine(s)}>{t("billable.add_line")}</Button>
                      <Button type="button" size="sm" variant="outline" disabled={renew.isPending} onClick={() => renew.mutate(s.id)}>
                        <ArrowClockwiseIcon aria-hidden />{t("renew.now")}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
              {pendingItems.length > 0 && (
                <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2.5">
                  <p className="text-sm">{t("billable.pending_count", { n: String(pendingItems.length) })}</p>
                  <Button type="button" size="sm" disabled={billPending.isPending} onClick={() => billPending.mutate()}>{t("billable.invoice_now")}</Button>
                </div>
              )}
            </section>
          )}

          {cust.id && (
            <>
              <Separator />
              <section className="grid gap-3">
                <div className="flex flex-wrap items-end justify-between gap-2">
                  <div>
                    <h3 className="text-sm font-medium">{t("billing.lines")}</h3>
                    <p className="text-sm text-muted-foreground">{t("billable.custom_lines_hint")}</p>
                  </div>
                  {priced.length > 0 && (
                    <Select value={productId || undefined} onValueChange={addProduct}>
                      <SelectTrigger className="h-9 w-56" aria-label={t("billable.add_product")}><SelectValue placeholder={t("billable.add_product")} /></SelectTrigger>
                      <SelectContent>
                        {priced.map((p) => {
                          const price = fromPrice(p.prices, cust.currency)!;
                          return <SelectItem key={p.id} value={p.id}>{p.name} · {formatMoney(price.amount, cust.currency, lang)}</SelectItem>;
                        })}
                      </SelectContent>
                    </Select>
                  )}
                </div>
                <LineEditor lines={lines} onChange={setLines} showTax />
                <p className="text-sm tabular-nums text-muted-foreground">
                  {t("billing.subtotal")}: <span className="font-medium text-foreground">{formatMoney(linesNet(lines).toFixed(2), cust.currency, lang)}</span>
                  <span> · {t("billable.tax_hint")}</span>
                </p>
              </section>

              <section className="grid gap-3 md:grid-cols-2">
                <div className="grid gap-1.5">
                  <Label htmlFor="inv-client-notes">{t("invoice_ops.client_notes")}</Label>
                  <Textarea id="inv-client-notes" maxLength={5000} rows={2} value={clientNotes} onChange={(e) => setClientNotes(e.target.value)} />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="inv-admin-notes">{t("invoice_ops.admin_notes")}</Label>
                  <Textarea id="inv-admin-notes" maxLength={10000} rows={2} value={adminNotes} onChange={(e) => setAdminNotes(e.target.value)} />
                </div>
              </section>

              <Label className="flex items-center gap-2"><Switch checked={fin} onCheckedChange={setFin} />{t("billable.finalize_now")}</Label>
            </>
          )}
          <FormError message={errMsg(m.error, tCode)} />
        </div>
        <DialogFooter className="border-t px-6 py-4">
          <Button disabled={!cust.id || !items || !dueOk || m.isPending} onClick={() => m.mutate()}>
            {fin ? t("billable.issue") : t("billable.save_draft")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Add one-off lines to a draft invoice. */
export function AddLinesDialog({ invoiceId, trigger }: { invoiceId: string; trigger?: ReactNode }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<LineDraft[]>([emptyLine()]);
  const items = linesPayload(lines);
  const m = useMutation({
    mutationFn: () => api(`/admin/invoices/${invoiceId}/items`, { method: "POST", body: { items } }),
    onSuccess: () => { setOpen(false); setLines([emptyLine()]); void qc.invalidateQueries({ queryKey: ["admin", "invoice", invoiceId] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); m.reset(); }}>
      <DialogTrigger asChild>{trigger ?? <Button size="sm" variant="outline"><PlusIcon aria-hidden />{t("billable.add_line")}</Button>}</DialogTrigger>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("common.edit")}</DialogTitle><DialogDescription>{t("billable.add_line")}</DialogDescription></DialogHeader>
        <LineEditor lines={lines} onChange={setLines} />
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={!items || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Per-product "generate the renewal invoice N days before due". */
export function ProductBillingDialog({ productId, productName }: { productId: string; productName: string }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [days, setDays] = useState<string | null>(null);
  const { data } = useQuery({ queryKey: ["admin", "product-billing", productId], enabled: open, queryFn: () => api<{ invoice_days_before: number | null; default_days: number }>(`/admin/catalog/products/${productId}/billing`) });
  const cur = days ?? (data?.invoice_days_before == null ? "" : String(data.invoice_days_before));
  const [term, setTerm] = useState<string | null>(null);
  const ft = useQuery({ queryKey: ["admin", "product-fixed-term", productId], enabled: open, queryFn: () => api<{ auto_terminate_days: number | null }>(`/admin/catalog/products/${productId}/fixed-term`) });
  const curTerm = term ?? (ft.data?.auto_terminate_days == null ? "" : String(ft.data.auto_terminate_days));
  const valid = (cur.trim() === "" || (/^\d+$/.test(cur.trim()) && Number(cur) <= 365))
    && (curTerm.trim() === "" || (/^\d+$/.test(curTerm.trim()) && Number(curTerm) >= 1 && Number(curTerm) <= 3650));
  const m = useMutation({
    mutationFn: async () => {
      await api(`/admin/catalog/products/${productId}/billing`, { method: "PUT", body: { invoice_days_before: cur.trim() === "" ? null : Number(cur) } });
      await api(`/admin/catalog/products/${productId}/fixed-term`, { method: "PUT", body: { auto_terminate_days: curTerm.trim() === "" ? null : Number(curTerm) } });
    },
    onSuccess: () => { setOpen(false); setDays(null); setTerm(null); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); setDays(null); setTerm(null); m.reset(); }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><CalendarCheckIcon aria-hidden />{t("billable.renewals")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{productName}</DialogTitle><DialogDescription>{t("billable.renewals_desc")}</DialogDescription></DialogHeader>
        <div className="grid gap-1.5">
          <Label htmlFor={`pb-${productId}`}>{t("billable.days_before")}</Label>
          <Input id={`pb-${productId}`} inputMode="numeric" placeholder={data ? String(data.default_days) : ""} value={cur} onChange={(e) => setDays(e.target.value)} />
          <p className="text-xs text-muted-foreground">{t("billable.days_hint")}</p>
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor={`ft-${productId}`}>{t("billable.fixed_term")}</Label>
          <Input id={`ft-${productId}`} inputMode="numeric" value={curTerm} onChange={(e) => setTerm(e.target.value)} />
          <p className="text-xs text-muted-foreground">{t("billable.fixed_term_hint")}</p>
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={!valid || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export interface StockValue { stock_enabled?: boolean; stock_qty?: number; allow_backorder?: boolean }

/** Per-product stock limit and backorder switch. */
export function ProductStockDialog({ productId, productName, value, onDone }: { productId: string; productName: string; value: StockValue; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [on, setOn] = useState(!!value.stock_enabled);
  const [qty, setQty] = useState(String(value.stock_qty ?? 0));
  const [back, setBack] = useState(!!value.allow_backorder);
  const valid = /^-?\d{1,7}$/.test(qty.trim());
  const m = useMutation({
    mutationFn: () => api(`/admin/catalog/products/${productId}/stock`, { method: "PUT", body: { stock_enabled: on, stock_qty: Number(qty), allow_backorder: back } }),
    onSuccess: () => { setOpen(false); onDone(); },
  });
  const reset = (o: boolean) => { setOpen(o); m.reset(); if (o) { setOn(!!value.stock_enabled); setQty(String(value.stock_qty ?? 0)); setBack(!!value.allow_backorder); } };
  return (
    <Dialog open={open} onOpenChange={reset}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><PackageIcon aria-hidden />{t("stock.title")}{value.stock_enabled && <Badge variant="secondary">{value.stock_qty ?? 0}</Badge>}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{productName}</DialogTitle><DialogDescription>{t("stock.desc")}</DialogDescription></DialogHeader>
        <div className="grid gap-4">
          <div className="flex items-center justify-between gap-3"><Label htmlFor={`se-${productId}`}>{t("stock.enabled")}</Label><Switch id={`se-${productId}`} checked={on} onCheckedChange={setOn} /></div>
          <div className="grid gap-1.5"><Label htmlFor={`sq-${productId}`}>{t("stock.qty")}</Label><Input id={`sq-${productId}`} inputMode="numeric" disabled={!on} value={qty} onChange={(e) => setQty(e.target.value)} /></div>
          <div className="flex items-center justify-between gap-3"><div className="grid gap-0.5"><Label htmlFor={`sb-${productId}`}>{t("stock.backorder")}</Label><p className="text-xs text-muted-foreground">{t("stock.backorder_hint")}</p></div><Switch id={`sb-${productId}`} disabled={!on} checked={back} onCheckedChange={setBack} /></div>
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={!valid || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function NewItemDialog() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const blank: BillableDraft = { customer_id: "", description: "", quantity: "1", unit_price: "", invoice_action: "hold", recur_cycle: "monthly", next_due_date: "", recur_times: "" };
  const [d, setD] = useState<BillableDraft>(blank);
  const body = billablePayload(d);
  const m = useMutation({
    mutationFn: () => api("/admin/billable-items", { method: "POST", body }),
    onSuccess: () => { setOpen(false); setD(blank); void qc.invalidateQueries({ queryKey: ["admin", "billable"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); m.reset(); }}>
      <DialogTrigger asChild><Button><PlusIcon aria-hidden />{t("billable.add")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("billable.add")}</DialogTitle></DialogHeader>
        <div className="grid gap-3">
          <CustomerPicker value={d.customer_id} onChange={(id) => setD({ ...d, customer_id: id })} />
          <LineEditor lines={[d]} onChange={(l) => l[0] && setD({ ...d, ...l[0] })} />
          <div className="grid gap-1.5"><Label>{t("billable.action")}</Label>
            <Select value={d.invoice_action} onValueChange={(v) => setD({ ...d, invoice_action: v })}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{ACTIONS.map((a) => <SelectItem key={a} value={a}>{tCode(`billable.action.${a}`)}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          {d.invoice_action === "recurring" && (
            <div className="grid gap-3 *:min-w-0 sm:grid-cols-3">
              <div className="grid gap-1.5"><Label>{t("billable.cycle")}</Label>
                <Select value={d.recur_cycle} onValueChange={(v) => setD({ ...d, recur_cycle: v })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{RECUR_CYCLES.map((c) => <SelectItem key={c} value={c}>{tCode(`cycle.${c}`)}</SelectItem>)}</SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5"><Label htmlFor="bi-next">{t("billable.first_date")}</Label><Input id="bi-next" type="date" value={d.next_due_date} onChange={(e) => setD({ ...d, next_due_date: e.target.value })} /></div>
              <div className="grid gap-1.5"><Label htmlFor="bi-times">{t("billable.times")}</Label><Input id="bi-times" inputMode="numeric" placeholder="∞" value={d.recur_times} onChange={(e) => setD({ ...d, recur_times: e.target.value })} /></div>
            </div>
          )}
          <FormError message={errMsg(m.error, tCode)} />
        </div>
        <DialogFooter><Button disabled={!body || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminBillableItemsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "billable"], queryFn: () => api<{ items: BillableItem[] }>("/admin/billable-items") });
  const inv = () => void qc.invalidateQueries({ queryKey: ["admin", "billable"] });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/billable-items/${id}`, { method: "DELETE" }), onSuccess: inv });
  const bill = useMutation({ mutationFn: (cid: string) => api(`/admin/customers/${cid}/billable-items/invoice`, { method: "POST" }), onSuccess: inv });
  const pending = (b: BillableItem) => !b.invoice_id && (b.invoice_action === "hold" || b.invoice_action === "next_invoice");
  return (
    <ListPage<BillableItem>
      title={t("billable.title")} description={t("billable.subtitle")} actions={<NewItemDialog />}
      rows={data?.items} rowKey={(b) => b.id} loading={isLoading} emptyIcon={ReceiptIcon} emptyTitle={t("billable.empty")}
      columns={[
        { header: t("billable.description"), primary: true, cell: (b) => <span className="grid"><span className="font-medium">{b.description}</span><span className="text-xs text-muted-foreground">{b.customer_email}</span></span> },
        { header: t("billing.total"), cell: (b) => <span className="tabular-nums">{Number(b.quantity) !== 1 && `${Number(b.quantity)} × `}{formatMoney(b.unit_price, b.currency, lang)}</span> },
        { header: t("billable.action"), cell: (b) => <span className="flex flex-wrap gap-1"><Badge variant="outline">{tCode(`billable.action.${b.invoice_action}`)}</Badge>{b.recur_cycle && <Badge variant="secondary">{tCode(`cycle.${b.recur_cycle}`)}</Badge>}</span> },
        { header: t("billable.next"), cell: (b) => b.next_due_date ? formatDate(b.next_due_date, lang) : "-" },
        { header: t("billing.invoice"), cell: (b) => b.invoice_id ? <Link className="text-primary hover:underline" to={`/admin/invoices/${b.invoice_id}`}>{b.invoice_number ?? t("billing.draft_invoice")}</Link> : <span className="text-muted-foreground">{t("billable.uninvoiced")}</span> },
        { header: "", cell: (b) => (
          <div className="flex flex-wrap justify-end gap-1">
            {pending(b) && <Button size="sm" variant="outline" disabled={bill.isPending} onClick={() => bill.mutate(b.customer_id)}>{t("billable.invoice_now")}</Button>}
            {(!b.invoice_id || b.invoice_action === "recurring") && <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(b.id)}><TrashIcon aria-hidden /></Button>}
          </div>
        ) },
      ]}
    >
      <FormError message={errMsg(del.error ?? bill.error, tCode)} />
    </ListPage>
  );
}
