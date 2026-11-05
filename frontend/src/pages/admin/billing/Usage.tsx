import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, ChartBarIcon, PencilSimpleIcon, PlusIcon, ReceiptIcon, TrashIcon } from "@phosphor-icons/react";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Product, type Service } from "@/lib/billing/billing";
import { newUuid } from "@/lib/ids";
import { useI18n } from "@/lib/i18n";
import { metricPayload, periodVariant, trimDecimal, validQuantity, type MetricDraft, type UsageMetric, type UsagePeriod } from "@/lib/billing/usage";

const EMPTY: MetricDraft = { product_id: "", key: "", name: "", unit: "", unit_price: "", included_quantity: "0", active: true, source_key: "", source_divisor: "1", aggregate: "sum" };

function useErr() {
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return { error, setError, fail: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) };
}

function MetricDialog({ products, metric }: { products: Product[]; metric?: UsageMetric }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<MetricDraft>(metric ? { ...metric, unit_price: trimDecimal(metric.unit_price), included_quantity: trimDecimal(metric.included_quantity), source_divisor: trimDecimal(metric.source_divisor ?? "1"), source_key: metric.source_key ?? "", aggregate: metric.aggregate ?? "sum" } : EMPTY);
  const e = useErr();
  const body = metricPayload(d);
  const save = useMutation({
    mutationFn: () => api(metric ? `/admin/usage-metrics/${metric.id}` : "/admin/usage-metrics", { method: metric ? "PUT" : "POST", body }),
    onSuccess: () => { setOpen(false); e.setError(null); if (!metric) setD(EMPTY); void qc.invalidateQueries({ queryKey: ["admin", "usage-metrics"] }); },
    onError: e.fail,
  });
  const f = (k: keyof MetricDraft, label: string, extra: object = {}) => (
    <div className="grid gap-2"><Label htmlFor={`m-${k}`}>{label}</Label><Input id={`m-${k}`} value={String(d[k])} onChange={(ev) => setD({ ...d, [k]: ev.target.value })} {...extra} /></div>
  );
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{metric ? <Button size="icon" variant="ghost" aria-label={t("common.edit")}><PencilSimpleIcon aria-hidden /></Button> : <Button size="sm"><PlusIcon aria-hidden />{t("usage.new_metric")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{metric ? metric.name : t("usage.new_metric")}</DialogTitle><DialogDescription>{t("usage.metric_help")}</DialogDescription></DialogHeader>
        <FormError message={e.error} />
        <div className="grid gap-3">
          <div className="grid gap-2">
            <Label htmlFor="m-product">{t("billing.product")}</Label>
            <Select value={d.product_id || undefined} onValueChange={(v) => setD({ ...d, product_id: v })}>
              <SelectTrigger id="m-product" className="w-full"><SelectValue placeholder={t("billing.product")} /></SelectTrigger>
              <SelectContent>{products.map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            {f("key", t("usage.key"), { className: "font-mono", placeholder: "bandwidth_gb" })}
            {f("name", t("usage.name"))}
            {f("unit", t("usage.unit"), { placeholder: "GB" })}
            {f("unit_price", t("usage.unit_price"), { inputMode: "decimal" })}
            {f("included_quantity", t("usage.included"), { inputMode: "decimal" })}
            {f("source_key", t("usage.source_key"), { className: "font-mono", placeholder: "disk_mb" })}
            {f("source_divisor", t("usage.source_divisor"), { inputMode: "decimal" })}
            <div className="grid gap-2">
              <Label htmlFor="m-agg">{t("usage.aggregate")}</Label>
              <Select value={d.aggregate} onValueChange={(v) => setD({ ...d, aggregate: v as "sum" | "max" })}>
                <SelectTrigger id="m-agg" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="sum">{t("usage.aggregate_sum")}</SelectItem><SelectItem value="max">{t("usage.aggregate_max")}</SelectItem></SelectContent>
              </Select>
            </div>
            <p className="text-xs text-muted-foreground sm:col-span-2">{t("usage.source_help")}</p>
            <div className="flex items-center gap-2 self-end pb-2"><Switch id="m-active" checked={d.active} onCheckedChange={(v) => setD({ ...d, active: v })} /><Label htmlFor="m-active">{t("usage.active")}</Label></div>
          </div>
        </div>
        <DialogFooter><Button disabled={!body || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminUsageMetricsPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const e = useErr();
  const [billed, setBilled] = useState<number | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "usage-metrics"], queryFn: () => api<{ items: UsageMetric[] }>("/admin/usage-metrics") });
  const { data: prods } = useQuery({ queryKey: ["admin", "products", "all"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products") });
  const products = prods?.items ?? [];
  const name = (id: string) => products.find((p) => p.id === id)?.name ?? "-";
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/usage-metrics/${id}`, { method: "DELETE" }), onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "usage-metrics"] }), onError: e.fail });
  const [sampled, setSampled] = useState<number | null>(null);
  const sample = useMutation({ mutationFn: () => api<{ recorded: number }>("/admin/usage/sample", { method: "POST" }), onSuccess: (r) => setSampled(r.recorded), onError: e.fail });
  const bill = useMutation({ mutationFn: () => api<{ invoices: number }>("/admin/usage/bill", { method: "POST" }), onSuccess: (r) => setBilled(r.invoices), onError: e.fail });
  return (
    <ListPage<UsageMetric>
      title={t("usage.title")} description={t("usage.description")}
      actions={
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" disabled={sample.isPending} onClick={() => sample.mutate()}><ArrowsClockwiseIcon aria-hidden />{t("usage.sample_now")}</Button>
          <Button size="sm" variant="outline" disabled={bill.isPending} onClick={() => bill.mutate()}><ReceiptIcon aria-hidden />{t("usage.bill_now")}</Button>
          <MetricDialog products={products} />
        </div>
      }
      rows={data?.items} loading={isLoading} rowKey={(m) => m.id} emptyIcon={ChartBarIcon} emptyTitle={t("usage.no_metrics")}
      columns={[
        { header: t("usage.name"), cell: (m) => m.name, primary: true },
        { header: t("usage.key"), cell: (m) => <span className="font-mono text-xs">{m.key}</span> },
        { header: t("usage.source"), cell: (m) => m.source_key ? <span className="font-mono text-xs">{m.source_key}{m.source_divisor && trimDecimal(m.source_divisor) !== "1" ? ` ÷ ${trimDecimal(m.source_divisor)}` : ""} · {m.aggregate}</span> : <span className="text-muted-foreground">{t("usage.source_api")}</span> },
        { header: t("billing.product"), cell: (m) => name(m.product_id) },
        { header: t("usage.unit_price"), cell: (m) => `${trimDecimal(m.unit_price)} / ${m.unit}` },
        { header: t("usage.included"), cell: (m) => `${trimDecimal(m.included_quantity)} ${m.unit}` },
        { header: t("billing.status"), cell: (m) => <Badge variant={m.active ? "default" : "outline"}>{m.active ? t("usage.active") : t("usage.inactive")}</Badge> },
        { header: "", cell: (m) => (
          <div className="flex gap-1">
            <MetricDialog products={products} metric={m} />
            <ConfirmAction description={t("usage.delete_confirm")} onConfirm={() => del.mutate(m.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>
        ) },
      ]}
    >
      <FormError message={e.error} />
      {sampled !== null && <p className="text-sm text-muted-foreground">{t("usage.sampled_n", { n: sampled })}</p>}
      {billed !== null && <p className="text-sm text-muted-foreground">{t("usage.billed_n", { n: billed })}</p>}
    </ListPage>
  );
}

/** Per-period usage breakdown, shared by admin and client. */
export function UsageHistory({ items, currency }: { items: UsagePeriod[]; currency: string }) {
  const { t, tCode, lang } = useI18n();
  if (items.length === 0) return <p className="text-sm text-muted-foreground">{t("usage.no_usage")}</p>;
  return (
    <div className="flex flex-col gap-4">
      {items.map(({ period: p, metrics }) => {
        const est = metrics.reduce((a, m) => a + Number(m.charge), 0);
        return (
          <section key={p.id} className="flex flex-col gap-2 rounded-md border p-3">
            <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
              <span className="font-medium">{formatDate(p.period_start, lang)}, {formatDate(p.period_end, lang)}</span>
              <span className="flex items-center gap-2">
                <Badge variant={periodVariant(p.status)}>{tCode(`usage.status.${p.status}`)}</Badge>
                <span className="tabular-nums">{formatMoney(p.total ?? est.toFixed(4), currency, lang)}</span>
              </span>
            </div>
            {metrics.length === 0 ? <p className="text-xs text-muted-foreground">{t("usage.no_usage")}</p> : (
              <ul className="grid gap-1 text-sm">
                {metrics.map((m) => (
                  <li key={m.key} className="grid grid-cols-[1fr_auto] gap-x-3">
                    <span>{m.name}</span>
                    <span className="text-right tabular-nums">{formatMoney(m.charge, currency, lang)}</span>
                    <span className="col-span-2 text-xs text-muted-foreground">{t("usage.line", { used: trimDecimal(m.quantity), included: trimDecimal(m.included), billable: trimDecimal(m.billable), unit: m.unit, price: trimDecimal(m.unit_price) })}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        );
      })}
    </div>
  );
}

/** Usage dialog for a service; admins can also record usage manually. */
export function ServiceUsageDialog({ service, admin }: { service: Service; admin?: boolean }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [metric, setMetric] = useState("");
  const [qty, setQty] = useState("");
  const e = useErr();
  const base = `/${admin ? "admin" : "client"}/services/${service.id}/usage`;
  const key = [admin ? "admin" : "client", "usage", service.id];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ items: UsagePeriod[] }>(base), enabled: open });
  const { data: metrics } = useQuery({ queryKey: ["admin", "usage-metrics"], queryFn: () => api<{ items: UsageMetric[] }>("/admin/usage-metrics"), enabled: open && !!admin });
  const mine = (metrics?.items ?? []).filter((m) => m.product_id === service.product_id && m.active);
  const rec = useMutation({
    mutationFn: () => api(base, { method: "POST", body: { metric, quantity: qty.trim(), idempotency_key: newUuid() } }),
    onSuccess: () => { setQty(""); e.setError(null); void qc.invalidateQueries({ queryKey: key }); },
    onError: e.fail,
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="ghost"><ChartBarIcon aria-hidden />{t("usage.usage")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{t("usage.usage")} · {service.product_name}</DialogTitle><DialogDescription>{t("usage.billed_at_end")}</DialogDescription></DialogHeader>
        <FormError message={e.error} />
        {admin && mine.length > 0 && (
          <div className="grid gap-2 *:min-w-0 sm:grid-cols-[1fr_7rem_auto] sm:items-end">
            <div className="grid gap-2"><Label htmlFor="u-metric">{t("usage.metric")}</Label>
              <Select value={metric || undefined} onValueChange={setMetric}>
                <SelectTrigger id="u-metric" className="w-full"><SelectValue placeholder={t("usage.metric")} /></SelectTrigger>
                <SelectContent>{mine.map((m) => <SelectItem key={m.id} value={m.key}>{m.name} ({m.unit})</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="grid gap-2"><Label htmlFor="u-qty">{t("usage.quantity")}</Label><Input id="u-qty" inputMode="decimal" value={qty} onChange={(ev) => setQty(ev.target.value)} /></div>
            <Button disabled={!metric || !validQuantity(qty) || rec.isPending} onClick={() => rec.mutate()}><PlusIcon aria-hidden />{t("usage.record")}</Button>
          </div>
        )}
        <UsageHistory items={data?.items ?? []} currency={service.currency} />
      </DialogContent>
    </Dialog>
  );
}
