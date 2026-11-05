import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BankIcon, NotePencilIcon, PlusIcon, StarIcon, TrashIcon, XIcon } from "@phosphor-icons/react";
import { FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { emptyEntity, entityPayload, formatNumber, type EntityDraft, type LegalEntity } from "@/lib/billing/legalEntities";

const errMsg = (e: unknown, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null);

function EntityDialog({ e }: { e?: LegalEntity }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const init: EntityDraft = e ? { ...e, due_days: e.due_days === null ? "" : String(e.due_days) } : emptyEntity;
  const [d, setD] = useState<EntityDraft>(init);
  const [utcYear] = useState(() => new Date().getUTCFullYear());
  const body = entityPayload(d);
  const preview = formatNumber(d.number_format || emptyEntity.number_format, d.invoice_prefix || "INV", utcYear, 1);
  const m = useMutation({
    mutationFn: () => api(e ? `/admin/legal-entities/${e.id}` : "/admin/legal-entities", { method: e ? "PUT" : "POST", body }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "legal-entities"] }); },
  });
  const field = (k: keyof EntityDraft, label: string, extra: { max?: number; mono?: boolean } = {}) => (
    <div className="grid gap-1.5">
      <Label htmlFor={`le-${k}`}>{label}</Label>
      <Input id={`le-${k}`} maxLength={extra.max} className={extra.mono ? "font-mono" : undefined} value={d[k]} onChange={(ev) => setD({ ...d, [k]: ev.target.value })} />
    </div>
  );
  const area = (k: keyof EntityDraft, label: string) => (
    <div className="grid gap-1.5 sm:col-span-2">
      <Label htmlFor={`le-${k}`}>{label}</Label>
      <Textarea id={`le-${k}`} rows={2} value={d[k]} onChange={(ev) => setD({ ...d, [k]: ev.target.value })} />
    </div>
  );
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setD(init); m.reset(); } }}>
      <DialogTrigger asChild>{e ? <Button size="icon" variant="ghost" aria-label={t("common.edit")}><NotePencilIcon aria-hidden /></Button> : <Button><PlusIcon aria-hidden />{t("entity.add")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{e ? e.name : t("entity.add")}</DialogTitle><DialogDescription>{t("entity.dialog_hint")}</DialogDescription></DialogHeader>
        <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
          {field("name", t("entity.name"), { max: 200 })}
          {field("tax_id", t("entity.tax_id"), { max: 100 })}
          {area("address", t("entity.address"))}
          {field("invoice_prefix", t("entity.invoice_prefix"), { max: 12, mono: true })}
          {field("credit_prefix", t("entity.credit_prefix"), { max: 12, mono: true })}
          <div className="grid gap-1.5">
            <Label htmlFor="le-number_format">{t("entity.number_format")}</Label>
            <Input id="le-number_format" className="font-mono" maxLength={64} value={d.number_format} onChange={(ev) => setD({ ...d, number_format: ev.target.value })} />
            <p className="text-xs text-muted-foreground">{preview ? t("entity.preview", { n: preview }) : t("entity.format_help")}</p>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="le-due_days">{t("entity.due_days")}</Label>
            <Input id="le-due_days" inputMode="numeric" placeholder={t("entity.due_default")} value={d.due_days} onChange={(ev) => setD({ ...d, due_days: ev.target.value })} />
          </div>
          {field("bank_name", t("entity.bank_name"), { max: 200 })}
          {field("iban", "IBAN", { max: 64, mono: true })}
          {field("swift", "SWIFT/BIC", { max: 16, mono: true })}
          {area("footer", t("entity.footer"))}
          {area("terms", t("entity.terms"))}
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={!body || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminLegalEntitiesPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "legal-entities"], queryFn: () => api<LegalEntity[]>("/admin/legal-entities") });
  const inv = () => void qc.invalidateQueries({ queryKey: ["admin", "legal-entities"] });
  const def = useMutation({ mutationFn: (id: string) => api(`/admin/legal-entities/${id}/default`, { method: "POST" }), onSuccess: inv });
  const clear = useMutation({ mutationFn: () => api("/admin/legal-entities/default", { method: "DELETE" }), onSuccess: inv });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/legal-entities/${id}`, { method: "DELETE" }), onSuccess: inv });
  return (
    <ListPage<LegalEntity>
      title={t("entity.title")} description={t("entity.subtitle")} actions={<EntityDialog />}
      rows={data} rowKey={(e) => e.id} loading={isLoading} emptyIcon={BankIcon} emptyTitle={t("entity.empty")}
      columns={[
        { header: t("entity.name"), primary: true, cell: (e) => <span className="flex items-center gap-2 font-medium">{e.name}{e.is_default && <Badge>{t("entity.default")}</Badge>}</span> },
        { header: t("entity.next_number"), cell: (e) => <span className="font-mono text-sm">{e.next_number}</span> },
        { header: t("entity.due_days"), cell: (e) => (e.due_days === null ? t("entity.due_default") : t("entity.days", { n: String(e.due_days) })) },
        { header: "IBAN", cell: (e) => <span className="font-mono text-sm">{e.iban || "-"}</span> },
        { header: "", cell: (e) => (
          <div className="flex justify-end gap-1">
            {e.is_default
              ? <Button size="sm" variant="outline" disabled={clear.isPending} onClick={() => clear.mutate()}><XIcon aria-hidden />{t("entity.clear_default")}</Button>
              : <Button size="sm" variant="outline" disabled={def.isPending} onClick={() => def.mutate(e.id)}><StarIcon aria-hidden />{t("entity.make_default")}</Button>}
            <EntityDialog e={e} />
            <Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={del.isPending} onClick={() => del.mutate(e.id)}><TrashIcon aria-hidden /></Button>
          </div>
        ) },
      ]}
    >
      <FormError message={errMsg(def.error ?? clear.error ?? del.error, tCode)} />
    </ListPage>
  );
}
