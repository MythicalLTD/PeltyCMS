import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FloppyDiskIcon, PencilSimpleIcon, PlusIcon, TextboxIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field, FormError } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { EMPTY_FIELD, FIELD_KINDS, fieldBody, fieldDraft, fieldDraftValid, IDENTITY_KEYS, type FieldDraft, type PackageField } from "@/lib/store/checkoutFields";

const NONE = "__none";

/** Checkout fields of one package (custom fields asked at checkout, Tebex-style). */
export function ProductCheckoutFieldsDialog({ productId, productName }: { productId: string; productName: string }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><TextboxIcon aria-hidden />{t("pfields.button")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("pfields.title", { name: productName })}</DialogTitle><DialogDescription>{t("pfields.desc")}</DialogDescription></DialogHeader>
        {open && <CheckoutFieldsEditor productId={productId} />}
      </DialogContent>
    </Dialog>
  );
}

export function CheckoutFieldsEditor({ productId }: { productId: string }) {
  const { t } = useI18n();
  const key = ["admin", "product-fields", productId];
  const q = useQuery({ queryKey: key, queryFn: () => api<{ items: PackageField[] }>(`/admin/catalog/products/${productId}/fields`) });
  const [editing, setEditing] = useState<string | null>(null);
  const items = q.data?.items ?? [];
  return (
    <div className="grid gap-3" data-testid="package-fields">
      {q.data && items.length === 0 && editing !== "new" && <p className="text-sm text-muted-foreground">{t("pfields.empty")}</p>}
      {items.map((f) => editing === f.id
        ? <FieldForm key={f.id} productId={productId} field={f} onDone={() => setEditing(null)} />
        : <FieldRow key={f.id} productId={productId} field={f} onEdit={() => setEditing(f.id)} />)}
      {editing === "new"
        ? <FieldForm productId={productId} onDone={() => setEditing(null)} />
        : <Button variant="outline" className="justify-self-start" onClick={() => setEditing("new")}><PlusIcon aria-hidden />{t("pfields.add")}</Button>}
    </div>
  );
}

function FieldRow({ productId, field, onEdit }: { productId: string; field: PackageField; onEdit: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const del = useMutation({
    mutationFn: () => api(`/admin/catalog/products/${productId}/fields/${field.id}`, { method: "DELETE" }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "product-fields", productId] }),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-md border p-3" data-field-row>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{field.name}{field.required && <span className="text-destructive"> *</span>}</p>
        <div className="mt-1 flex flex-wrap gap-1.5">
          <Badge variant="outline">{t(`pfields.kind.${field.kind}` as TranslationKey)}</Badge>
          {field.identity_key && <Badge variant="secondary">{t("pfields.autofill_badge", { field: t(`game.field.${field.identity_key}` as TranslationKey) })}</Badge>}
          {field.admin_only && <Badge variant="outline">{t("pfields.admin_only")}</Badge>}
        </div>
      </div>
      <Button size="sm" variant="ghost" aria-label={t("common.edit")} onClick={onEdit}><PencilSimpleIcon aria-hidden /></Button>
      <Button size="sm" variant="ghost" aria-label={t("common.delete")} disabled={del.isPending} onClick={() => del.mutate()}><TrashIcon aria-hidden /></Button>
    </div>
  );
}

function FieldForm({ productId, field, onDone }: { productId: string; field?: PackageField; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [d, setD] = useState<FieldDraft>(field ? fieldDraft(field) : EMPTY_FIELD);
  const set = (p: Partial<FieldDraft>) => setD((x) => ({ ...x, ...p }));
  const save = useMutation({
    mutationFn: () => api(`/admin/catalog/products/${productId}/fields${field ? `/${field.id}` : ""}`, { method: field ? "PUT" : "POST", body: fieldBody(d) }),
    onSuccess: async () => { await qc.invalidateQueries({ queryKey: ["admin", "product-fields", productId] }); toast.success(t("common.saved")); onDone(); },
  });
  const keyed = d.kind === "text" || d.kind === "number";
  const err = save.error ? (save.error instanceof ApiError ? tCode(save.error.code) : t("error.internal")) : null;
  const fid = field?.id ?? "new";
  return (
    <form className="grid gap-3 rounded-md border p-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field id={`pf-name-${fid}`} required maxLength={120} label={t("pfields.name")} placeholder={t("pfields.name_placeholder")} value={d.name} onChange={(e) => set({ name: e.target.value })} />
        <SelectField id={`pf-kind-${fid}`} label={t("pfields.kind_label")} value={d.kind} onChange={(v) => set({ kind: v })}
          options={FIELD_KINDS.map((k) => ({ value: k, label: t(`pfields.kind.${k}` as TranslationKey) }))} />
        {keyed && (
          <div className="grid gap-1 sm:col-span-2">
            <SelectField id={`pf-id-${fid}`} label={t("pfields.autofill")} value={d.identity_key || NONE} onChange={(v) => set({ identity_key: v === NONE ? "" : v })}
              options={[{ value: NONE, label: t("pfields.autofill_none") }, ...IDENTITY_KEYS.map((k) => ({ value: k, label: t(`game.field.${k}` as TranslationKey) }))]} />
            <p className="text-xs text-muted-foreground">{t("pfields.autofill_hint")}</p>
          </div>
        )}
        {d.kind === "select" && (
          <div className="grid gap-2 sm:col-span-2"><Label htmlFor={`pf-ch-${fid}`}>{t("pfields.choices")}</Label><Textarea id={`pf-ch-${fid}`} rows={3} value={d.choices} onChange={(e) => set({ choices: e.target.value })} /><p className="text-xs text-muted-foreground">{t("pfields.choices_hint")}</p></div>
        )}
        <Field id={`pf-desc-${fid}`} className="sm:col-span-2" maxLength={500} label={t("pfields.help")} value={d.description} onChange={(e) => set({ description: e.target.value })} />
        {(d.kind === "text" || d.kind === "textarea") && <Field id={`pf-pat-${fid}`} maxLength={200} className="font-mono" label={t("pfields.pattern")} hint={t("pfields.pattern_hint")} value={d.pattern} onChange={(e) => set({ pattern: e.target.value })} />}
      </div>
      <div className="flex flex-wrap items-center gap-6">
        <div className="flex items-center gap-2"><Switch id={`pf-req-${fid}`} checked={d.required} onCheckedChange={(v) => set({ required: v })} /><Label htmlFor={`pf-req-${fid}`}>{t("pfields.required")}</Label></div>
        <div className="flex items-center gap-2"><Switch id={`pf-adm-${fid}`} checked={d.admin_only} onCheckedChange={(v) => set({ admin_only: v })} /><Label htmlFor={`pf-adm-${fid}`}>{t("pfields.admin_only")}</Label></div>
      </div>
      <FormError message={err} />
      <div className="flex gap-2">
        <Button type="submit" disabled={!fieldDraftValid(d) || save.isPending}><FloppyDiskIcon aria-hidden />{t("common.save")}</Button>
        <Button type="button" variant="ghost" onClick={onDone}>{t("common.cancel")}</Button>
      </div>
    </form>
  );
}
