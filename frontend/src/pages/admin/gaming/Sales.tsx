import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { LoadError } from "@/components/LoadError";
import { PageHeader } from "@/components/PageHeader";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { fromLocalInput, toLocalInput, type Sale } from "@/lib/sales";
import { useCatalog } from "@/pages/store/Store";

type Form = { name: string; percent: string; product_ids: string[]; group_ids: string[]; starts: string; ends: string; enabled: boolean };

function blank(): Form {
  const now = new Date();
  const end = new Date(now.getTime() + 3 * 86_400_000);
  return { name: "", percent: "20", product_ids: [], group_ids: [], starts: toLocalInput(now.toISOString()), ends: toLocalInput(end.toISOString()), enabled: true };
}

/** Client mirror of the server checks. */
function saleFormValid(f: Form): boolean {
  const pct = Number(f.percent);
  return f.name.trim().length > 0 && f.name.length <= 80 && /^\d{1,2}(\.\d{1,2})?$/.test(f.percent) && pct > 0 && pct <= 95
    && f.product_ids.length + f.group_ids.length > 0 && !!f.starts && !!f.ends && new Date(f.ends) > new Date(f.starts);
}

const toggle = (list: string[], id: string, on: boolean) => (on ? [...list, id] : list.filter((x) => x !== id));

function status(s: Sale, now = Date.now()): "running" | "scheduled" | "ended" | "off" {
  if (!s.enabled) return "off";
  if (Date.parse(s.ends_at) <= now) return "ended";
  return Date.parse(s.starts_at) > now ? "scheduled" : "running";
}

/** Timed sales: percent off products or groups, no code needed. */
export function AdminSalesPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "sales"], queryFn: () => api<{ items: Sale[] }>("/admin/sales") });
  const catalog = useCatalog();
  const [edit, setEdit] = useState<{ id: string | null; f: Form } | null>(null);
  const [del, setDel] = useState<Sale | null>(null);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin", "sales"] }); void qc.invalidateQueries({ queryKey: ["public", "sales"] }); };
  const save = useMutation({
    mutationFn: ({ id, f }: { id: string | null; f: Form }) => api<Sale>(id ? `/admin/sales/${id}` : "/admin/sales", {
      method: id ? "PUT" : "POST",
      body: { name: f.name.trim(), percent: f.percent, product_ids: f.product_ids, group_ids: f.group_ids, starts_at: fromLocalInput(f.starts), ends_at: fromLocalInput(f.ends), enabled: f.enabled },
    }),
    onSuccess: () => { setEdit(null); refresh(); toast.success(t("market.saved")); },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (id: string) => api(`/admin/sales/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const set = (p: Partial<Form>) => setEdit((e) => (e ? { ...e, f: { ...e.f, ...p } } : e));
  const groups = catalog.data?.groups ?? [];
  const products = catalog.data?.products ?? [];
  const label = { running: t("sales.running"), scheduled: t("sales.scheduled"), ended: t("sales.ended"), off: t("community.disabled") };
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("sales.title")} description={t("sales.desc")}
        actions={<Button onClick={() => setEdit({ id: null, f: blank() })}><PlusIcon aria-hidden />{t("sales.add")}</Button>} />
      {q.isError && !q.data ? <LoadError retry={() => void q.refetch()} /> : !q.data ? <Skeleton className="h-40 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("sales.empty")} /> : (
        <Table>
          <TableHeader><TableRow><TableHead>{t("sales.name")}</TableHead><TableHead>{t("sales.percent")}</TableHead><TableHead>{t("sales.window")}</TableHead><TableHead>{t("market.status")}</TableHead><TableHead /></TableRow></TableHeader>
          <TableBody>
            {q.data.items.map((s) => {
              const st = status(s);
              return (
                <TableRow key={s.id}>
                  <TableCell className="font-medium">{s.name}<div className="text-xs text-muted-foreground">{t("sales.targets", { p: s.product_ids.length, g: s.group_ids.length })}</div></TableCell>
                  <TableCell className="tabular-nums">−{Number(s.percent)}%</TableCell>
                  <TableCell className="text-xs">{formatDateTime(s.starts_at, lang)} → {formatDateTime(s.ends_at, lang)}</TableCell>
                  <TableCell><Badge variant={st === "running" ? "secondary" : "outline"}>{label[st]}</Badge></TableCell>
                  <TableCell className="text-right">
                    <Button size="icon" variant="ghost" aria-label={t("common.edit")} onClick={() => setEdit({ id: s.id, f: { name: s.name, percent: String(Number(s.percent)), product_ids: s.product_ids, group_ids: s.group_ids, starts: toLocalInput(s.starts_at), ends: toLocalInput(s.ends_at), enabled: s.enabled } })}><PencilSimpleIcon aria-hidden /></Button>
                    <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => setDel(s)}><TrashIcon aria-hidden /></Button>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent className="max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>{edit?.id ? t("sales.edit") : t("sales.add")}</DialogTitle></DialogHeader>
          {edit && (
            <form id="sale-form" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); if (saleFormValid(edit.f)) save.mutate(edit); }}>
              <Field id="sl-name" label={t("sales.name")} required maxLength={80} placeholder={t("sales.name_placeholder")} value={edit.f.name} onChange={(e) => set({ name: e.target.value })} />
              <Field id="sl-pct" label={t("sales.percent")} inputMode="decimal" value={edit.f.percent} onChange={(e) => set({ percent: e.target.value.replace(",", ".") })} />
              <div className="grid gap-3 sm:grid-cols-2">
                <Field id="sl-start" label={t("sales.starts")} type="datetime-local" value={edit.f.starts} onChange={(e) => set({ starts: e.target.value })} />
                <Field id="sl-end" label={t("sales.ends")} type="datetime-local" value={edit.f.ends} onChange={(e) => set({ ends: e.target.value })} />
              </div>
              <fieldset className="grid gap-2">
                <legend className="mb-1 text-sm font-medium">{t("sales.groups")}</legend>
                {groups.map((g) => (
                  <div key={g.id} className="flex items-center gap-2"><Checkbox id={`sl-g-${g.id}`} checked={edit.f.group_ids.includes(g.id)} onCheckedChange={(v) => set({ group_ids: toggle(edit.f.group_ids, g.id, v === true) })} /><Label htmlFor={`sl-g-${g.id}`}>{g.name}</Label></div>
                ))}
              </fieldset>
              <fieldset className="grid max-h-48 gap-2 overflow-y-auto">
                <legend className="mb-1 text-sm font-medium">{t("sales.products")}</legend>
                {products.map((p) => (
                  <div key={p.id} className="flex items-center gap-2"><Checkbox id={`sl-p-${p.id}`} checked={edit.f.product_ids.includes(p.id)} onCheckedChange={(v) => set({ product_ids: toggle(edit.f.product_ids, p.id, v === true) })} /><Label htmlFor={`sl-p-${p.id}`}>{p.name}</Label></div>
                ))}
              </fieldset>
              <div className="flex items-center gap-2"><Switch id="sl-on" checked={edit.f.enabled} onCheckedChange={(v) => set({ enabled: v })} /><Label htmlFor="sl-on">{t("sales.enabled")}</Label></div>
              <p className="text-xs text-muted-foreground">{t("sales.hint")}</p>
              {!saleFormValid(edit.f) && <p role="alert" className="text-sm text-destructive">{t("error.sale_invalid")}</p>}
            </form>
          )}
          <DialogFooter><Button type="submit" form="sale-form" disabled={!edit || !saleFormValid(edit.f) || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <AlertDialog open={!!del} onOpenChange={(o) => !o && setDel(null)}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>{t("sales.confirm_delete", { name: del?.name ?? "" })}</AlertDialogTitle><AlertDialogDescription>{t("sales.confirm_delete_hint")}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={() => { if (del) remove.mutate(del.id); setDel(null); }}>{t("common.delete")}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
