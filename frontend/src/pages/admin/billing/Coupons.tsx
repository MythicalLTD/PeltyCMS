import { useState } from "react";
import { ListPage } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { PlusIcon, TicketIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Checkbox } from "@/components/ui/checkbox";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Product } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { splitList, tldList } from "@/lib/store/orderForms";

export interface Coupon {
  id: string; code: string; description: string; kind: "percent" | "fixed"; value: string; currency: string | null;
  recurring: boolean; max_uses: number | null; max_uses_per_customer: number | null; uses: number;
  starts_at: string | null; expires_at: string | null; active: boolean; product_ids: string[];
  recurring_cycles: number | null; applies_addons: boolean; applies_domains: boolean; tlds: string[]; customer_ids: string[]; client_group_ids: string[];
}

type Draft = Omit<Coupon, "id" | "uses">;
const blank: Draft = { code: "", description: "", kind: "percent", value: "10", currency: null, recurring: false, max_uses: null, max_uses_per_customer: null, starts_at: null, expires_at: null, active: true, product_ids: [], recurring_cycles: null, applies_addons: false, applies_domains: false, tlds: [], customer_ids: [], client_group_ids: [] };

const couponIso = (v: string | null) => (v ? new Date(v).toISOString() : null);

/** Payload sent to the API: dates as RFC 3339, empty numbers as null. */
export function toPayload(d: Draft) {
  return { ...d, recurring_cycles: d.recurring ? d.recurring_cycles : null, currency: d.kind === "fixed" ? (d.currency || "EUR") : null, starts_at: couponIso(d.starts_at), expires_at: couponIso(d.expires_at) };
}

export function couponValue(c: Pick<Coupon, "kind" | "value" | "currency">, lang: string) {
  return c.kind === "percent" ? `${Number(c.value)}%` : formatMoney(c.value, c.currency ?? "EUR", lang);
}

const couponNum = (v: string) => (v === "" ? null : Math.max(1, Number(v)));
const couponDtLocal = (v: string | null) => (v ? v.slice(0, 16) : "");

function CouponDialog({ initial, onDone, trigger }: { initial?: Coupon; onDone: () => void; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<Draft>(initial ?? blank);
  const [error, setError] = useState<string | null>(null);
  const { data: products } = useQuery({ queryKey: ["admin", "products"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products"), enabled: open });
  const { data: groups } = useQuery({ queryKey: ["admin", "client-groups"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/client-groups"), enabled: open });
  const [tlds, setTlds] = useState("");
  const [customers, setCustomers] = useState("");
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/coupons/${initial.id}` : "/admin/coupons", { method: initial ? "PUT" : "POST", body: toPayload({ ...d, tlds: tldList(tlds), customer_ids: splitList(customers) }) }),
    onSuccess: () => { setOpen(false); setError(null); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { const v = { ...blank, ...initial }; setD(v); setTlds(v.tlds.join(", ")); setCustomers(v.customer_ids.join(", ")); } }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{initial ? t("coupons.edit") : t("coupons.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="c-code">{t("coupons.code")}</Label>
            <Input id="c-code" required className="font-mono uppercase" value={d.code} onChange={(e) => set("code", e.target.value.toUpperCase())} /></div>
          <div className="grid gap-2"><Label htmlFor="c-desc">{t("coupons.description")}</Label>
            <Input id="c-desc" value={d.description} onChange={(e) => set("description", e.target.value)} /></div>
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2"><Label>{t("coupons.kind")}</Label>
              <Select value={d.kind} onValueChange={(v) => set("kind", v as Draft["kind"])}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="percent">{t("coupons.kind_percent")}</SelectItem><SelectItem value="fixed">{t("coupons.kind_fixed")}</SelectItem></SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label htmlFor="c-val">{t("coupons.value")}</Label>
              <Input id="c-val" inputMode="decimal" required value={d.value} onChange={(e) => set("value", e.target.value)} /></div>
          </div>
          {d.kind === "fixed" && (
            <div className="grid gap-2"><Label htmlFor="c-cur">{t("billing.currency")}</Label>
              <Input id="c-cur" maxLength={3} className="uppercase" value={d.currency ?? "EUR"} onChange={(e) => set("currency", e.target.value.toUpperCase())} /></div>
          )}
          <div className="flex items-center justify-between gap-3 rounded-md border p-3">
            <div><Label htmlFor="c-rec">{t("coupons.recurring")}</Label><p className="text-sm text-muted-foreground">{t("coupons.recurring_hint")}</p></div>
            <Switch id="c-rec" checked={d.recurring} onCheckedChange={(v) => set("recurring", v)} />
          </div>
          {d.recurring && (
            <div className="grid gap-2"><Label htmlFor="c-cyc">{t("coupons.recurring_cycles")}</Label>
              <Input id="c-cyc" type="number" min={1} placeholder="∞" value={d.recurring_cycles ?? ""} onChange={(e) => set("recurring_cycles", couponNum(e.target.value))} />
              <p className="text-sm text-muted-foreground">{t("coupons.recurring_cycles_hint")}</p></div>
          )}
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="flex items-center justify-between gap-3 rounded-md border p-3">
              <Label htmlFor="c-add">{t("coupons.applies_addons")}</Label>
              <Switch id="c-add" checked={d.applies_addons} onCheckedChange={(v) => set("applies_addons", v)} />
            </div>
            <div className="flex items-center justify-between gap-3 rounded-md border p-3">
              <Label htmlFor="c-dom">{t("coupons.applies_domains")}</Label>
              <Switch id="c-dom" checked={d.applies_domains} onCheckedChange={(v) => set("applies_domains", v)} />
            </div>
          </div>
          {d.applies_domains && (
            <div className="grid gap-2"><Label htmlFor="c-tlds">{t("coupons.tlds")}</Label>
              <Input id="c-tlds" placeholder="com, ro" value={tlds} onChange={(e) => setTlds(e.target.value)} />
              <p className="text-sm text-muted-foreground">{t("coupons.tlds_hint")}</p></div>
          )}
          <div className="grid gap-2">
            <Label>{t("coupons.client_groups")}</Label>
            <p className="text-sm text-muted-foreground">{t("coupons.audience_hint")}</p>
            <div className="grid max-h-32 gap-2 overflow-y-auto rounded-md border p-3">
              {(groups?.items ?? []).map((g) => (
                <label key={g.id} className="flex items-center gap-2 text-sm">
                  <Checkbox checked={d.client_group_ids.includes(g.id)} onCheckedChange={(v) => set("client_group_ids", v ? [...d.client_group_ids, g.id] : d.client_group_ids.filter((x) => x !== g.id))} />
                  {g.name}
                </label>
              ))}
            </div>
          </div>
          <div className="grid gap-2"><Label htmlFor="c-cust">{t("coupons.customers")}</Label>
            <Input id="c-cust" className="font-mono" value={customers} onChange={(e) => setCustomers(e.target.value)} />
            <p className="text-sm text-muted-foreground">{t("coupons.customers_hint")}</p></div>
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2"><Label htmlFor="c-max">{t("coupons.max_uses")}</Label>
              <Input id="c-max" type="number" min={1} placeholder="∞" value={d.max_uses ?? ""} onChange={(e) => set("max_uses", couponNum(e.target.value))} /></div>
            <div className="grid gap-2"><Label htmlFor="c-maxc">{t("coupons.max_per_customer")}</Label>
              <Input id="c-maxc" type="number" min={1} placeholder="∞" value={d.max_uses_per_customer ?? ""} onChange={(e) => set("max_uses_per_customer", couponNum(e.target.value))} /></div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2"><Label htmlFor="c-start">{t("coupons.starts")}</Label>
              <Input id="c-start" type="datetime-local" value={couponDtLocal(d.starts_at)} onChange={(e) => set("starts_at", e.target.value || null)} /></div>
            <div className="grid gap-2"><Label htmlFor="c-exp">{t("coupons.expires")}</Label>
              <Input id="c-exp" type="datetime-local" value={couponDtLocal(d.expires_at)} onChange={(e) => set("expires_at", e.target.value || null)} /></div>
          </div>
          <div className="grid gap-2">
            <Label>{t("coupons.products")}</Label>
            <p className="text-sm text-muted-foreground">{t("coupons.products_hint")}</p>
            <div className="grid max-h-40 gap-2 overflow-y-auto rounded-md border p-3">
              {(products?.items ?? []).map((p) => (
                <label key={p.id} className="flex items-center gap-2 text-sm">
                  <Checkbox checked={d.product_ids.includes(p.id)} onCheckedChange={(v) => set("product_ids", v ? [...d.product_ids, p.id] : d.product_ids.filter((x) => x !== p.id))} />
                  {p.name}
                </label>
              ))}
            </div>
          </div>
          <div className="flex items-center justify-between rounded-md border p-3">
            <Label htmlFor="c-act">{t("coupons.active")}</Label>
            <Switch id="c-act" checked={d.active} onCheckedChange={(v) => set("active", v)} />
          </div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AdminCouponsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "coupons"], queryFn: () => api<{ items: Coupon[] }>("/admin/coupons") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "coupons"] });
  const del = useMutation({
    mutationFn: (id: string) => api(`/admin/coupons/${id}`, { method: "DELETE" }),
    onSuccess: () => { setError(null); refresh(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const limit = (c: Coupon) => `${c.uses} / ${c.max_uses ?? "∞"}`;
  return (
    <ListPage<Coupon>
      title={t("coupons.title")}
      actions={<CouponDialog onDone={refresh} trigger={<Button><PlusIcon aria-hidden />{t("coupons.new")}</Button>} />}
      rows={data?.items} rowKey={(c) => c.id} loading={isLoading} emptyIcon={TicketIcon} emptyTitle={t("coupons.empty")}
      columns={[
              { header: t("coupons.code"), primary: true, cell: (c) => <span className="font-mono font-medium">{c.code}</span> },
              { header: t("coupons.value"), cell: (c) => couponValue(c, lang) },
              { header: t("coupons.type"), cell: (c) => <Badge variant="outline">{c.recurring ? (c.recurring_cycles ? t("coupons.recurring_n", { n: c.recurring_cycles }) : t("coupons.recurring")) : t("coupons.first_only")}</Badge> },
              { header: t("coupons.uses"), cell: (c) => <span className="tabular-nums">{limit(c)}</span> },
              { header: t("coupons.expires"), cell: (c) => (c.expires_at ? formatDate(c.expires_at, lang) : "-") },
              { header: t("billing.status"), cell: (c) => <Badge variant={c.active ? "default" : "secondary"}>{c.active ? t("coupons.active") : t("coupons.inactive")}</Badge> },
              { header: "", cell: (c) => (
                <div className="flex justify-end gap-1">
                  <CouponDialog initial={c} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
                  <ConfirmAction description={t("coupons.delete_confirm", { code: c.code })} onConfirm={() => del.mutate(c.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={del.isPending}><TrashIcon aria-hidden /></Button></ConfirmAction>
                </div>
              ) },
            ]}
    >
      <FormError message={error} />
    </ListPage>
  );
}
