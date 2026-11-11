import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsLeftRightIcon, PackageIcon, PencilSimpleIcon, PlusIcon, TrashIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListPage, ListCard } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import type { Product } from "@/lib/billing/billing";
import type { Bundle, Recommendations } from "@/lib/store/cart";
import { useI18n } from "@/lib/i18n";
import { useModules } from "@/lib/modules";
import { OWNERSHIP_MODES, parseOwnership, validMinPercent, type OwnershipSettings } from "@/lib/bundleOwnership";

interface Draft { name: string; description: string; active: boolean; discount_percent: string; items: { product_id: string; cycle: string }[] }
const EMPTY: Draft = { name: "", description: "", active: true, discount_percent: "10", items: [{ product_id: "", cycle: "" }, { product_id: "", cycle: "" }] };

function useErr() {
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return { error, setError, fail: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) };
}
const cyclesOf = (p?: Product) => [...new Set((p?.prices ?? []).map((x) => x.cycle))];

function BundleDialog({ products, bundle }: { products: Product[]; bundle?: Bundle }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const init = (): Draft => bundle ? { name: bundle.name, description: bundle.description, active: bundle.active, discount_percent: String(Number(bundle.discount_percent)), items: bundle.items.map((i) => ({ product_id: i.product_id, cycle: i.cycle })) } : EMPTY;
  const [d, setD] = useState<Draft>(init);
  const e = useErr();
  const valid = d.name.trim() !== "" && d.items.length >= 2 && d.items.every((i) => i.product_id && i.cycle) && new Set(d.items.map((i) => i.product_id)).size === d.items.length && /^\d{1,3}(\.\d{1,2})?$/.test(d.discount_percent) && Number(d.discount_percent) <= 100;
  const save = useMutation({
    mutationFn: () => api(bundle ? `/admin/bundles/${bundle.id}` : "/admin/bundles", { method: bundle ? "PUT" : "POST", body: { ...d, name: d.name.trim() } }),
    onSuccess: () => { setOpen(false); e.setError(null); void qc.invalidateQueries({ queryKey: ["admin", "bundles"] }); },
    onError: e.fail,
  });
  const setItem = (n: number, v: Partial<Draft["items"][number]>) => setD((x) => ({ ...x, items: x.items.map((it, j) => (j === n ? { ...it, ...v } : it)) }));
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setD(init()); e.setError(null); } }}>
      <DialogTrigger asChild>{bundle ? <Button size="icon" variant="ghost" aria-label={t("common.edit")}><PencilSimpleIcon aria-hidden /></Button> : <Button size="sm"><PlusIcon aria-hidden />{t("bundles.new")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{bundle ? t("bundles.edit") : t("bundles.new")}</DialogTitle><DialogDescription>{t("bundles.dialog_hint")}</DialogDescription></DialogHeader>
        <form id="bundle-form" className="grid gap-4" onSubmit={(ev) => { ev.preventDefault(); if (valid) save.mutate(); }}>
          <FormError message={e.error} />
          <div className="grid gap-2"><Label htmlFor="b-name">{t("bundles.name")}</Label><Input id="b-name" value={d.name} onChange={(ev) => setD({ ...d, name: ev.target.value })} /></div>
          <div className="grid gap-2"><Label htmlFor="b-desc">{t("bundles.description")}</Label><Textarea id="b-desc" value={d.description} onChange={(ev) => setD({ ...d, description: ev.target.value })} /></div>
          <div className="grid grid-cols-2 items-end gap-4">
            <div className="grid gap-2"><Label htmlFor="b-pct">{t("bundles.discount_percent")}</Label><Input id="b-pct" inputMode="decimal" value={d.discount_percent} onChange={(ev) => setD({ ...d, discount_percent: ev.target.value })} /></div>
            <Label className="flex h-9 items-center gap-2"><Switch checked={d.active} onCheckedChange={(v) => setD({ ...d, active: v })} />{t("usage.active")}</Label>
          </div>
          <div className="grid gap-2">
            <Label>{t("bundles.items")}</Label>
            {d.items.map((it, n) => {
              const p = products.find((x) => x.id === it.product_id);
              return (
                <div key={n} className="grid grid-cols-[1fr_8rem_auto] gap-2">
                  <Select value={it.product_id} onValueChange={(v) => setItem(n, { product_id: v, cycle: cyclesOf(products.find((x) => x.id === v))[0] ?? "" })}>
                    <SelectTrigger className="w-full min-w-0" aria-label={t("billing.product")}><SelectValue placeholder={t("billing.product")} /></SelectTrigger>
                    <SelectContent>{products.map((x) => <SelectItem key={x.id} value={x.id}>{x.name}</SelectItem>)}</SelectContent>
                  </Select>
                  <Select value={it.cycle} onValueChange={(v) => setItem(n, { cycle: v })} disabled={!p}>
                    <SelectTrigger className="w-full" aria-label={t("billing.cycle")}><SelectValue /></SelectTrigger>
                    <SelectContent>{cyclesOf(p).map((c) => <SelectItem key={c} value={c}>{tCode(`cycle.${c}`)}</SelectItem>)}</SelectContent>
                  </Select>
                  <Button type="button" size="icon" variant="ghost" aria-label={t("cart.remove")} disabled={d.items.length <= 2} onClick={() => setD({ ...d, items: d.items.filter((_, j) => j !== n) })}><XIcon aria-hidden /></Button>
                </div>
              );
            })}
            <Button type="button" size="sm" variant="outline" className="self-start" disabled={d.items.length >= 10} onClick={() => setD({ ...d, items: [...d.items, { product_id: "", cycle: "" }] })}><PlusIcon aria-hidden />{t("bundles.add_item")}</Button>
          </div>
        </form>
        <DialogFooter><Button type="submit" form="bundle-form" disabled={!valid || save.isPending}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Picker({ label, value, onChange, options }: { label: string; value: string[]; onChange: (v: string[]) => void; options: Product[] }) {
  const { t } = useI18n();
  const left = options.filter((p) => !value.includes(p.id));
  return (
    <div className="grid gap-2">
      <Label>{label}</Label>
      <div className="flex flex-wrap gap-1">
        {value.map((id) => <Badge key={id} variant="secondary" className="gap-1">{options.find((p) => p.id === id)?.name ?? "-"}<button type="button" aria-label={t("cart.remove")} onClick={() => onChange(value.filter((x) => x !== id))}><XIcon aria-hidden /></button></Badge>)}
        {value.length === 0 && <span className="text-sm text-muted-foreground">{t("recs.none")}</span>}
      </div>
      {value.length < 6 && left.length > 0 && (
        <Select value="" onValueChange={(v) => onChange([...value, v])}>
          <SelectTrigger className="w-full" aria-label={label}><SelectValue placeholder={t("recs.add")} /></SelectTrigger>
          <SelectContent>{left.map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}</SelectContent>
        </Select>
      )}
    </div>
  );
}

function RecsDialog({ product, products }: { product: Product; products: Product[] }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<Recommendations>({ upsell: [], cross_sell: [] });
  const e = useErr();
  const load = useMutation({ mutationFn: () => api<Recommendations>(`/admin/catalog/products/${product.id}/recommendations`), onSuccess: setD, onError: e.fail });
  const save = useMutation({ mutationFn: () => api(`/admin/catalog/products/${product.id}/recommendations`, { method: "PUT", body: d }), onSuccess: () => setOpen(false), onError: e.fail });
  const others = products.filter((p) => p.id !== product.id);
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { e.setError(null); load.mutate(); } }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><ArrowsLeftRightIcon aria-hidden />{t("recs.edit")}</Button></DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader><DialogTitle>{t("recs.title", { name: product.name })}</DialogTitle><DialogDescription>{t("recs.hint")}</DialogDescription></DialogHeader>
        <FormError message={e.error} />
        <Picker label={t("recs.upsell")} value={d.upsell} options={others} onChange={(upsell) => setD({ ...d, upsell })} />
        <Picker label={t("recs.cross_sell")} value={d.cross_sell} options={others} onChange={(cross_sell) => setD({ ...d, cross_sell })} />
        <DialogFooter><Button disabled={load.isPending || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Module `bundle_ownership` settings (shown while the module is on). */
function OwnershipCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "modules"], queryFn: () => api<{ items: { id: string; enabled: boolean; settings: Record<string, unknown> }[] }>("/admin/modules") });
  const m = data?.items.find((x) => x.id === "bundle_ownership");
  const [f, setF] = useState<OwnershipSettings | null>(null);
  const cur = f ?? parseOwnership(m?.settings);
  const save = useMutation({
    mutationFn: () => api("/admin/modules/bundle_ownership", { method: "PUT", body: { enabled: true, settings: { mode: cur.mode, min_percent: cur.min_percent.trim() } } }),
    onSuccess: () => { setF(null); qc.invalidateQueries({ queryKey: ["admin", "modules"] }); qc.invalidateQueries({ queryKey: ["public", "bundle-ownership"] }); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!m?.enabled) return null;
  const ok = validMinPercent(cur.min_percent);
  return (
    <Card className="min-w-0 shadow-sm" data-testid="bundle-ownership-settings">
      <CardHeader>
        <CardTitle>{t("bundle_own.title")}</CardTitle>
        <CardDescription>{t("bundle_own.desc")}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 sm:grid-cols-2">
        <div className="flex flex-col gap-2">
          <Label htmlFor="own-mode">{t("bundle_own.mode")}</Label>
          <Select value={cur.mode} onValueChange={(v) => setF({ ...cur, mode: v as OwnershipSettings["mode"] })}>
            <SelectTrigger id="own-mode" className="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>{OWNERSHIP_MODES.map((x) => <SelectItem key={x} value={x}>{t(`bundle_own.mode_${x}`)}</SelectItem>)}</SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">{t(`bundle_own.mode_${cur.mode}_hint`)}</p>
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="own-min">{t("bundle_own.min_percent")}</Label>
          <Input id="own-min" inputMode="decimal" value={cur.min_percent} aria-invalid={!ok} onChange={(e) => setF({ ...cur, min_percent: e.target.value })} />
          <p className="text-xs text-muted-foreground">{t("bundle_own.min_percent_hint")}</p>
        </div>
        <div className="sm:col-span-2"><Button disabled={!ok || save.isPending || f === null} onClick={() => save.mutate()}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}

export function AdminBundlesPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const e = useErr();
  const modules = useModules();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "bundles"], queryFn: () => api<{ items: Bundle[] }>("/admin/bundles") });
  const { data: prods, isLoading: pl } = useQuery({ queryKey: ["admin", "products", "all"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products") });
  const products = prods?.items ?? [];
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/bundles/${id}`, { method: "DELETE" }), onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "bundles"] }), onError: e.fail });
  return (
    <ListPage<Bundle>
      title={t("bundles.admin_title")} description={t("bundles.admin_description")} actions={<BundleDialog products={products} />}
      rows={data?.items} loading={isLoading} rowKey={(b) => b.id} emptyIcon={PackageIcon} emptyTitle={t("bundles.none")}
      columns={[
        { header: t("bundles.name"), cell: (b) => b.name, primary: true },
        { header: t("bundles.items"), cell: (b) => b.items.map((i) => i.product_name).join(" + ") },
        { header: t("bundles.discount_percent"), cell: (b) => `${Number(b.discount_percent)}%` },
        { header: t("billing.status"), cell: (b) => <Badge variant={b.active ? "default" : "outline"}>{b.active ? t("usage.active") : t("usage.inactive")}</Badge> },
        { header: "", cell: (b) => (
          <div className="flex gap-1">
            <BundleDialog products={products} bundle={b} />
            <ConfirmAction description={t("bundles.delete_confirm")} onConfirm={() => del.mutate(b.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>
        ) },
      ]}
    >
      <FormError message={e.error} />
      {modules.enabled.has("bundle_ownership") && <OwnershipCard />}
      <Card className="min-w-0 gap-0 overflow-hidden py-0 shadow-sm"><CardHeader><CardTitle>{t("recs.section")}</CardTitle></CardHeader></Card>
      <ListCard<Product> rows={prods?.items} loading={pl} rowKey={(p) => p.id} emptyIcon={PackageIcon} emptyTitle={t("bundles.none")}
        columns={[
          { header: t("billing.product"), cell: (p) => p.name, primary: true },
          { header: "", cell: (p) => <RecsDialog product={p} products={products} /> },
        ]} />
    </ListPage>
  );
}
