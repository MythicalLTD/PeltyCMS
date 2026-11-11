import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PencilSimpleIcon, PlusIcon, TagIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { couponBody, couponDraft, couponPreview, couponValid, toggle, type CouponDraft, type CreatorCoupon } from "@/lib/creatorTools";
import { useI18n } from "@/lib/i18n";
import type { Listing } from "@/lib/marketplace";

/** Pick listings with checkboxes (codes and bundles). */
export function ListingPicker({ id, listings, value, onChange }: { id: string; listings: Listing[]; value: string[]; onChange: (v: string[]) => void }) {
  const { t } = useI18n();
  return (
    <fieldset className="grid gap-2">
      <legend className="mb-1 text-sm font-medium">{t("ctools.listings")}</legend>
      <div className="grid max-h-48 gap-1.5 overflow-y-auto rounded-md border p-2">
        {listings.length === 0 && <p className="text-sm text-muted-foreground">{t("market.no_listings")}</p>}
        {listings.map((l) => (
          <label key={l.product_id} htmlFor={`${id}-${l.product_id}`} className="flex items-center gap-2 text-sm">
            <Checkbox id={`${id}-${l.product_id}`} checked={value.includes(l.product_id)} onCheckedChange={() => onChange(toggle(value, l.product_id))} />
            <span className="min-w-0 flex-1 truncate">{l.name}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** Creator dashboard tab: discount codes limited to the creator's own listings. */
export function CreatorCoupons() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["client", "creator", "coupons"], queryFn: () => api<{ items: CreatorCoupon[]; prefix: string }>("/client/creator/coupons") });
  const listings = useQuery({ queryKey: ["client", "creator", "listings"], queryFn: () => api<{ items: Listing[] }>("/client/creator/listings") });
  const [d, setD] = useState<CouponDraft | null>(null);
  const prefix = q.data?.prefix ?? "";
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["client", "creator", "coupons"] });
  const save = useMutation({
    mutationFn: (x: CouponDraft) => api(x.id ? `/client/creator/coupons/${x.id}` : "/client/creator/coupons", { method: x.id ? "PUT" : "POST", body: couponBody(x) }),
    onSuccess: () => { setD(null); refresh(); toast.success(t("ctools.code_saved")); },
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api<{ deleted: boolean }>(`/client/creator/coupons/${id}`, { method: "DELETE" }),
    onSuccess: (r) => { refresh(); toast.success(r.deleted ? t("ctools.code_deleted") : t("ctools.code_disabled")); },
    onError: fail,
  });
  const names = new Map((listings.data?.items ?? []).map((l) => [l.product_id, l.name]));
  return (
    <div className="flex flex-col gap-3" data-testid="creator-coupons">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">{t("ctools.codes_hint")}</p>
        <Button onClick={() => setD(couponDraft())}><PlusIcon aria-hidden />{t("ctools.new_code")}</Button>
      </div>
      {!q.data ? <Skeleton className="h-24 w-full" /> : q.data.items.length === 0 ? <EmptyState icon={TagIcon} title={t("ctools.no_codes")} description={t("ctools.no_codes_hint")} /> : (
        <div className="grid gap-3 md:grid-cols-2">
          {q.data.items.map((c) => (
            <Card key={c.id} data-testid="creator-coupon">
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <div className="min-w-0">
                  <CardTitle className="truncate font-mono text-base">{c.code}</CardTitle>
                  <CardDescription>{c.kind === "percent" ? t("ctools.off_percent", { n: Number(c.value) }) : t("ctools.off_fixed", { amount: Number(c.value).toLocaleString(lang), currency: c.currency ?? "" })}</CardDescription>
                </div>
                <Badge variant={c.active ? "secondary" : "outline"}>{c.active ? t("ctools.active") : t("ctools.inactive")}</Badge>
              </CardHeader>
              <CardContent className="flex flex-col gap-2 text-sm">
                <p className="text-muted-foreground">{t("ctools.uses", { n: c.uses, max: c.max_uses ?? "∞" })}{c.expires_at && ` · ${t("ctools.expires", { date: new Date(c.expires_at).toLocaleDateString(lang) })}`}</p>
                <div className="flex flex-wrap gap-1">{c.product_ids.map((p) => <Badge key={p} variant="outline">{names.get(p) ?? "…"}</Badge>)}</div>
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" onClick={() => setD(couponDraft(c, prefix))}><PencilSimpleIcon aria-hidden />{t("common.edit")}</Button>
                  <Button size="sm" variant="ghost" disabled={remove.isPending} onClick={() => remove.mutate(c.id)}><TrashIcon aria-hidden />{t("common.delete")}</Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
      <Dialog open={!!d} onOpenChange={(o) => !o && setD(null)}>
        <DialogContent className="max-h-[90svh] overflow-y-auto">
          <DialogHeader><DialogTitle>{d?.id ? t("ctools.edit_code") : t("ctools.new_code")}</DialogTitle><DialogDescription>{t("ctools.code_dialog_hint")}</DialogDescription></DialogHeader>
          {d && (
            <form id="ct-coupon" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(d); }}>
              <Field id="ct-code" label={t("ctools.code")} required maxLength={40} value={d.code} onChange={(e) => setD({ ...d, code: e.target.value })} />
              {d.code && <p className="-mt-2 text-xs text-muted-foreground">{t("ctools.code_preview", { code: couponPreview(prefix, d.code) })}</p>}
              <div className="grid grid-cols-2 gap-2">
                <SelectField id="ct-kind" label={t("ctools.kind")} value={d.kind} onChange={(v) => setD({ ...d, kind: v as CouponDraft["kind"] })} options={[{ value: "percent", label: t("ctools.kind_percent") }, { value: "fixed", label: t("ctools.kind_fixed") }]} />
                <Field id="ct-value" label={d.kind === "percent" ? t("ctools.percent") : t("ctools.amount")} required inputMode="decimal" value={d.value} onChange={(e) => setD({ ...d, value: e.target.value })} />
              </div>
              {d.kind === "fixed" && <Field id="ct-cur" label={t("creator.currency")} required maxLength={3} value={d.currency} onChange={(e) => setD({ ...d, currency: e.target.value.toUpperCase() })} />}
              <div className="grid grid-cols-2 gap-2">
                <Field id="ct-max" label={t("ctools.max_uses")} inputMode="numeric" placeholder="∞" value={d.max_uses} onChange={(e) => setD({ ...d, max_uses: e.target.value })} />
                <Field id="ct-per" label={t("ctools.max_per_customer")} inputMode="numeric" placeholder="∞" value={d.max_uses_per_customer} onChange={(e) => setD({ ...d, max_uses_per_customer: e.target.value })} />
              </div>
              <Field id="ct-exp" label={t("ctools.expires_on")} type="date" value={d.expires_at} onChange={(e) => setD({ ...d, expires_at: e.target.value })} />
              <ListingPicker id="ct-p" listings={listings.data?.items ?? []} value={d.product_ids} onChange={(v) => setD({ ...d, product_ids: v })} />
              <div className="flex items-center gap-2"><Switch id="ct-active" checked={d.active} onCheckedChange={(v) => setD({ ...d, active: v })} /><Label htmlFor="ct-active">{t("ctools.active")}</Label></div>
            </form>
          )}
          <DialogFooter><Button type="submit" form="ct-coupon" disabled={!d || !couponValid(d, prefix) || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
