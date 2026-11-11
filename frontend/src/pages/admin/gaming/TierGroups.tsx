import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownIcon, ArrowUpIcon, PencilSimpleIcon, PlusIcon, TrashIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { LoadError } from "@/components/LoadError";
import { PageHeader } from "@/components/PageHeader";
import { SelectField } from "@/components/SelectField";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { CREDIT_MODES, MAX_TIERS, moveTier, tierBody, tierDraft, tierValid, type TierDraft, type TierGroup } from "@/lib/tiers";
import { useCatalog } from "@/pages/store/Store";

/** Tier groups: ranked packages whose owners pay only the difference. */
export function AdminTierGroupsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "tier-groups"], queryFn: () => api<{ items: TierGroup[] }>("/admin/tier-groups") });
  const catalog = useCatalog();
  const [edit, setEdit] = useState<{ id: string | null; d: TierDraft } | null>(null);
  const [add, setAdd] = useState("");
  const [del, setDel] = useState<TierGroup | null>(null);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin", "tier-groups"] }); void qc.invalidateQueries({ queryKey: ["public", "tier-upgrades"] }); };
  const save = useMutation({
    mutationFn: ({ id, d }: { id: string | null; d: TierDraft }) => api<TierGroup>(id ? `/admin/tier-groups/${id}` : "/admin/tier-groups", { method: id ? "PUT" : "POST", body: tierBody(d) }),
    onSuccess: () => { setEdit(null); refresh(); toast.success(t("market.saved")); },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (id: string) => api(`/admin/tier-groups/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const set = (p: Partial<TierDraft>) => setEdit((e) => (e ? { ...e, d: { ...e.d, ...p } } : e));
  const products = catalog.data?.products ?? [];
  const names = new Map(products.map((p) => [p.id, p.name]));
  // A package belongs to one group: hide the ones ranked elsewhere.
  const taken = new Set((q.data?.items ?? []).filter((g) => g.id !== edit?.id).flatMap((g) => g.tiers.map((x) => x.product_id)));
  const addable = products.filter((p) => !taken.has(p.id) && !edit?.d.product_ids.includes(p.id));
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("tiers.title")} description={t("tiers.desc")}
        actions={<Button onClick={() => { setAdd(""); setEdit({ id: null, d: tierDraft() }); }}><PlusIcon aria-hidden />{t("tiers.add")}</Button>} />
      {q.isError && !q.data ? <LoadError retry={() => void q.refetch()} /> : !q.data ? <Skeleton className="h-40 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("tiers.empty")} description={t("tiers.empty_hint")} /> : (
        <div className="grid gap-4 md:grid-cols-2" data-testid="tier-groups">
          {q.data.items.map((g) => (
            <Card key={g.id} data-testid="tier-group">
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <div className="min-w-0">
                  <CardTitle className="truncate">{g.name}</CardTitle>
                  <CardDescription>{t(`tiers.mode.${g.credit_mode}`)}{Number(g.min_price) > 0 ? ` · ${t("tiers.min_short", { amount: String(Number(g.min_price)) })}` : ""}</CardDescription>
                </div>
                <div className="flex shrink-0 gap-1">
                  <Button size="icon" variant="ghost" aria-label={t("common.edit")} onClick={() => { setAdd(""); setEdit({ id: g.id, d: tierDraft(g) }); }}><PencilSimpleIcon aria-hidden /></Button>
                  <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => setDel(g)}><TrashIcon aria-hidden /></Button>
                </div>
              </CardHeader>
              <CardContent className="grid gap-3">
                <ol className="flex flex-wrap items-center gap-1.5 text-sm">
                  {g.tiers.map((x, i) => (
                    <li key={x.product_id} className="flex items-center gap-1.5">
                      {i > 0 && <span aria-hidden className="text-muted-foreground">‹</span>}
                      <Badge variant={i === g.tiers.length - 1 ? "default" : "secondary"}>{x.name}</Badge>
                    </li>
                  ))}
                </ol>
                <div className="flex flex-wrap gap-1.5 text-xs">
                  {g.cancel_old_recurring && <Badge variant="outline">{t("tiers.cancel_old_short")}</Badge>}
                  {g.revoke_old && <Badge variant="outline">{t("tiers.revoke_old_short")}</Badge>}
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent className="max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>{edit?.id ? t("tiers.edit") : t("tiers.add")}</DialogTitle><DialogDescription>{t("tiers.form_hint")}</DialogDescription></DialogHeader>
          {edit && (
            <form id="tier-form" className="grid gap-4" onSubmit={(e) => { e.preventDefault(); if (tierValid(edit.d)) save.mutate(edit); }}>
              <Field id="tg-name" label={t("tiers.name")} required maxLength={100} placeholder={t("tiers.name_placeholder")} value={edit.d.name} onChange={(e) => set({ name: e.target.value })} />
              <fieldset className="grid gap-2" data-testid="tier-order">
                <legend className="mb-1 text-sm font-medium">{t("tiers.order")}</legend>
                {edit.d.product_ids.length === 0 && <p className="text-sm text-muted-foreground">{t("tiers.order_empty")}</p>}
                <ol className="grid gap-1.5">
                  {edit.d.product_ids.map((id, i) => (
                    <li key={id} className="flex items-center gap-2 rounded-md border px-2 py-1.5">
                      <span className="w-6 text-center text-xs tabular-nums text-muted-foreground">{i + 1}</span>
                      <span className="min-w-0 flex-1 truncate text-sm">{names.get(id) ?? id}</span>
                      <Button type="button" size="icon" variant="ghost" aria-label={t("tiers.move_up")} disabled={i === 0} onClick={() => set({ product_ids: moveTier(edit.d.product_ids, i, -1) })}><ArrowUpIcon aria-hidden /></Button>
                      <Button type="button" size="icon" variant="ghost" aria-label={t("tiers.move_down")} disabled={i === edit.d.product_ids.length - 1} onClick={() => set({ product_ids: moveTier(edit.d.product_ids, i, 1) })}><ArrowDownIcon aria-hidden /></Button>
                      <Button type="button" size="icon" variant="ghost" aria-label={t("tiers.remove")} onClick={() => set({ product_ids: edit.d.product_ids.filter((x) => x !== id) })}><XIcon aria-hidden /></Button>
                    </li>
                  ))}
                </ol>
                {edit.d.product_ids.length < MAX_TIERS && addable.length > 0 && (
                  <div className="flex gap-2">
                    <div className="flex-1"><SelectField id="tg-add" label={t("tiers.add_tier")} value={add} onChange={setAdd} options={addable.map((p) => ({ value: p.id, label: p.name }))} /></div>
                    <Button type="button" variant="outline" className="self-end" disabled={!add} onClick={() => { set({ product_ids: [...edit.d.product_ids, add] }); setAdd(""); }}><PlusIcon aria-hidden />{t("common.add")}</Button>
                  </div>
                )}
              </fieldset>
              <SelectField id="tg-mode" label={t("tiers.credit_mode")} value={edit.d.credit_mode} onChange={(v) => set({ credit_mode: v as TierDraft["credit_mode"] })}
                options={CREDIT_MODES.map((m) => ({ value: m, label: t(`tiers.mode.${m}`) }))} />
              <p className="-mt-2 text-xs text-muted-foreground">{t(`tiers.mode_hint.${edit.d.credit_mode}`)}</p>
              <Field id="tg-min" label={t("tiers.min_price")} inputMode="decimal" value={edit.d.min_price} onChange={(e) => set({ min_price: e.target.value.replace(",", ".") })} hint={t("tiers.min_price_hint")} />
              <div className="flex items-start gap-2"><Switch id="tg-cancel" checked={edit.d.cancel_old_recurring} onCheckedChange={(v) => set({ cancel_old_recurring: v })} /><Label htmlFor="tg-cancel" className="grid gap-0.5 font-normal"><span className="font-medium">{t("tiers.cancel_old")}</span><span className="text-xs text-muted-foreground">{t("tiers.cancel_old_hint")}</span></Label></div>
              <div className="flex items-start gap-2"><Switch id="tg-revoke" checked={edit.d.revoke_old} onCheckedChange={(v) => set({ revoke_old: v })} /><Label htmlFor="tg-revoke" className="grid gap-0.5 font-normal"><span className="font-medium">{t("tiers.revoke_old")}</span><span className="text-xs text-muted-foreground">{t("tiers.revoke_old_hint")}</span></Label></div>
              {!tierValid(edit.d) && <p role="alert" className="text-sm text-destructive">{t("error.tier_group_invalid")}</p>}
            </form>
          )}
          <DialogFooter><Button type="submit" form="tier-form" disabled={!edit || !tierValid(edit.d) || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <AlertDialog open={!!del} onOpenChange={(o) => !o && setDel(null)}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>{t("tiers.confirm_delete", { name: del?.name ?? "" })}</AlertDialogTitle><AlertDialogDescription>{t("tiers.confirm_delete_hint")}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={() => { if (del) remove.mutate(del.id); setDel(null); }}>{t("common.delete")}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
