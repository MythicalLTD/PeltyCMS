import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PackageIcon, PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListingPicker } from "@/components/creator/CreatorCoupons";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { bundleValid, type BundleDraft, type CreatorBundle } from "@/lib/creatorTools";
import { useI18n } from "@/lib/i18n";
import type { Listing } from "@/lib/marketplace";

const empty: BundleDraft = { name: "", description: "", discount_percent: "15", active: true, product_ids: [] };

/** Creator dashboard tab: bundles of the creator's own listings at a discount. */
export function CreatorBundles() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["client", "creator", "bundles"], queryFn: () => api<{ items: CreatorBundle[] }>("/client/creator/bundles") });
  const listings = useQuery({ queryKey: ["client", "creator", "listings"], queryFn: () => api<{ items: Listing[] }>("/client/creator/listings") });
  const [d, setD] = useState<BundleDraft | null>(null);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["client", "creator", "bundles"] });
  const save = useMutation({
    mutationFn: (x: BundleDraft) => api(x.id ? `/client/creator/bundles/${x.id}` : "/client/creator/bundles", { method: x.id ? "PUT" : "POST", body: { name: x.name.trim(), description: x.description.trim(), discount_percent: x.discount_percent.trim(), active: x.active, product_ids: x.product_ids } }),
    onSuccess: () => { setD(null); refresh(); toast.success(t("ctools.bundle_saved")); },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (id: string) => api(`/client/creator/bundles/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  return (
    <div className="flex flex-col gap-3" data-testid="creator-bundles">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">{t("ctools.bundles_hint")}</p>
        <Button onClick={() => setD(empty)}><PlusIcon aria-hidden />{t("ctools.new_bundle")}</Button>
      </div>
      {!q.data ? <Skeleton className="h-24 w-full" /> : q.data.items.length === 0 ? <EmptyState icon={PackageIcon} title={t("ctools.no_bundles")} description={t("ctools.no_bundles_hint")} /> : (
        <div className="grid gap-3 md:grid-cols-2">
          {q.data.items.map((b) => (
            <Card key={b.id} data-testid="creator-bundle">
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <div className="min-w-0"><CardTitle className="truncate text-base">{b.name}</CardTitle>{b.description && <CardDescription className="line-clamp-2">{b.description}</CardDescription>}</div>
                <Badge variant={b.active ? "secondary" : "outline"}>{t("ctools.off_percent", { n: Number(b.discount_percent) })}</Badge>
              </CardHeader>
              <CardContent className="flex flex-col gap-2">
                <ul className="flex flex-col gap-1 text-sm">{b.items.map((i) => <li key={i.product_id} className="flex items-center gap-1.5"><PackageIcon aria-hidden className="text-muted-foreground" />{i.product_name}</li>)}</ul>
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" onClick={() => setD({ id: b.id, name: b.name, description: b.description, discount_percent: String(Number(b.discount_percent)), active: b.active, product_ids: b.items.map((i) => i.product_id) })}><PencilSimpleIcon aria-hidden />{t("common.edit")}</Button>
                  <Button size="sm" variant="ghost" disabled={remove.isPending} onClick={() => remove.mutate(b.id)}><TrashIcon aria-hidden />{t("common.delete")}</Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
      <Dialog open={!!d} onOpenChange={(o) => !o && setD(null)}>
        <DialogContent className="max-h-[90svh] overflow-y-auto">
          <DialogHeader><DialogTitle>{d?.id ? t("ctools.edit_bundle") : t("ctools.new_bundle")}</DialogTitle><DialogDescription>{t("ctools.bundle_dialog_hint")}</DialogDescription></DialogHeader>
          {d && (
            <form id="ct-bundle" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(d); }}>
              <Field id="ctb-name" label={t("ctools.bundle_name")} required maxLength={200} value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} />
              <div className="grid gap-2"><Label htmlFor="ctb-desc">{t("ctools.bundle_description")}</Label><Textarea id="ctb-desc" rows={2} maxLength={2000} value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} /></div>
              <Field id="ctb-pct" label={t("ctools.percent")} required inputMode="decimal" value={d.discount_percent} onChange={(e) => setD({ ...d, discount_percent: e.target.value })} />
              <ListingPicker id="ctb-p" listings={(listings.data?.items ?? []).filter((l) => l.status === "approved")} value={d.product_ids} onChange={(v) => setD({ ...d, product_ids: v })} />
              <div className="flex items-center gap-2"><Switch id="ctb-active" checked={d.active} onCheckedChange={(v) => setD({ ...d, active: v })} /><Label htmlFor="ctb-active">{t("ctools.active")}</Label></div>
            </form>
          )}
          <DialogFooter><Button type="submit" form="ct-bundle" disabled={!d || !bundleValid(d) || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
