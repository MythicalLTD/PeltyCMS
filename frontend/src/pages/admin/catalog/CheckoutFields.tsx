import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownIcon, ArrowUpIcon, CheckIcon, FloppyDiskIcon, LightningIcon, LockKeyIcon, PencilSimpleIcon, PlusIcon, PuzzlePieceIcon, TextboxIcon, TrashIcon } from "@phosphor-icons/react";
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
import {
  EMPTY_FIELD, FIELD_KINDS, fieldBody, fieldDraft, fieldDraftValid, fieldKey, IDENTITY_KEYS, keyValid, moduleMissing, moveField, PATTERN_KINDS, presetMissing, rankPresets, suggestKey,
  type FieldDraft, type FieldPreset, type ModuleUse, type PackageField, type PresetField,
} from "@/lib/store/checkoutFields";

const NONE = "__none";
interface FieldsView { items: PackageField[]; games: string[]; module: string | null; module_uses: ModuleUse[] }

/** Checkout fields of one package: the custom-field kit (Tebex-style fields, read by provisioning modules and delivery commands). */
export function ProductCheckoutFieldsDialog({ productId, productName }: { productId: string; productName: string }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><TextboxIcon aria-hidden />{t("pfields.button")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader><DialogTitle>{t("pfields.title", { name: productName })}</DialogTitle><DialogDescription>{t("pfields.desc")}</DialogDescription></DialogHeader>
        {open && <CheckoutFieldsEditor productId={productId} />}
      </DialogContent>
    </Dialog>
  );
}

/** Translated name of a preset / module field (player IDs use the game field names). */
function useKitName() {
  const { t } = useI18n();
  return (f: Pick<PresetField, "key" | "name" | "identity_key">) => {
    if (f.identity_key) return t(`game.field.${f.identity_key}` as TranslationKey);
    const k = `pfields.preset_field.${f.key}` as TranslationKey;
    const v = t(k);
    return v === k ? f.name : v;
  };
}

export function CheckoutFieldsEditor({ productId }: { productId: string }) {
  const { t } = useI18n();
  const key = ["admin", "product-fields", productId];
  const qc = useQueryClient();
  const q = useQuery({ queryKey: key, queryFn: () => api<FieldsView>(`/admin/catalog/products/${productId}/fields`) });
  const [editing, setEditing] = useState<string | null>(null);
  const items = q.data?.items ?? [];
  const ids = items.filter((f) => !f.module_key).map((f) => f.id);
  const reorder = useMutation({
    mutationFn: (next: string[]) => api(`/admin/catalog/products/${productId}/fields/order`, { method: "PUT", body: { ids: next } }),
    onSettled: () => void qc.invalidateQueries({ queryKey: key }),
  });
  return (
    <div className="grid gap-4" data-testid="package-fields">
      <PresetBar productId={productId} view={q.data} />
      {q.data && q.data.module_uses.length > 0 && <ModuleNeeds productId={productId} view={q.data} />}
      <div className="grid gap-2">
        {q.data && items.length === 0 && editing !== "new" && <p className="rounded-md border border-dashed p-4 text-center text-sm text-muted-foreground">{t("pfields.empty")}</p>}
        {items.map((f) => editing === f.id
          ? <FieldForm key={f.id} productId={productId} field={f} onDone={() => setEditing(null)} />
          : <FieldRow key={f.id} productId={productId} field={f} onEdit={() => setEditing(f.id)}
              canUp={!f.module_key && ids.indexOf(f.id) > 0} canDown={!f.module_key && ids.indexOf(f.id) < ids.length - 1}
              onMove={(dir) => reorder.mutate(moveField(ids, f.id, dir))} moving={reorder.isPending} />)}
      </div>
      {editing === "new"
        ? <FieldForm productId={productId} onDone={() => setEditing(null)} />
        : <Button variant="outline" className="justify-self-start" onClick={() => setEditing("new")}><PlusIcon aria-hidden />{t("pfields.add")}</Button>}
    </div>
  );
}

/** One-click presets: game-aware (suggested for the store's games), only missing fields are added. */
function PresetBar({ productId, view }: { productId: string; view?: FieldsView }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const name = useKitName();
  const presets = useQuery({ queryKey: ["admin", "field-presets"], queryFn: () => api<{ items: FieldPreset[] }>("/admin/catalog/field-presets"), staleTime: Infinity });
  const apply = useMutation({
    mutationFn: (p: FieldPreset) => api<{ added: PackageField[] }>(`/admin/catalog/products/${productId}/fields/preset`, { method: "POST", body: { preset: p.id, names: Object.fromEntries(p.fields.map((f) => [f.key, name(f)])) } }),
    onSuccess: async (r) => { await qc.invalidateQueries({ queryKey: ["admin", "product-fields", productId] }); toast.success(t("pfields.preset_added", { n: r.added.length })); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!presets.data || !view) return null;
  const ranked = rankPresets(presets.data.items, view.games);
  return (
    <section className="grid gap-2 rounded-lg border bg-muted/30 p-3" aria-labelledby={`presets-${productId}`} data-testid="field-presets">
      <div className="flex items-center gap-2">
        <LightningIcon aria-hidden className="text-primary" />
        <h3 id={`presets-${productId}`} className="text-sm font-medium">{t("pfields.presets")}</h3>
      </div>
      <p className="text-xs text-muted-foreground">{t("pfields.presets_hint")}</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {ranked.map(({ preset: p, suggested }) => {
          const miss = presetMissing(p, view.items);
          return (
            <div key={p.id} className="flex items-start gap-3 rounded-md border bg-background p-3" data-preset={p.id}>
              <div className="min-w-0 flex-1">
                <p className="flex flex-wrap items-center gap-1.5 text-sm font-medium">
                  {t(`pfields.preset.${p.id}` as TranslationKey)}
                  {suggested && <Badge variant="secondary">{t("pfields.suggested")}</Badge>}
                </p>
                <p className="mt-1 text-xs text-muted-foreground">{p.fields.map((f) => name(f)).join(" · ")}</p>
              </div>
              <Button size="sm" variant={suggested ? "default" : "outline"} disabled={miss.length === 0 || apply.isPending} onClick={() => apply.mutate(p)}
                aria-label={t("pfields.apply_preset", { name: t(`pfields.preset.${p.id}` as TranslationKey) })}>
                {miss.length === 0 ? <><CheckIcon aria-hidden />{t("pfields.preset_done")}</> : <><PlusIcon aria-hidden />{t("pfields.preset_add", { n: miss.length })}</>}
              </Button>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/** What the product's provisioning module reads from the kit, with one click to add the missing keys. */
function ModuleNeeds({ productId, view }: { productId: string; view: FieldsView }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const miss = moduleMissing(view.module_uses, view.items);
  const add = useMutation({
    mutationFn: async () => {
      for (const [i, u] of miss.entries()) {
        const kind = u.kind || "text";
        const identity = (kind === "text" || kind === "number") && u.identity_key ? u.identity_key : "";
        await api(`/admin/catalog/products/${productId}/fields`, { method: "POST", body: fieldBody({ ...EMPTY_FIELD, name: u.name || u.key, kind, required: u.required, identity_key: identity, key: u.key, sort_order: 100 + i }) });
      }
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: ["admin", "product-fields", productId] }),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <section className="grid gap-2 rounded-lg border p-3" data-testid="module-needs">
      <div className="flex flex-wrap items-center gap-2">
        <PuzzlePieceIcon aria-hidden className="text-primary" />
        <h3 className="text-sm font-medium">{t("pfields.module_reads", { module: view.module ?? "" })}</h3>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {view.module_uses.map((u) => {
          const has = !miss.includes(u);
          return <Badge key={u.key} variant={has ? "secondary" : "outline"} className="font-mono">{has && <CheckIcon aria-hidden />}{u.key}{u.required ? " *" : ""}</Badge>;
        })}
      </div>
      {miss.length > 0 && <Button size="sm" variant="outline" className="justify-self-start" disabled={add.isPending} onClick={() => add.mutate()}><PlusIcon aria-hidden />{t("pfields.module_add_missing", { n: miss.length })}</Button>}
    </section>
  );
}

function FieldRow({ productId, field, onEdit, canUp, canDown, onMove, moving }: { productId: string; field: PackageField; onEdit: () => void; canUp: boolean; canDown: boolean; onMove: (dir: -1 | 1) => void; moving: boolean }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const del = useMutation({
    mutationFn: () => api(`/admin/catalog/products/${productId}/fields/${field.id}`, { method: "DELETE" }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "product-fields", productId] }),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const k = fieldKey(field);
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-md border p-3" data-field-row data-key={k ?? undefined}>
      <div className="flex flex-col">
        <Button size="icon" variant="ghost" className="size-7" aria-label={t("pfields.move_up")} disabled={!canUp || moving} onClick={() => onMove(-1)}><ArrowUpIcon aria-hidden /></Button>
        <Button size="icon" variant="ghost" className="size-7" aria-label={t("pfields.move_down")} disabled={!canDown || moving} onClick={() => onMove(1)}><ArrowDownIcon aria-hidden /></Button>
      </div>
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-1.5 text-sm font-medium">
          {field.kind === "password" && <LockKeyIcon aria-hidden className="text-muted-foreground" />}
          {field.name}{field.required && <span className="text-destructive"> *</span>}
        </p>
        <div className="mt-1 flex flex-wrap gap-1.5">
          <Badge variant="outline">{t(`pfields.kind.${field.kind}` as TranslationKey)}</Badge>
          {k && <Badge variant="outline" className="font-mono" title={t("pfields.key_hint")}>{`{${k}}`}</Badge>}
          {field.identity_key && <Badge variant="secondary">{t("pfields.autofill_badge", { field: t(`game.field.${field.identity_key}` as TranslationKey) })}</Badge>}
          {field.module_key && <Badge variant="secondary">{t("pfields.from_module")}</Badge>}
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
  const moduleOwned = !!field?.module_key;
  return (
    <form className="grid gap-3 rounded-md border border-primary/40 p-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field id={`pf-name-${fid}`} required maxLength={120} label={t("pfields.name")} placeholder={t("pfields.name_placeholder")} value={d.name} onChange={(e) => set({ name: e.target.value })} />
        <SelectField id={`pf-kind-${fid}`} label={t("pfields.kind_label")} value={d.kind} onChange={(v) => set({ kind: v, identity_key: v === "text" || v === "number" ? d.identity_key : "" })}
          options={FIELD_KINDS.map((k) => ({ value: k, label: t(`pfields.kind.${k}` as TranslationKey) }))} />
        {keyed && (
          <div className="grid gap-1 sm:col-span-2">
            <SelectField id={`pf-id-${fid}`} label={t("pfields.autofill")} value={d.identity_key || NONE} onChange={(v) => set({ identity_key: v === NONE ? "" : v })}
              options={[{ value: NONE, label: t("pfields.autofill_none") }, ...IDENTITY_KEYS.map((k) => ({ value: k, label: t(`game.field.${k}` as TranslationKey) }))]} />
            <p className="text-xs text-muted-foreground">{t("pfields.autofill_hint")}</p>
          </div>
        )}
        {d.kind === "password" && <p className="rounded-md bg-muted/50 p-2 text-xs text-muted-foreground sm:col-span-2" data-testid="password-note"><LockKeyIcon aria-hidden className="mr-1 inline" />{t("pfields.password_hint")}</p>}
        {d.kind === "select" && (
          <div className="grid gap-2 sm:col-span-2"><Label htmlFor={`pf-ch-${fid}`}>{t("pfields.choices")}</Label><Textarea id={`pf-ch-${fid}`} rows={3} value={d.choices} onChange={(e) => set({ choices: e.target.value })} /><p className="text-xs text-muted-foreground">{t("pfields.choices_hint")}</p></div>
        )}
        {moduleOwned
          ? <p className="text-xs text-muted-foreground sm:col-span-2">{t("pfields.module_key", { key: field!.module_key! })}</p>
          : <Field id={`pf-key-${fid}`} maxLength={40} className="font-mono" label={t("pfields.key")} placeholder={suggestKey(d.name, d.identity_key || null)} hint={t("pfields.key_hint")}
              error={keyValid(d.key) ? undefined : t("error.invalid_field_key")} value={d.key} onChange={(e) => set({ key: e.target.value.toLowerCase() })} />}
        {PATTERN_KINDS.includes(d.kind) && <Field id={`pf-pat-${fid}`} maxLength={200} className="font-mono" label={t("pfields.pattern")} hint={t("pfields.pattern_hint")} value={d.pattern} onChange={(e) => set({ pattern: e.target.value })} />}
        <Field id={`pf-desc-${fid}`} className="sm:col-span-2" maxLength={500} label={t("pfields.help")} value={d.description} onChange={(e) => set({ description: e.target.value })} />
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
