import { invoiceLabel } from "@/lib/billing/billing";
import { PageHeader } from "@/components/PageHeader";
import { OrderStatusBadge } from "@/components/OrderStatusBadge";
import { PageSkeleton } from "@/components/PageSkeleton";
import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowCounterClockwiseIcon, CheckIcon, NoteIcon, PlusIcon, ShieldWarningIcon, TrashIcon, WarningCircleIcon, XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { CustomerPicker } from "@/components/admin/CustomerPicker";
import { StatusBadge } from "@/components/StatusBadge";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { useCrumb } from "@/lib/crumb";
import type { CatalogProduct } from "@/pages/client/services/Order";

interface AdminOrder {
  order: { id: string; number: number; status: string; currency: string; total: string; invoice_id: string | null; created_at: string; approved_at: string | null };
  items: { id: string; product_name: string; cycle: string; amount: string; setup_fee: string }[];
  customer: { id: string; email: string; first_name: string; last_name: string; organization: string; country: string } | null;
  invoice: { id: string; number: string | null; status: string; total: string; amount_paid: string } | null;
  fraud: { score: number; verdict: string; reasons: unknown[]; ip: string | null; created_at: string } | null;
  notes: { id: string; author: string | null; body: string; created_at: string }[];
  services: { id: string; product_name: string; status: string }[];
}

/** Admin order page: items, customer, invoice, fraud, notes, accept/cancel/refund. */
export function AdminOrderPage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const key = ["admin", "order", id];
  const { data, isError, error: queryError } = useQuery({ queryKey: key, queryFn: () => api<AdminOrder>(`/admin/orders/${encodeURIComponent(id)}`), retry: false });
  const nav = useNavigate();
  const act = useMutation({
    mutationFn: ({ path, body }: { path: string; body?: unknown }) => api(`/admin/orders/${id}/${path}`, { method: "POST", body: body ?? {} }),
    onSuccess: () => { setError(null); setNote(""); void qc.invalidateQueries({ queryKey: ["admin"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  useCrumb(data ? `#${data.order.number}` : null);
  // Opened by order number: switch to the canonical UUID URL so actions target the right order.
  useEffect(() => {
    if (data && data.order.id !== id) nav(`/admin/orders/${data.order.id}`, { replace: true });
  }, [data, id, nav]);
  if (isError) {
    const notFound = queryError instanceof ApiError && queryError.status === 404;
    return (
      <EmptyState icon={WarningCircleIcon} title={t(notFound ? "orders.not_found" : "common.error")}
        description={notFound ? t("orders.not_found_hint", { id }) : undefined} />
    );
  }
  if (!data) return <PageSkeleton />;
  const o = data.order;
  const m = (v: string) => formatMoney(v, o.currency, lang);
  const c = data.customer;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("store.order_n", { n: o.number })} badge={<OrderStatusBadge status={o.status} invoiceStatus={data.invoice?.status} />}
        description={`${formatDate(o.created_at, lang)} · ${m(o.total)}`} back={{ to: "/admin/orders", label: t("nav.orders") }}
        actions={<>
            {o.status === "pending" && <Button size="sm" disabled={act.isPending} onClick={() => act.mutate({ path: "approve" })}><CheckIcon aria-hidden />{t("billing.approve")}</Button>}
            {o.status === "pending" && <ConfirmAction description={t("admin_orders.cancel_hint")} onConfirm={() => act.mutate({ path: "cancel" })}><Button size="sm" variant="outline"><XIcon aria-hidden />{t("billing.cancel")}</Button></ConfirmAction>}
            {o.status === "pending" && <ConfirmAction description={t("admin_orders.fraud_hint")} onConfirm={() => act.mutate({ path: "cancel", body: { fraud: true } })}><Button size="sm" variant="outline"><ShieldWarningIcon aria-hidden />{t("billing.fraud")}</Button></ConfirmAction>}
            {data.invoice?.status === "paid" && o.status !== "cancelled" && <ConfirmAction description={t("admin_orders.refund_hint")} onConfirm={() => act.mutate({ path: "refund" })}><Button size="sm" variant="outline"><ArrowCounterClockwiseIcon aria-hidden />{t("admin_orders.refund")}</Button></ConfirmAction>}
        </>} />
      <FormError message={error} />
      <Replaceable section="summary" ctx={{ order_id: o.id, status: o.status }}>
      <Card>
        <CardContent className="grid gap-4 *:min-w-0 md:grid-cols-2">
          <div className="grid gap-1 text-sm">
            <p className="font-medium">{t("admin_orders.customer")}</p>
            <span>{c ? [c.first_name, c.last_name].filter(Boolean).join(" ") || c.email : "-"}</span>
            {c && <p className="text-muted-foreground">{c.email}{c.organization ? ` · ${c.organization}` : ""}{c.country ? ` · ${c.country}` : ""}</p>}
          </div>
          <div className="grid gap-1 text-sm">
            <p className="font-medium">{t("billing.invoice")}</p>
            {data.invoice ? <span className="flex items-center gap-2"><Link className="text-primary hover:underline" to={`/admin/invoices/${data.invoice.id}`}>{invoiceLabel(data.invoice, t)}</Link><StatusBadge status={data.invoice.status} /></span> : "-"}
          </div>
          <ul className="flex flex-col divide-y rounded-md border md:col-span-2">
            {data.items.map((it) => (
              <li key={it.id} className="flex items-center justify-between gap-3 p-3 text-sm">
                <span>{it.product_name} · {tCode(`cycle.${it.cycle}`)}</span>
                <span className="tabular-nums">{m(it.amount)}{Number(it.setup_fee) > 0 ? ` + ${m(it.setup_fee)}` : ""}</span>
              </li>
            ))}
          </ul>
          {data.services.length > 0 && (
            <div className="flex flex-wrap gap-2 md:col-span-2">{data.services.map((s) => <Badge key={s.id} variant="secondary">{s.product_name} · {tCode(`status.${s.status}`)}</Badge>)}</div>
          )}
        </CardContent>
      </Card>
      </Replaceable>
      <div className="grid gap-4 *:min-w-0 lg:grid-cols-2">
        <Replaceable section="fraud" ctx={{ order_id: o.id, status: o.status }}>
        <Card>
          <CardHeader><CardTitle className="flex items-center gap-2"><ShieldWarningIcon weight="duotone" className="size-5 text-primary" aria-hidden />{t("admin_orders.fraud")}</CardTitle></CardHeader>
          <CardContent className="text-sm">
            {data.fraud ? (
              <div className="grid gap-2">
                <div className="flex items-center gap-2"><Badge variant={data.fraud.verdict === "accept" ? "secondary" : "destructive"}>{tCode(`admin_orders.verdict_${data.fraud.verdict}`)}</Badge><span className="tabular-nums">{t("admin_orders.score", { n: data.fraud.score })}</span></div>
                {data.fraud.ip && <p className="font-mono text-xs text-muted-foreground">{data.fraud.ip}</p>}
                {data.fraud.reasons.length > 0 && <ul className="list-inside list-disc text-muted-foreground">{data.fraud.reasons.map((r, i) => <li key={i}>{typeof r === "string" ? r : JSON.stringify(r)}</li>)}</ul>}
              </div>
            ) : <p className="text-muted-foreground">{t("admin_orders.no_fraud")}</p>}
          </CardContent>
        </Card>
        </Replaceable>
        <Replaceable section="notes" ctx={{ order_id: o.id, status: o.status }}>
        <Card>
          <CardHeader><CardTitle className="flex items-center gap-2"><NoteIcon weight="duotone" className="size-5 text-primary" aria-hidden />{t("admin_orders.notes")}</CardTitle></CardHeader>
          <CardContent className="grid gap-3">
            {data.notes.length === 0 && <p className="text-sm text-muted-foreground">{t("admin_orders.no_notes")}</p>}
            {data.notes.map((n) => (
              <div key={n.id} className="rounded-md border p-3 text-sm">
                <p className="whitespace-pre-line">{n.body}</p>
                <p className="mt-1 text-xs text-muted-foreground">{n.author ?? "-"} · {new Date(n.created_at).toLocaleString(lang)}</p>
              </div>
            ))}
            <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); if (note.trim()) act.mutate({ path: "notes", body: { body: note.trim() } }); }}>
              <Label htmlFor="order-note" className="sr-only">{t("admin_orders.add_note")}</Label>
              <Textarea id="order-note" value={note} maxLength={4000} onChange={(e) => setNote(e.target.value)} placeholder={t("admin_orders.note_placeholder")} />
              <Button type="submit" size="sm" variant="outline" className="justify-self-end" disabled={!note.trim() || act.isPending}><PlusIcon aria-hidden />{t("admin_orders.add_note")}</Button>
            </form>
          </CardContent>
        </Card>
        </Replaceable>
      </div>
      <PluginSlot slot="admin.order.detail" ctx={{ order_id: o.id }} />
    </div>
  );
}

interface Line { product_id: string; cycle: string }

/** Place an order on a customer's behalf. */
export function ManualOrderDialog() {
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const [open, setOpen] = useState(false);
  const [customer, setCustomer] = useState("");
  const [currency, setCurrency] = useState("");
  const [lines, setLines] = useState<Line[]>([]);
  const [coupon, setCoupon] = useState("");
  const [approve, setApprove] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { data: catalog } = useQuery({ queryKey: ["catalog"], queryFn: () => api<{ products: CatalogProduct[] }>("/catalog"), enabled: open });
  const { data: cur } = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies"), enabled: open });
  const ccy = currency || cur?.items[0]?.code || "";
  const products = (catalog?.products ?? []).filter((p) => p.prices.some((x) => x.currency === ccy));
  const place = useMutation({
    mutationFn: () => api<{ order: { id: string } }>("/admin/orders", { method: "POST", body: { customer_id: customer, currency: ccy, items: lines.map((l) => ({ ...l, config: {} })), coupon: coupon.trim() || null, approve } }),
    onSuccess: (r) => { setOpen(false); nav(`/admin/orders/${r.order.id}`); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const setLine = (i: number, l: Line) => setLines((x) => x.map((y, j) => (j === i ? l : y)));
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); setError(null); }}>
      <DialogTrigger asChild><Button size="sm"><PlusIcon aria-hidden />{t("admin_orders.new")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90svh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("admin_orders.new")}</DialogTitle></DialogHeader>
        <div className="grid gap-4">
          <FormError message={error} />
          <div className="grid gap-2"><Label>{t("admin_orders.customer")}</Label><CustomerPicker value={customer} onChange={setCustomer} /></div>
          <div className="grid gap-2"><Label>{t("billing.currency")}</Label>
            <Select value={ccy} onValueChange={(c) => { setCurrency(c); setLines([]); }}>
              <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
              <SelectContent>{(cur?.items ?? []).map((c) => <SelectItem key={c.code} value={c.code}>{c.code}</SelectItem>)}</SelectContent>
            </Select></div>
          {lines.map((l, i) => {
            const p = products.find((x) => x.id === l.product_id);
            return (
              <div key={i} className="grid gap-2 rounded-md border p-3 sm:grid-cols-[1fr_10rem_auto]">
                <Select value={l.product_id} onValueChange={(v) => setLine(i, { product_id: v, cycle: products.find((x) => x.id === v)?.prices.find((x) => x.currency === ccy)?.cycle ?? "" })}>
                  <SelectTrigger className="w-full" aria-label={t("billing.product")}><SelectValue /></SelectTrigger>
                  <SelectContent>{products.map((x) => <SelectItem key={x.id} value={x.id}>{x.name}</SelectItem>)}</SelectContent>
                </Select>
                <Select value={l.cycle} onValueChange={(v) => setLine(i, { ...l, cycle: v })}>
                  <SelectTrigger className="w-full" aria-label={t("billing.cycle")}><SelectValue /></SelectTrigger>
                  <SelectContent>{(p?.prices ?? []).filter((x) => x.currency === ccy).map((x) => <SelectItem key={x.cycle} value={x.cycle}>{tCode(`cycle.${x.cycle}`)} · {formatMoney(x.amount, ccy, lang)}</SelectItem>)}</SelectContent>
                </Select>
                <Button size="icon" variant="ghost" aria-label={t("cart.remove")} onClick={() => setLines((x) => x.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
              </div>
            );
          })}
          <Button variant="outline" size="sm" className="justify-self-start" disabled={products.length === 0} onClick={() => { const p = products[0]; if (!p) return; setLines((x) => [...x, { product_id: p.id, cycle: p.prices.find((y) => y.currency === ccy)?.cycle ?? "" }]); }}><PlusIcon aria-hidden />{t("admin_orders.add_line")}</Button>
          <div className="grid gap-2"><Label htmlFor="mo-coupon">{t("order.promo")}</Label><Input id="mo-coupon" className="font-mono uppercase" value={coupon} onChange={(e) => setCoupon(e.target.value.toUpperCase())} /></div>
          <div className="flex min-h-11 items-center justify-between gap-3 rounded-md border px-3"><Label htmlFor="mo-approve">{t("admin_orders.approve_now")}</Label><Switch id="mo-approve" checked={approve} onCheckedChange={setApprove} /></div>
        </div>
        <DialogFooter><Button disabled={!customer || lines.length === 0 || place.isPending} onClick={() => place.mutate()}>{t("admin_orders.create")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
