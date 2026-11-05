import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, CurrencyCircleDollarIcon, NotePencilIcon, PlusIcon, StarIcon } from "@phosphor-icons/react";
import { FormError } from "@/components/Field";
import { ListCard } from "@/components/ListPage";
import { PageHeader } from "@/components/PageHeader";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { formatDateTime } from "@/lib/format";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface Currency { code: string; exponent: number; enabled: boolean; rate: string; is_default: boolean; prefix: string; suffix: string; decimal_sep?: string; thousands_sep?: string; auto_update?: boolean; updated_at: string }
export interface CurrencyDraft { code: string; exponent: string; enabled: boolean; rate: string; prefix: string; suffix: string; decimal_sep?: string; thousands_sep?: string; auto_update?: boolean }

/** Validate + build the payload (null = invalid). */
export function currencyPayload(d: CurrencyDraft, create: boolean) {
  const code = d.code.trim().toUpperCase();
  if (create && !/^[A-Z]{3}$/.test(code)) return null;
  if (!/^[0-4]$/.test(d.exponent.trim())) return null;
  if (!/^\d+(\.\d{1,10})?$/.test(d.rate.trim()) || Number(d.rate) <= 0) return null;
  const decimal_sep = d.decimal_sep ?? ".";
  const thousands_sep = d.thousands_sep ?? ",";
  if ([...decimal_sep].length !== 1 || [...thousands_sep].length > 1 || decimal_sep === thousands_sep) return null;
  return { ...(create ? { code } : {}), exponent: Number(d.exponent), enabled: d.enabled, rate: d.rate.trim(), prefix: d.prefix, suffix: d.suffix, decimal_sep, thousands_sep, auto_update: d.auto_update ?? true };
}

/** Rate markup must be a decimal between -50 and 100 percent. */
export function markupValid(v: string) {
  return /^-?\d+(\.\d{1,4})?$/.test(v.trim()) && Number(v) >= -50 && Number(v) <= 100;
}

/** Example conversion shown next to the rate: 100 default units in this currency. */
export function example(rate: string, exponent: number) {
  const n = Number(rate) * 100;
  return Number.isFinite(n) ? n.toFixed(exponent) : "-";
}

const errMsg = (e: unknown, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null);

function CurrencyDialog({ c }: { c?: Currency }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const init: CurrencyDraft = c ? { code: c.code, exponent: String(c.exponent), enabled: c.enabled, rate: String(Number(c.rate)), prefix: c.prefix, suffix: c.suffix, decimal_sep: c.decimal_sep ?? ".", thousands_sep: c.thousands_sep ?? ",", auto_update: c.auto_update ?? true } : { code: "", exponent: "2", enabled: true, rate: "1", prefix: "", suffix: "", decimal_sep: ".", thousands_sep: ",", auto_update: true };
  const [d, setD] = useState<CurrencyDraft>(init);
  const body = currencyPayload(d, !c);
  const m = useMutation({
    mutationFn: () => api(c ? `/admin/currencies/${c.code}` : "/admin/currencies", { method: c ? "PUT" : "POST", body }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "currencies"] }); void qc.invalidateQueries({ queryKey: ["currencies"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setD(init); m.reset(); } }}>
      <DialogTrigger asChild>{c ? <Button size="icon" variant="ghost" aria-label={t("common.edit")}><NotePencilIcon aria-hidden /></Button> : <Button><PlusIcon aria-hidden />{t("currency.add")}</Button>}</DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{c ? c.code : t("currency.add")}</DialogTitle>{c?.is_default && <DialogDescription>{t("currency.default_locked")}</DialogDescription>}</DialogHeader>
        <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
          {!c && <div className="grid gap-1.5"><Label htmlFor="cur-code">{t("currency.code")}</Label><Input id="cur-code" maxLength={3} value={d.code} onChange={(e) => setD({ ...d, code: e.target.value })} /></div>}
          <div className="grid gap-1.5"><Label htmlFor="cur-rate">{t("currency.rate")}</Label><Input id="cur-rate" inputMode="decimal" disabled={c?.is_default} value={d.rate} onChange={(e) => setD({ ...d, rate: e.target.value })} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cur-exp">{t("currency.decimals")}</Label><Input id="cur-exp" inputMode="numeric" value={d.exponent} onChange={(e) => setD({ ...d, exponent: e.target.value })} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cur-pre">{t("currency.prefix")}</Label><Input id="cur-pre" maxLength={8} value={d.prefix} onChange={(e) => setD({ ...d, prefix: e.target.value })} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cur-suf">{t("currency.suffix")}</Label><Input id="cur-suf" maxLength={8} value={d.suffix} onChange={(e) => setD({ ...d, suffix: e.target.value })} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cur-dsep">{t("currency.decimal_sep")}</Label><Input id="cur-dsep" maxLength={1} value={d.decimal_sep ?? "."} onChange={(e) => setD({ ...d, decimal_sep: e.target.value })} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cur-tsep">{t("currency.thousands_sep")}</Label><Input id="cur-tsep" maxLength={1} value={d.thousands_sep ?? ","} onChange={(e) => setD({ ...d, thousands_sep: e.target.value })} /></div>
          <Label className="flex items-center gap-2 sm:col-span-2"><Switch checked={d.auto_update ?? true} disabled={c?.is_default} onCheckedChange={(v) => setD({ ...d, auto_update: v })} />{t("currency.auto_update")}</Label>
          <Label className="flex items-center gap-2 sm:col-span-2"><Switch checked={d.enabled} disabled={c?.is_default} onCheckedChange={(v) => setD({ ...d, enabled: v })} />{t("currency.enabled")}</Label>
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={!body || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface RateSettings { enabled: boolean; source: string; url: string; markup_percent: string; update_prices: boolean; last_run_at: string | null; last_error: string | null }
interface RateHistory { code: string; old_rate: string; new_rate: string; source: string; created_at: string }

function RatesCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "currency-rates"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<RateSettings>("/admin/currencies/rates/settings") });
  const hist = useQuery({ queryKey: [...key, "history"], queryFn: () => api<{ items: RateHistory[] }>("/admin/currencies/rates/history") });
  const [d, setD] = useState<RateSettings | null>(null);
  const cur = d ?? data;
  const [result, setResult] = useState<string | null>(null);
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin"] }); };
  const save = useMutation({
    mutationFn: () => api("/admin/currencies/rates/settings", { method: "PUT", body: { enabled: cur!.enabled, source: cur!.source, url: cur!.url.trim(), markup_percent: cur!.markup_percent.trim(), update_prices: cur!.update_prices } }),
    onSuccess: () => { setD(null); refresh(); },
  });
  const run = useMutation({
    mutationFn: () => api<{ updated: number; missing: string[]; prices_synced: number | null }>("/admin/currencies/rates/update", { method: "POST" }),
    onSuccess: (r) => { setResult(t("currency.rates_updated", { n: String(r.updated), missing: r.missing.join(", ") || "-" })); refresh(); },
    onError: () => refresh(),
  });
  if (!cur) return null;
  const ok = markupValid(cur.markup_percent) && /^https:\/\/\S+$/.test(cur.url.trim());
  return (
    <Card>
      <CardHeader><CardTitle>{t("currency.rates_title")}</CardTitle><CardDescription>{t("currency.rates_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="grid gap-3 *:min-w-0 md:grid-cols-2">
          <div className="grid gap-1.5"><Label htmlFor="rate-src">{t("currency.rate_source")}</Label>
            <Select value={cur.source} onValueChange={(v) => setD({ ...cur, source: v })}>
              <SelectTrigger id="rate-src" className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="ecb">{t("currency.source_ecb")}</SelectItem><SelectItem value="json">{t("currency.source_json")}</SelectItem></SelectContent>
            </Select>
          </div>
          <div className="grid gap-1.5"><Label htmlFor="rate-markup">{t("currency.markup")}</Label><Input id="rate-markup" inputMode="decimal" value={cur.markup_percent} onChange={(e) => setD({ ...cur, markup_percent: e.target.value })} /></div>
          <div className="grid gap-1.5 md:col-span-2"><Label htmlFor="rate-url">{t("currency.rate_url")}</Label><Input id="rate-url" type="url" value={cur.url} onChange={(e) => setD({ ...cur, url: e.target.value })} /></div>
          <Label className="flex items-center gap-2"><Switch checked={cur.enabled} onCheckedChange={(v) => setD({ ...cur, enabled: v })} />{t("currency.rates_auto")}</Label>
          <Label className="flex items-center gap-2"><Switch checked={cur.update_prices} onCheckedChange={(v) => setD({ ...cur, update_prices: v })} />{t("currency.rates_prices")}</Label>
        </div>
        <p className="text-xs text-muted-foreground">{t("currency.last_run")}: {cur.last_run_at ? formatDateTime(cur.last_run_at, lang) : "-"}{cur.last_error ? ` · ${cur.last_error}` : ""}</p>
        {result && <Alert><CurrencyCircleDollarIcon aria-hidden /><AlertDescription>{result}</AlertDescription></Alert>}
        <FormError message={errMsg(save.error ?? run.error, tCode)} />
        <div className="flex flex-wrap gap-2">
          <Button disabled={!d || !ok || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button>
          <Button variant="outline" disabled={run.isPending || !!d} onClick={() => run.mutate()}><ArrowsClockwiseIcon aria-hidden />{t("currency.rates_now")}</Button>
        </div>
        {!!hist.data?.items.length && (
          <ResponsiveTable<RateHistory>
            rows={hist.data.items.slice(0, 20)} rowKey={(h) => h.code + h.created_at}
            columns={[
              { header: t("currency.code"), primary: true, cell: (h) => h.code },
              { header: t("currency.rate"), cell: (h) => <span className="tabular-nums">{Number(h.old_rate)} → {Number(h.new_rate)}</span> },
              { header: t("currency.rate_source"), cell: (h) => h.source },
              { header: t("currency.when"), cell: (h) => formatDateTime(h.created_at, lang) },
            ]}
          />
        )}
      </CardContent>
    </Card>
  );
}

export function AdminCurrenciesPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [synced, setSynced] = useState<number | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "currencies"], queryFn: () => api<{ items: Currency[] }>("/admin/currencies") });
  const inv = () => { void qc.invalidateQueries({ queryKey: ["admin", "currencies"] }); void qc.invalidateQueries({ queryKey: ["admin", "products"] }); };
  const def = useMutation({ mutationFn: (code: string) => api(`/admin/currencies/${code}/default`, { method: "POST" }), onSuccess: inv });
  const sync = useMutation({ mutationFn: (only: boolean) => api<{ updated: number }>("/admin/currencies/sync-prices", { method: "POST", body: { only_missing: only } }), onSuccess: (r) => { setSynced(r.updated); inv(); } });
  const base = data?.items.find((c) => c.is_default)?.code ?? "";
  const [all, setAll] = useState(false);
  const rows = data?.items.filter((c) => all || c.enabled || c.is_default);
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("currency.title")} description={t("currency.subtitle")} actions={<CurrencyDialog />} />
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={sync.isPending} onClick={() => sync.mutate(true)}><ArrowsClockwiseIcon aria-hidden />{t("currency.sync_missing")}</Button>
        <Button variant="outline" disabled={sync.isPending} onClick={() => sync.mutate(false)}><ArrowsClockwiseIcon aria-hidden />{t("currency.sync_all")}</Button>
      </div>
      <Label className="flex items-center gap-2"><Switch checked={all} onCheckedChange={setAll} />{t("currency.show_all")}</Label>
      {synced !== null && <Alert><CurrencyCircleDollarIcon aria-hidden /><AlertDescription>{t("currency.synced", { n: String(synced) })}</AlertDescription></Alert>}
      <FormError message={errMsg(def.error ?? sync.error, tCode)} />
      <ListCard<Currency>
        rows={rows} rowKey={(c) => c.code} loading={isLoading}
        columns={[
          { header: t("currency.code"), primary: true, cell: (c) => <span className="flex items-center gap-2 font-medium">{c.code}{c.is_default && <Badge>{t("currency.default")}</Badge>}{!c.enabled && <Badge variant="outline">{t("currency.disabled")}</Badge>}</span> },
          { header: t("currency.rate"), cell: (c) => <span className="tabular-nums">{Number(c.rate)} <span className="text-xs text-muted-foreground">(100 {base} = {c.prefix}{example(c.rate, c.exponent)}{c.suffix || ` ${c.code}`})</span></span> },
          { header: t("currency.decimals"), cell: (c) => c.exponent },
          { header: "", cell: (c) => (
            <div className="flex justify-end gap-1">
              {!c.is_default && c.enabled && <Button size="sm" variant="outline" disabled={def.isPending} onClick={() => def.mutate(c.code)}><StarIcon aria-hidden />{t("currency.make_default")}</Button>}
              <CurrencyDialog c={c} />
            </div>
          ) },
        ]}
      />
      <RatesCard />
    </div>
  );
}
