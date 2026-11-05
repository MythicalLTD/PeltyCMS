import { useState } from "react";
import { ArrowsClockwiseIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TldField } from "@/lib/store/domains";

export interface MarginRule { tld: string; percent: string; fixed: string; min_margin: string; rounding: "cent" | "whole" | "ninety_nine" }
interface SyncLine { tld: string; currency: string; action: string; old_register: string | null; new_register: string | null; old_renew: string | null; new_renew: string | null; old_transfer: string | null; new_transfer: string | null }
interface SyncResult { dry_run: boolean; updated: number; invalid: number; lines: SyncLine[] }
interface Premium { enabled: boolean; markup_percent: string; max_price: string }

const ROUNDINGS = ["cent", "whole", "ninety_nine"] as const;

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => (e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

export function PriceSyncCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const rules = useQuery({ queryKey: ["admin", "tld-margins"], queryFn: () => api<MarginRule[]>("/admin/domains/margins") });
  const regs = useQuery({ queryKey: ["admin", "registrars"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/registrars") });
  const [rows, setRows] = useState<MarginRule[]>([]);
  const [registrar, setRegistrar] = useState("");
  const [result, setResult] = useState<SyncResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seenRules, setSeenRules] = useState(rules.data);
  if (rules.data !== seenRules) {
    setSeenRules(rules.data);
    if (rules.data !== undefined) setRows(rules.data);
  }
  const save = useMutation({
    mutationFn: () => api<MarginRule[]>("/admin/domains/margins", { method: "PUT", body: rows }),
    onSuccess: (r) => { setRows(r); setError(null); toast.success(t("common.saved")); qc.invalidateQueries({ queryKey: ["admin", "tld-margins"] }); },
    onError: (e) => setError(err(e)),
  });
  const sync = useMutation({
    mutationFn: (dry: boolean) => api<SyncResult>("/admin/domains/sync-prices", { method: "POST", body: { registrar, dry_run: dry } }),
    onSuccess: (r) => { setResult(r); setError(null); if (!r.dry_run) { toast.success(t("domains.tld_updated_n", { n: r.updated })); qc.invalidateQueries({ queryKey: ["admin", "tlds"] }); } },
    onError: (e) => setError(err(e)),
  });
  const set = (i: number, patch: Partial<MarginRule>) => setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <div className="grid gap-4">
      <Card>
        <CardHeader><CardTitle>{t("domains.margins_title")}</CardTitle><CardDescription>{t("domains.margins_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-3">
          <FormError message={error} />
          {rows.map((r, i) => (
            <div key={i} className="grid grid-cols-2 items-end gap-2 sm:grid-cols-6">
              <div className="grid gap-1"><Label htmlFor={`m-tld-${i}`}>{t("domains.tld")}</Label><Input id={`m-tld-${i}`} value={r.tld} maxLength={63} onChange={(e) => set(i, { tld: e.target.value.toLowerCase() })} /></div>
              <div className="grid gap-1"><Label htmlFor={`m-p-${i}`}>{t("domains.margin_percent")}</Label><Input id={`m-p-${i}`} inputMode="decimal" value={r.percent} onChange={(e) => set(i, { percent: e.target.value })} /></div>
              <div className="grid gap-1"><Label htmlFor={`m-f-${i}`}>{t("domains.margin_fixed")}</Label><Input id={`m-f-${i}`} inputMode="decimal" value={r.fixed} onChange={(e) => set(i, { fixed: e.target.value })} /></div>
              <div className="grid gap-1"><Label htmlFor={`m-m-${i}`}>{t("domains.margin_min")}</Label><Input id={`m-m-${i}`} inputMode="decimal" value={r.min_margin} onChange={(e) => set(i, { min_margin: e.target.value })} /></div>
              <div className="grid gap-1"><Label>{t("domains.rounding")}</Label>
                <Select value={r.rounding} onValueChange={(v) => set(i, { rounding: v as MarginRule["rounding"] })}>
                  <SelectTrigger className="w-full" aria-label={t("domains.rounding")}><SelectValue /></SelectTrigger>
                  <SelectContent>{ROUNDINGS.map((x) => <SelectItem key={x} value={x}>{t(`domains.rounding_${x}`)}</SelectItem>)}</SelectContent>
                </Select></div>
              <Button variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => setRows(rows.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
            </div>
          ))}
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" onClick={() => setRows([...rows, { tld: rows.some((r) => r.tld === "*") ? "" : "*", percent: "20", fixed: "0", min_margin: "0", rounding: "cent" }])}><PlusIcon aria-hidden />{t("common.add")}</Button>
            <Button onClick={() => save.mutate()} disabled={save.isPending}>{t("common.save")}</Button>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("domains.sync_title")}</CardTitle><CardDescription>{t("domains.sync_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-3">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
            <div className="grid gap-1 sm:w-64"><Label>{t("domains.registrar")}</Label>
              <Select value={registrar} onValueChange={setRegistrar}>
                <SelectTrigger className="w-full" aria-label={t("domains.registrar")}><SelectValue /></SelectTrigger>
                <SelectContent>{(regs.data?.items ?? []).map((r) => <SelectItem key={r.id} value={r.id}>{r.name}</SelectItem>)}</SelectContent>
              </Select></div>
            <Button variant="outline" disabled={!registrar || sync.isPending} onClick={() => sync.mutate(true)}>{t("domains.sync_preview")}</Button>
            <Button disabled={!registrar || sync.isPending} onClick={() => sync.mutate(false)}><ArrowsClockwiseIcon aria-hidden />{t("domains.sync_apply")}</Button>
          </div>
          {result && (
            <>
              <p className="text-sm text-muted-foreground">{t(result.dry_run ? "domains.sync_preview_summary" : "domains.sync_summary", { n: result.updated, invalid: result.invalid })}</p>
              <ResponsiveTable
                rows={result.lines}
                rowKey={(l) => `${l.tld}-${l.currency}`}
                columns={[
                  { header: t("domains.tld"), cell: (l) => <span className="font-mono">.{l.tld}</span> },
                  { header: t("domains.currency_col"), cell: (l) => l.currency },
                  { header: t("domains.field.register_price"), cell: (l) => `${l.old_register ?? "-"} → ${l.new_register ?? "-"}` },
                  { header: t("domains.field.renew_price"), cell: (l) => `${l.old_renew ?? "-"} → ${l.new_renew ?? "-"}` },
                  { header: t("domains.field.transfer_price"), cell: (l) => `${l.old_transfer ?? "-"} → ${l.new_transfer ?? "-"}` },
                  { header: t("domains.sync_status"), cell: (l) => <Badge variant={l.action === "updated" ? "default" : "secondary"}>{tCode(`domains.sync_action_${l.action}`)}</Badge> },
                ]}
              />
            </>
          )}
        </CardContent>
      </Card>
      <PremiumCard />
    </div>
  );
}

function PremiumCard() {
  const { t } = useI18n();
  const err = useErr();
  const q = useQuery({ queryKey: ["admin", "premium-settings"], queryFn: () => api<Premium>("/admin/domains/premium-settings") });
  const [v, setV] = useState<Premium | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seenPremium, setSeenPremium] = useState(q.data);
  if (q.data !== seenPremium) {
    setSeenPremium(q.data);
    if (q.data !== undefined) setV(q.data);
  }
  const save = useMutation({
    mutationFn: () => api<Premium>("/admin/domains/premium-settings", { method: "PUT", body: v }),
    onSuccess: () => { setError(null); toast.success(t("common.saved")); },
    onError: (e) => setError(err(e)),
  });
  if (!v) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("domains.premium_title")}</CardTitle><CardDescription>{t("domains.premium_hint")}</CardDescription></CardHeader>
      <CardContent>
        <form className="grid gap-3 sm:max-w-md" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="flex items-center gap-2"><Switch id="pr-on" checked={v.enabled} onCheckedChange={(x) => setV({ ...v, enabled: x })} /><Label htmlFor="pr-on">{t("domains.premium_enabled")}</Label></div>
          <div className="grid gap-1"><Label htmlFor="pr-mk">{t("domains.premium_markup")}</Label><Input id="pr-mk" inputMode="decimal" value={v.markup_percent} onChange={(e) => setV({ ...v, markup_percent: e.target.value })} /></div>
          <div className="grid gap-1"><Label htmlFor="pr-max">{t("domains.premium_max")}</Label><Input id="pr-max" inputMode="decimal" value={v.max_price} onChange={(e) => setV({ ...v, max_price: e.target.value })} /></div>
          <Button type="submit" className="w-fit" disabled={save.isPending}>{t("common.save")}</Button>
        </form>
      </CardContent>
    </Card>
  );
}

const blankField = (tld: string, n: number): TldField => ({ tld, key: "", label: "", kind: "text", options: [], required: false, max_length: 200, sort_order: n });

export function TldFieldsCard() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const q = useQuery({ queryKey: ["admin", "tld-fields"], queryFn: () => api<TldField[]>("/admin/domains/tld-fields") });
  const [tld, setTld] = useState("");
  const [rows, setRows] = useState<TldField[]>([]);
  const [error, setError] = useState<string | null>(null);
  const tlds = [...new Set((q.data ?? []).map((f) => f.tld))];
  const load = (x: string) => { setTld(x); setRows((q.data ?? []).filter((f) => f.tld === x)); setError(null); };
  const save = useMutation({
    mutationFn: () => api<TldField[]>(`/admin/domains/tld-fields/${encodeURIComponent(tld)}`, { method: "PUT", body: rows.map((r, i) => ({ ...r, tld, sort_order: i })) }),
    onSuccess: (r) => { setRows(r); setError(null); toast.success(t("common.saved")); qc.invalidateQueries({ queryKey: ["admin", "tld-fields"] }); },
    onError: (e) => setError(err(e)),
  });
  const set = (i: number, patch: Partial<TldField>) => setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <Card>
      <CardHeader><CardTitle>{t("domains.fields_title")}</CardTitle><CardDescription>{t("domains.fields_hint")}</CardDescription></CardHeader>
      <CardContent className="grid gap-3">
        <FormError message={error} />
        <div className="flex flex-wrap items-end gap-2">
          <div className="grid gap-1"><Label htmlFor="tf-tld">{t("domains.tld")}</Label><Input id="tf-tld" className="w-40" value={tld} onChange={(e) => load(e.target.value.toLowerCase().replace(/^\./, ""))} /></div>
          {tlds.map((x) => <Button key={x} variant={x === tld ? "default" : "outline"} size="sm" onClick={() => load(x)}>.{x}</Button>)}
        </div>
        {tld && rows.map((r, i) => (
          <div key={i} className="grid grid-cols-2 items-end gap-2 rounded-md border p-3 sm:grid-cols-6">
            <div className="grid gap-1"><Label htmlFor={`tf-k-${i}`}>{t("domains.field_key")}</Label><Input id={`tf-k-${i}`} value={r.key} maxLength={64} onChange={(e) => set(i, { key: e.target.value })} /></div>
            <div className="grid gap-1"><Label htmlFor={`tf-l-${i}`}>{t("domains.field_label")}</Label><Input id={`tf-l-${i}`} value={r.label} maxLength={120} onChange={(e) => set(i, { label: e.target.value })} /></div>
            <div className="grid gap-1"><Label>{t("domains.field_kind")}</Label>
              <Select value={r.kind} onValueChange={(v) => set(i, { kind: v as TldField["kind"] })}>
                <SelectTrigger className="w-full" aria-label={t("domains.field_kind")}><SelectValue /></SelectTrigger>
                <SelectContent>{(["text", "select", "checkbox"] as const).map((k) => <SelectItem key={k} value={k}>{t(`domains.field_kind_${k}`)}</SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-1"><Label htmlFor={`tf-o-${i}`}>{t("domains.field_options")}</Label><Input id={`tf-o-${i}`} disabled={r.kind !== "select"} value={r.options.join(", ")} onChange={(e) => set(i, { options: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} /></div>
            <div className="flex items-center gap-2 pb-2"><Switch id={`tf-r-${i}`} checked={r.required} onCheckedChange={(v) => set(i, { required: v })} /><Label htmlFor={`tf-r-${i}`}>{t("domains.field_required")}</Label></div>
            <Button variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => setRows(rows.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
          </div>
        ))}
        {tld && (
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" onClick={() => setRows([...rows, blankField(tld, rows.length)])}><PlusIcon aria-hidden />{t("common.add")}</Button>
            <Button onClick={() => save.mutate()} disabled={save.isPending}>{t("common.save")}</Button>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
