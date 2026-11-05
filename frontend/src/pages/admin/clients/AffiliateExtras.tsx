import { useState } from "react";
import { PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { ListCard } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import type { Material, ProductRate, Tier } from "@/lib/admin/affiliateExtras";
import { useI18n } from "@/lib/i18n";

const KEY = ["admin", "affiliates", "extras"];

function useSave<B>(path: (b: B) => string, method: (b: B) => string, done: () => void) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const m = useMutation({
    mutationFn: (b: B) => api(path(b), { method: method(b), body: method(b) === "DELETE" ? undefined : b }),
    onSuccess: () => { setError(null); done(); void qc.invalidateQueries({ queryKey: KEY }); },
    onError: (e) => { const msg = e instanceof ApiError ? tCode(e.code) : t("error.internal"); setError(msg); toast.error(msg); },
  });
  return { ...m, error };
}

function RateDialog() {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState({ product_id: "", kind: "percent", value: "10", currency: "EUR" });
  const { data: products } = useQuery({ queryKey: ["admin", "products", "all"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/products"), enabled: open });
  const { data: currencies } = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies"), enabled: open });
  const save = useSave<typeof d>(() => "/admin/affiliates/product-rates", () => "PUT", () => setOpen(false));
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm"><PlusIcon aria-hidden />{t("affiliate.add_rate")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("affiliate.product_rates")}</DialogTitle></DialogHeader>
        <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate({ ...d, currency: d.kind === "fixed" ? d.currency : null } as typeof d); }}>
          <div className="flex flex-col gap-1.5"><Label>{t("billing.product")}</Label>
            <Select value={d.product_id} onValueChange={(v) => setD({ ...d, product_id: v })}>
              <SelectTrigger className="w-full"><SelectValue placeholder={t("billing.product")} /></SelectTrigger>
              <SelectContent>{(products?.items ?? []).map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-3">
            <div className="flex flex-col gap-1.5"><Label>{t("affiliate.rate_kind")}</Label>
              <Select value={d.kind} onValueChange={(v) => setD({ ...d, kind: v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="percent">{t("affiliate.kind.percent")}</SelectItem><SelectItem value="fixed">{t("affiliate.kind.fixed")}</SelectItem></SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5"><Label htmlFor="ar-v">{t("affiliate.rate_value")}</Label><Input id="ar-v" inputMode="decimal" value={d.value} onChange={(e) => setD({ ...d, value: e.target.value })} /></div>
            {d.kind === "fixed" && (
              <div className="flex flex-col gap-1.5"><Label>{t("billing.currency")}</Label>
                <Select value={d.currency} onValueChange={(v) => setD({ ...d, currency: v })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{(currencies?.items ?? []).map((c) => <SelectItem key={c.code} value={c.code}>{c.code}</SelectItem>)}</SelectContent>
                </Select>
              </div>
            )}
          </div>
          <FormError message={save.error} />
          <DialogFooter><Button type="submit" disabled={!d.product_id || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ProductRatesCard() {
  const { t, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: [...KEY, "rates"], queryFn: () => api<{ items: ProductRate[] }>("/admin/affiliates/product-rates") });
  const del = useSave<string>((id) => `/admin/affiliates/product-rates/${id}`, () => "DELETE", () => {});
  return (
    <div className="flex flex-col gap-3">
      <Card className="gap-0 overflow-hidden py-0 shadow-sm">
        <CardHeader className="py-4"><CardTitle>{t("affiliate.product_rates")}</CardTitle><CardDescription>{t("affiliate.product_rates_help")}</CardDescription><CardAction><RateDialog /></CardAction></CardHeader>
      </Card>
      <ListCard<ProductRate> rows={data?.items} rowKey={(r) => r.product_id} loading={isLoading} emptyTitle={t("affiliate.no_rates")}
        columns={[
          { header: t("billing.product"), primary: true, cell: (r) => r.product_name },
          { header: t("affiliate.rate_value"), cell: (r) => r.kind === "percent" ? `${Number(r.value)}%` : `${formatMoney(r.value, r.currency ?? "EUR", lang)} / ${t("affiliate.per_service")}` },
          { header: "", cell: (r) => <div className="flex justify-end"><Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(r.product_id)}><TrashIcon aria-hidden /></Button></div> },
        ]} />
    </div>
  );
}

function TiersCard() {
  const { t } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: [...KEY, "tiers"], queryFn: () => api<{ items: Tier[] }>("/admin/affiliates/tiers") });
  const [d, setD] = useState({ name: "", min_referrals: "5", commission_percent: "15" });
  const add = useSave<{ name: string; min_referrals: number; commission_percent: string }>(() => "/admin/affiliates/tiers", () => "POST", () => setD({ name: "", min_referrals: "5", commission_percent: "15" }));
  const del = useSave<string>((id) => `/admin/affiliates/tiers/${id}`, () => "DELETE", () => {});
  return (
    <div className="flex flex-col gap-3">
      <Card className="gap-0 overflow-hidden py-0 shadow-sm">
        <CardHeader className="py-4"><CardTitle>{t("affiliate.tiers")}</CardTitle><CardDescription>{t("affiliate.tiers_help")}</CardDescription></CardHeader>
        <CardContent className="pb-4">
          <form className="grid gap-2 sm:grid-cols-[1fr_8rem_8rem_auto] sm:items-end" onSubmit={(e) => { e.preventDefault(); add.mutate({ name: d.name, min_referrals: Number(d.min_referrals), commission_percent: d.commission_percent }); }}>
            <div className="flex flex-col gap-1.5"><Label htmlFor="tr-n">{t("affiliate.tier_name")}</Label><Input id="tr-n" value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} /></div>
            <div className="flex flex-col gap-1.5"><Label htmlFor="tr-m">{t("affiliate.min_referrals")}</Label><Input id="tr-m" type="number" min={1} value={d.min_referrals} onChange={(e) => setD({ ...d, min_referrals: e.target.value })} /></div>
            <div className="flex flex-col gap-1.5"><Label htmlFor="tr-p">{t("affiliate.commission_percent")}</Label><Input id="tr-p" inputMode="decimal" value={d.commission_percent} onChange={(e) => setD({ ...d, commission_percent: e.target.value })} /></div>
            <Button type="submit" disabled={!d.name.trim() || add.isPending}><PlusIcon aria-hidden />{t("common.add")}</Button>
          </form>
        </CardContent>
      </Card>
      <ListCard<Tier> rows={data?.items} rowKey={(r) => r.id} loading={isLoading} emptyTitle={t("affiliate.no_tiers")}
        columns={[
          { header: t("affiliate.tier_name"), primary: true, cell: (r) => r.name },
          { header: t("affiliate.min_referrals"), cell: (r) => r.min_referrals },
          { header: t("affiliate.commission_percent"), cell: (r) => `${Number(r.commission_percent)}%` },
          { header: "", cell: (r) => <div className="flex justify-end"><Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(r.id)}><TrashIcon aria-hidden /></Button></div> },
        ]} />
    </div>
  );
}

type MaterialDraft = Omit<Material, "id" | "created_at">;
const blankMaterial: MaterialDraft = { kind: "text", title: "", body: "", image_url: "", sort_order: 0, active: true };

function MaterialDialog({ initial }: { initial?: Material }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<MaterialDraft>(blankMaterial);
  const save = useSave<MaterialDraft>(() => initial ? `/admin/affiliates/materials/${initial.id}` : "/admin/affiliates/materials", () => initial ? "PUT" : "POST", () => setOpen(false));
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) setD(initial ? { kind: initial.kind, title: initial.title, body: initial.body, image_url: initial.image_url, sort_order: initial.sort_order, active: initial.active } : blankMaterial); }}>
      <DialogTrigger asChild>{initial ? <Button size="sm" variant="outline">{t("common.edit")}</Button> : <Button size="sm"><PlusIcon aria-hidden />{t("affiliate.add_material")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("affiliate.materials")}</DialogTitle></DialogHeader>
        <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(d); }}>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="flex flex-col gap-1.5"><Label>{t("affiliate.material_kind")}</Label>
              <Select value={d.kind} onValueChange={(v) => setD({ ...d, kind: v as Material["kind"] })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="text">{t("affiliate.material.text")}</SelectItem><SelectItem value="banner">{t("affiliate.material.banner")}</SelectItem></SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5"><Label htmlFor="m-s">{t("affiliate.sort_order")}</Label><Input id="m-s" type="number" value={d.sort_order} onChange={(e) => setD({ ...d, sort_order: Number(e.target.value) })} /></div>
          </div>
          <div className="flex flex-col gap-1.5"><Label htmlFor="m-t">{t("affiliate.material_title")}</Label><Input id="m-t" value={d.title} onChange={(e) => setD({ ...d, title: e.target.value })} /></div>
          {d.kind === "banner" && <div className="flex flex-col gap-1.5"><Label htmlFor="m-i">{t("affiliate.image_url")}</Label><Input id="m-i" type="url" value={d.image_url} onChange={(e) => setD({ ...d, image_url: e.target.value })} /></div>}
          <div className="flex flex-col gap-1.5"><Label htmlFor="m-b">{t("affiliate.material_body")}</Label><Textarea id="m-b" rows={4} value={d.body} onChange={(e) => setD({ ...d, body: e.target.value })} /><p className="text-xs text-muted-foreground">{t("affiliate.material_body_help")}</p></div>
          <div className="flex items-center gap-2"><Switch id="m-a" checked={d.active} onCheckedChange={(v) => setD({ ...d, active: v })} /><Label htmlFor="m-a">{t("affiliate.material_active")}</Label></div>
          <FormError message={save.error} />
          <DialogFooter><Button type="submit" disabled={!d.title.trim() || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function MaterialsCard() {
  const { t } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: [...KEY, "materials"], queryFn: () => api<{ items: Material[] }>("/admin/affiliates/materials") });
  const del = useSave<string>((id) => `/admin/affiliates/materials/${id}`, () => "DELETE", () => {});
  return (
    <div className="flex flex-col gap-3">
      <Card className="gap-0 overflow-hidden py-0 shadow-sm">
        <CardHeader className="py-4"><CardTitle>{t("affiliate.materials")}</CardTitle><CardDescription>{t("affiliate.materials_help")}</CardDescription><CardAction><MaterialDialog /></CardAction></CardHeader>
      </Card>
      <ListCard<Material> rows={data?.items} rowKey={(r) => r.id} loading={isLoading} emptyTitle={t("affiliate.no_materials")}
        columns={[
          { header: t("affiliate.material_title"), primary: true, cell: (r) => r.title },
          { header: t("affiliate.material_kind"), cell: (r) => t(`affiliate.material.${r.kind}`) },
          { header: t("billing.status"), cell: (r) => <Badge variant={r.active ? "default" : "secondary"}>{r.active ? t("affiliate.material_active") : t("affiliate.material_hidden")}</Badge> },
          { header: "", cell: (r) => <div className="flex justify-end gap-1"><MaterialDialog initial={r} /><Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(r.id)}><TrashIcon aria-hidden /></Button></div> },
        ]} />
    </div>
  );
}

export function AffiliateExtrasAdmin() {
  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <ProductRatesCard />
      <TiersCard />
      <div className="xl:col-span-2"><MaterialsCard /></div>
    </div>
  );
}
