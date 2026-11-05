import { useState } from "react";
import { EInvoiceCard } from "@/pages/admin/billing/EInvoice";
import { toast } from "sonner";
import { PencilSimpleIcon, PlusIcon, ReceiptIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ListCard } from "@/components/ListPage";
import { PageHeader } from "@/components/PageHeader";
import { Field, FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatNumber } from "@/lib/format";
import { CountryRegionFields, CountrySelect } from "@/components/geo/CountryRegionFields";
import { useI18n } from "@/lib/i18n";

export interface TaxSettings { enabled: boolean; inclusive: boolean; home_country: string; reverse_charge: boolean }
export interface TaxRule { id: string; name: string; country: string; region: string; rate: string; compound: boolean; is_vat: boolean; priority: number; enabled: boolean }
type RuleForm = Omit<TaxRule, "id">;
const EMPTY_RULE: RuleForm = { name: "", country: "", region: "", rate: "", compound: false, is_vat: true, priority: 0, enabled: true };

function SwitchRow({ id, label, hint, checked, onChange }: { id: string; label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="flex flex-col gap-1">
        <Label htmlFor={id}>{label}</Label>
        {hint && <p className="text-sm text-muted-foreground">{hint}</p>}
      </div>
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

function SettingsCard() {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "settings", "tax"], queryFn: () => api<TaxSettings>("/admin/settings/tax") });
  const [form, setForm] = useState<TaxSettings>(data ?? { enabled: false, inclusive: false, home_country: "", reverse_charge: false });
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setForm(data);
  }
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (b: TaxSettings) => api<TaxSettings>("/admin/settings/tax", { method: "PUT", body: b }),
    onSuccess: (b) => { setForm(b); setError(null); toast.success(t("common.saved")); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader><CardTitle>{t("admin.tax.settings")}</CardTitle></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
          <FormError message={error} />
          <SwitchRow id="tax-enabled" label={t("admin.tax.enabled")} checked={form.enabled} onChange={(v) => setForm({ ...form, enabled: v })} />
          <SwitchRow id="tax-inclusive" label={t("admin.tax.inclusive")} hint={t("admin.tax.inclusive_hint")} checked={form.inclusive} onChange={(v) => setForm({ ...form, inclusive: v })} />
          <SwitchRow id="tax-rc" label={t("admin.tax.reverse_charge")} hint={t("admin.tax.reverse_charge_hint")} checked={form.reverse_charge} onChange={(v) => setForm({ ...form, reverse_charge: v })} />
          <div className="max-w-sm grid gap-1.5">
            <Label htmlFor="tax-home">{t("admin.tax.home_country")}</Label>
            <CountrySelect id="tax-home" value={form.home_country} onChange={(home_country) => setForm({ ...form, home_country })} />
          </div>
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

function RuleDialog({ rule, open, onOpenChange }: { rule: TaxRule | null; open: boolean; onOpenChange: (v: boolean) => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [form, setForm] = useState<RuleForm>(EMPTY_RULE);
  const [error, setError] = useState<string | null>(null);
  const [gate, setGate] = useState({ open, rule });
  if (open !== gate.open || rule !== gate.rule) {
    setGate({ open, rule });
    if (open) { setForm(rule ? { ...rule } : EMPTY_RULE); setError(null); }
  }
  const save = useMutation({
    mutationFn: (b: RuleForm) => api<TaxRule>(rule ? `/admin/tax-rules/${rule.id}` : "/admin/tax-rules", { method: rule ? "PUT" : "POST", body: { ...b, priority: Number(b.priority) || 0 } }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["admin", "tax-rules"] }); onOpenChange(false); toast.success(t("common.saved")); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const text = (k: "name" | "rate") => ({ value: form[k], onChange: (e: { target: { value: string } }) => setForm({ ...form, [k]: e.target.value }) });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{rule ? t("admin.tax.edit_rule") : t("admin.tax.add_rule")}</DialogTitle></DialogHeader>
        <form id="tax-rule-form" className="grid gap-3 *:min-w-0 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
          <div className="sm:col-span-2"><FormError message={error} /></div>
          <div className="sm:col-span-2"><Field id="rule-name" label={t("admin.tax.name")} maxLength={100} required {...text("name")} /></div>
          <div className="sm:col-span-2">
            <CountryRegionFields
              country={form.country}
              region={form.region}
              onCountry={(country) => setForm({ ...form, country, region: "" })}
              onRegion={(region) => setForm({ ...form, region })}
              countryId="rule-country"
              regionId="rule-region"
            />
            <p className="mt-1 text-xs text-muted-foreground">{t("admin.tax.country_hint")}</p>
          </div>
          <Field id="rule-rate" label={t("admin.tax.rate")} inputMode="decimal" required {...text("rate")} />
          <Field id="rule-priority" label={t("admin.tax.priority")} type="number" value={String(form.priority)} onChange={(e) => setForm({ ...form, priority: Number(e.target.value) })} />
          <div className="flex flex-col gap-3 sm:col-span-2">
            <SwitchRow id="rule-compound" label={t("admin.tax.compound")} hint={t("admin.tax.compound_hint")} checked={form.compound} onChange={(v) => setForm({ ...form, compound: v })} />
            <SwitchRow id="rule-vat" label={t("admin.tax.is_vat")} checked={form.is_vat} onChange={(v) => setForm({ ...form, is_vat: v })} />
            <SwitchRow id="rule-enabled" label={t("admin.tax.rule_enabled")} checked={form.enabled} onChange={(v) => setForm({ ...form, enabled: v })} />
          </div>
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t("common.cancel")}</Button>
          <Button type="submit" form="tax-rule-form" disabled={save.isPending}>{t("common.save")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export interface InvoiceBranding { company_name: string; address: string; tax_id: string; footer: string; accent: string }
export const isHexColor = (s: string) => /^#[0-9a-fA-F]{6}$/.test(s);

function BrandingCard() {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "settings", "invoice-branding"], queryFn: () => api<InvoiceBranding>("/admin/settings/invoice-branding") });
  const [form, setForm] = useState<InvoiceBranding>(data ?? { company_name: "", address: "", tax_id: "", footer: "", accent: "#111827" });
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setForm(data);
  }
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (b: InvoiceBranding) => api<InvoiceBranding>("/admin/settings/invoice-branding", { method: "PUT", body: b }),
    onSuccess: (b) => { setForm(b); setError(null); toast.success(t("common.saved")); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader><CardTitle>{t("admin.branding.title")}</CardTitle></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); if (!isHexColor(form.accent)) { setError(t("admin.branding.invalid_accent")); return; } save.mutate(form); }}>
          <FormError message={error} />
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
            <Field id="br-name" label={t("admin.branding.company_name")} maxLength={200} value={form.company_name} onChange={(e) => setForm({ ...form, company_name: e.target.value })} />
            <Field id="br-tax" label={t("admin.branding.tax_id")} maxLength={100} value={form.tax_id} onChange={(e) => setForm({ ...form, tax_id: e.target.value })} />
          </div>
          <div className="flex flex-col gap-2"><Label htmlFor="br-address">{t("admin.branding.address")}</Label><Textarea id="br-address" rows={3} maxLength={1000} value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} /></div>
          <div className="flex flex-col gap-2"><Label htmlFor="br-footer">{t("admin.branding.footer")}</Label><Textarea id="br-footer" rows={2} maxLength={1000} value={form.footer} onChange={(e) => setForm({ ...form, footer: e.target.value })} /></div>
          <div className="flex items-end gap-2">
            <div className="max-w-40"><Field id="br-accent" label={t("admin.branding.accent")} maxLength={7} value={form.accent} onChange={(e) => setForm({ ...form, accent: e.target.value })} /></div>
            <span aria-hidden className="mb-1 size-8 rounded-md border" style={{ backgroundColor: isHexColor(form.accent) ? form.accent : undefined }} />
          </div>
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

export function AdminTaxPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "tax-rules"], queryFn: () => api<{ items: TaxRule[] }>("/admin/tax-rules") });
  const [editing, setEditing] = useState<TaxRule | null>(null);
  const [open, setOpen] = useState(false);
  const [toDelete, setToDelete] = useState<TaxRule | null>(null);
  const del = useMutation({
    mutationFn: (id: string) => api(`/admin/tax-rules/${id}`, { method: "DELETE" }),
    onSuccess: () => { setToDelete(null); qc.invalidateQueries({ queryKey: ["admin", "tax-rules"] }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("admin.tax.title")} description={t("admin.tax.subtitle")} />
      <SettingsCard />
      <BrandingCard />
      <EInvoiceCard />
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-base font-semibold">{t("admin.tax.rules")}</h2>
          <Button size="sm" onClick={() => { setEditing(null); setOpen(true); }}><PlusIcon aria-hidden />{t("admin.tax.add_rule")}</Button>
        </div>
        <ListCard<TaxRule>
          rows={data?.items} rowKey={(r) => r.id} loading={isLoading} emptyIcon={ReceiptIcon} emptyTitle={t("admin.tax.none")}
          columns={[
            { header: t("admin.tax.name"), primary: true, cell: (r) => <span className="flex flex-wrap items-center gap-1.5">{r.name}{r.compound && <Badge variant="outline">{t("admin.tax.compound")}</Badge>}{!r.enabled && <Badge variant="secondary">{t("admin.tax.disabled")}</Badge>}</span> },
            { header: t("admin.tax.country"), cell: (r) => r.country || t("admin.tax.all") },
            { header: t("admin.tax.region"), cell: (r) => r.region || "-" },
            { header: t("admin.tax.rate"), cell: (r) => <span className="tabular-nums">{formatNumber(r.rate, lang)} %</span> },
            { header: t("admin.tax.priority"), cell: (r) => <span className="tabular-nums">{r.priority}</span> },
            { header: "", cell: (r) => (
              <div className="flex gap-1">
                <Button size="icon" variant="ghost" aria-label={t("admin.tax.edit_rule")} onClick={() => { setEditing(r); setOpen(true); }}><PencilSimpleIcon aria-hidden /></Button>
                <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => setToDelete(r)}><TrashIcon aria-hidden /></Button>
              </div>
            ) },
          ]}
        />
      </div>
      <RuleDialog rule={editing} open={open} onOpenChange={setOpen} />
      <Dialog open={!!toDelete} onOpenChange={(v) => !v && setToDelete(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("admin.tax.delete_confirm")}</DialogTitle></DialogHeader>
          <p className="text-sm text-muted-foreground">{toDelete?.name}</p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setToDelete(null)}>{t("common.cancel")}</Button>
            <Button variant="destructive" disabled={del.isPending} onClick={() => toDelete && del.mutate(toDelete.id)}>{t("common.delete")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
