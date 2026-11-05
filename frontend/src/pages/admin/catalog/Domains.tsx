import { useEffect, useState } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useParams } from "react-router";
import { ArrowClockwiseIcon, ArrowLeftIcon, GlobeIcon, PlayIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { MassBar, selectColumn, useSelection } from "@/components/admin/MassBar";
import { FormError } from "@/components/Field";
import { AdminRegistrantCard, RegistrantSettingsCard } from "@/components/domains/RegistrantVerification";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { DnsEditor } from "@/components/domains/DnsEditor";
import { AdminSslList } from "@/components/domains/SslStatus";
import { Textarea } from "@/components/ui/textarea";
import { DOMAIN_STATUSES, TLD_DEFAULTS, TLD_GROUPS, type Domain, type DomainDetail, type Tld } from "@/lib/store/domains";
import { DomainStatus } from "@/pages/client/services/Domains";
import { PriceSyncCard, TldFieldsCard } from "@/pages/admin/catalog/DomainPricingExtras";

const EMPTY: Tld = { tld: "", currency: "EUR", register_price: "0.00", renew_price: "0.00", transfer_price: "0.00", min_years: 1, max_years: 10, registrar: "mock", active: true, ...TLD_DEFAULTS };

function TldDialog({ initial, trigger }: { initial?: Tld; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<Tld>(initial ?? EMPTY);
  const [error, setError] = useState<string | null>(null);
  const { data: regs } = useQuery({ queryKey: ["admin", "registrars"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/registrars"), enabled: open });
  const save = useMutation({
    mutationFn: () => api<Tld>("/admin/tlds", { method: "PUT", body: d }),
    onSuccess: () => { setOpen(false); qc.invalidateQueries({ queryKey: ["admin", "tlds"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const num = (k: keyof Tld) => (
    <div className="grid gap-2"><Label htmlFor={`tld-${k}`}>{tCode(`domains.field.${k}`)}</Label>
      <Input id={`tld-${k}`} inputMode="decimal" required value={String(d[k])}
        onChange={(e) => setD({ ...d, [k]: typeof d[k] === "number" ? Number(e.target.value) : e.target.value })} /></div>
  );
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setD(initial ?? EMPTY); setError(null); } }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{initial ? `.${initial.tld}` : t("domains.add_tld")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label htmlFor="tld-tld">{t("domains.tld")}</Label>
              <Input id="tld-tld" required disabled={!!initial} value={d.tld} onChange={(e) => setD({ ...d, tld: e.target.value })} /></div>
            <div className="grid gap-2"><Label htmlFor="tld-cur">{t("domains.field.currency")}</Label>
              <Input id="tld-cur" required maxLength={3} disabled={!!initial} value={d.currency} onChange={(e) => setD({ ...d, currency: e.target.value.toUpperCase() })} /></div>
            {num("register_price")}{num("renew_price")}{num("transfer_price")}
            {num("min_years")}{num("max_years")}
            {num("id_protection_price")}{num("dns_management_price")}
            {num("grace_days")}{num("grace_fee")}
            {num("redemption_days")}{num("redemption_fee")}
            {num("sort_order")}
            <div className="grid gap-2"><Label>{t("domains.field.tld_group")}</Label>
              <Select value={d.tld_group || "none"} onValueChange={(v) => setD({ ...d, tld_group: (v === "none" ? "" : v) as Tld["tld_group"] })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{TLD_GROUPS.map((g) => <SelectItem key={g || "none"} value={g || "none"}>{tCode(`domains.group.${g || "none"}`)}</SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label>{t("domains.registrar")}</Label>
              <Select value={d.registrar} onValueChange={(v) => setD({ ...d, registrar: v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{(regs?.items ?? [{ id: d.registrar, name: d.registrar }]).map((r) => <SelectItem key={r.id} value={r.id}>{r.name}</SelectItem>)}</SelectContent>
              </Select></div>
          </div>
          <div className="flex items-center gap-2"><Switch id="tld-act" checked={d.active} onCheckedChange={(v) => setD({ ...d, active: v })} /><Label htmlFor="tld-act">{t("domains.field.active")}</Label></div>
          <div className="flex items-center gap-2"><Switch id="tld-epp" checked={d.epp_required} onCheckedChange={(v) => setD({ ...d, epp_required: v })} /><Label htmlFor="tld-epp">{t("domains.field.epp_required")}</Label></div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function TldTools() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState<"adjust" | "copy" | null>(null);
  const [percent, setPercent] = useState("10");
  const [currency, setCurrency] = useState("");
  const [from, setFrom] = useState("EUR");
  const [to, setTo] = useState("USD");
  const [rate, setRate] = useState("1");
  const [overwrite, setOverwrite] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useMutation({
    mutationFn: () => open === "adjust"
      ? api<{ updated: number }>("/admin/tlds/bulk-adjust", { method: "POST", body: { percent, currency: currency.trim() || null } })
      : api<{ updated: number }>("/admin/tlds/copy-currency", { method: "POST", body: { from, to, rate, overwrite } }),
    onSuccess: (r) => { setMsg(t("domains.tld_updated_n", { n: r.updated })); setError(null); qc.invalidateQueries({ queryKey: ["admin", "tlds"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const field = (id: string, label: string, v: string, set: (x: string) => void, upper = false) => (
    <div className="grid gap-2"><Label htmlFor={id}>{label}</Label><Input id={id} value={v} inputMode={upper ? undefined : "decimal"} maxLength={upper ? 3 : 20} onChange={(e) => set(upper ? e.target.value.toUpperCase() : e.target.value)} /></div>
  );
  return (
    <>
      <Button variant="outline" onClick={() => { setOpen("adjust"); setMsg(null); setError(null); }}>{t("domains.bulk_adjust")}</Button>
      <Button variant="outline" onClick={() => { setOpen("copy"); setMsg(null); setError(null); }}>{t("domains.copy_currency")}</Button>
      <Dialog open={open !== null} onOpenChange={(o) => !o && setOpen(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader><DialogTitle>{open === "adjust" ? t("domains.bulk_adjust") : t("domains.copy_currency")}</DialogTitle></DialogHeader>
          <form className="grid gap-3" onSubmit={(e) => { e.preventDefault(); run.mutate(); }}>
            <FormError message={error} />
            {msg && <p className="text-sm text-muted-foreground">{msg}</p>}
            {open === "adjust" ? (<>
              {field("ba-pct", t("domains.percent"), percent, setPercent)}
              {field("ba-cur", t("domains.currency_optional"), currency, setCurrency, true)}
            </>) : (<>
              <div className="grid grid-cols-2 gap-3">{field("cc-from", t("domains.from_currency"), from, setFrom, true)}{field("cc-to", t("domains.to_currency"), to, setTo, true)}</div>
              {field("cc-rate", t("domains.rate"), rate, setRate)}
              <div className="flex items-center gap-2"><Switch id="cc-ow" checked={overwrite} onCheckedChange={setOverwrite} /><Label htmlFor="cc-ow">{t("domains.overwrite")}</Label></div>
            </>)}
            <DialogFooter><Button type="submit" disabled={run.isPending}>{t("domains.apply")}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}

function TldPricing() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "tlds"], queryFn: () => api<{ items: Tld[] }>("/admin/tlds") });
  const del = useMutation({ mutationFn: (x: Tld) => api(`/admin/tlds/${encodeURIComponent(x.tld)}/${x.currency}`, { method: "DELETE" }), onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "tlds"] }) });
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2"><TldDialog trigger={<Button><PlusIcon aria-hidden />{t("domains.add_tld")}</Button>} /><TldTools /></div>
      <ListCard<Tld> rows={data?.items} rowKey={(x) => `${x.tld}-${x.currency}`} loading={isLoading} emptyIcon={GlobeIcon} emptyTitle={t("domains.no_tlds")}
        columns={[
          { header: t("domains.tld"), primary: true, cell: (x) => <TldDialog initial={x} trigger={<Button variant="link" className="h-auto p-0">.{x.tld} · {x.currency}</Button>} /> },
          { header: t("domains.kind.register"), cell: (x) => formatMoney(x.register_price, x.currency, lang) },
          { header: t("domains.renew"), cell: (x) => formatMoney(x.renew_price, x.currency, lang) },
          { header: t("domains.kind.transfer"), cell: (x) => formatMoney(x.transfer_price, x.currency, lang) },
          { header: t("domains.field.redemption_fee"), cell: (x) => formatMoney(x.redemption_fee, x.currency, lang) },
          { header: t("domains.registrar"), cell: (x) => `${x.registrar}${x.active ? "" : ` (${t("domains.inactive")})`}${x.tld_group ? ` · ${t(`domains.group.${x.tld_group}`)}` : ""}` },
          { header: "", cell: (x) => (
            <ConfirmAction description={t("domains.delete_tld_confirm")} onConfirm={() => del.mutate(x)}>
              <Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
            </ConfirmAction>) },
        ]} />
    </div>
  );
}

function DomainList() {
  const { t, tCode, lang } = useI18n();
  const sel = useSelection();
  const [status, setStatus] = useState("all");
  const [q, setQ] = useState("");
  const params = new URLSearchParams({ per_page: "100" });
  if (status !== "all") params.set("status", status);
  if (q.trim()) params.set("q", q.trim());
  const { data, isLoading } = useQuery({ queryKey: ["admin", "domains", status, q], queryFn: () => api<{ items: Domain[] }>(`/admin/domains?${params}`) });
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2 sm:flex-row">
        <Input aria-label={t("domains.search")} placeholder={t("domains.search")} value={q} onChange={(e) => setQ(e.target.value)} />
        <Select value={status} onValueChange={setStatus}>
          <SelectTrigger className="w-full sm:w-48"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("domains.all_statuses")}</SelectItem>
            {DOMAIN_STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`domains.status.${s}`)}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      <MassBar sel={sel} endpoint="/admin/mass/domains" invalidate={["admin", "domains"]} actions={[
        { key: "auto_renew_on", label: t("mass.auto_renew_on") },
        { key: "auto_renew_off", label: t("mass.auto_renew_off") },
      ]} />
      <ListCard<Domain> rows={data?.items} rowKey={(x) => x.id} loading={isLoading} emptyIcon={GlobeIcon} emptyTitle={t("domains.empty")}
        columns={[
          selectColumn<Domain>(sel, (x) => x.name),
          { header: t("domains.name"), primary: true, cell: (x) => <Link className="font-medium break-all hover:underline" to={`/admin/domains/${x.id}`}>{x.name}</Link> },
          { header: t("billing.status"), cell: (x) => <DomainStatus status={x.status} /> },
          { header: t("domains.order_kind"), cell: (x) => tCode(`domains.kind.${x.kind}`) },
          { header: t("domains.expires"), cell: (x) => formatDate(x.expires_on, lang) },
        ]} />
    </div>
  );
}

type DnsRec = { rtype: string; host: string; value: string; ttl: number; priority?: number | null };
type Defaults = { nameservers: string[]; dns_template: DnsRec[] };
const DNS_TYPES = ["A", "AAAA", "CNAME", "MX", "TXT", "SRV", "CAA", "NS", "URL", "FRAME"];

function DomainDefaultsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "domain-defaults"], queryFn: () => api<Defaults>("/admin/settings/domain-defaults") });
  const [ns, setNs] = useState("");
  const [recs, setRecs] = useState<DnsRec[]>([]);
  useEffect(() => { if (q.data) { setNs(q.data.nameservers.join("\n")); setRecs(q.data.dns_template); } }, [q.data]);
  const save = useMutation({
    mutationFn: () => api<Defaults>("/admin/settings/domain-defaults", { method: "PUT", body: { nameservers: ns.split(/\s+/).filter(Boolean), dns_template: recs } }),
    onSuccess: (d) => { qc.setQueryData(["admin", "domain-defaults"], d); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const upd = (i: number, p: Partial<DnsRec>) => setRecs(recs.map((r, j) => (j === i ? { ...r, ...p } : r)));
  return (
    <Card>
      <CardHeader><CardTitle>{t("domains.defaults")}</CardTitle><CardDescription>{t("domains.defaults_hint")}</CardDescription></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="dd-ns">{t("domains.default_ns")}</Label>
            <Textarea id="dd-ns" rows={3} className="font-mono" placeholder={"ns1.example.com\nns2.example.com"} value={ns} onChange={(e) => setNs(e.target.value)} />
          </div>
          <div className="flex flex-col gap-2">
            <Label>{t("domains.dns_template")}</Label>
            <p className="text-sm text-muted-foreground">{t("domains.dns_template_hint")}</p>
            {recs.map((r, i) => (
              <div key={i} className="grid gap-2 *:min-w-0 sm:grid-cols-[8rem_1fr_2fr_6rem_auto]">
                <Select value={r.rtype} onValueChange={(v) => upd(i, { rtype: v })}>
                  <SelectTrigger className="w-full" aria-label={t("domains.dns_type")}><SelectValue /></SelectTrigger>
                  <SelectContent>{DNS_TYPES.map((x) => <SelectItem key={x} value={x}>{x}</SelectItem>)}</SelectContent>
                </Select>
                <Input aria-label={t("domains.dns_host")} value={r.host} onChange={(e) => upd(i, { host: e.target.value })} />
                <Input aria-label={t("domains.dns_value")} value={r.value} onChange={(e) => upd(i, { value: e.target.value })} />
                <Input aria-label="TTL" type="number" min={60} value={r.ttl} onChange={(e) => upd(i, { ttl: Number(e.target.value) || 3600 })} />
                <Button type="button" variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => setRecs(recs.filter((_, j) => j !== i))}><TrashIcon /></Button>
              </div>
            ))}
            <div><Button type="button" variant="outline" onClick={() => setRecs([...recs, { rtype: "A", host: "@", value: "", ttl: 3600 }])}><PlusIcon aria-hidden />{t("domains.dns_add")}</Button></div>
          </div>
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

export function AdminDomainsPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const run = useMutation({ mutationFn: () => api("/admin/domains/run", { method: "POST" }), onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "domains"] }) });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("domains.title")} description={t("domains.admin_hint")}
        actions={<Button variant="outline" onClick={() => run.mutate()} disabled={run.isPending}><PlayIcon aria-hidden />{t("domains.run_now")}</Button>} />
      <RegistrantSettingsCard />
      <Tabs defaultValue="domains">
        <TabsList className="w-full justify-start overflow-x-auto sm:w-auto">
          <TabsTrigger value="domains">{t("domains.title")}</TabsTrigger>
          <TabsTrigger value="pricing">{t("domains.pricing")}</TabsTrigger>
          <TabsTrigger value="sync">{t("domains.tab_sync")}</TabsTrigger>
          <TabsTrigger value="fields">{t("domains.tab_fields")}</TabsTrigger>
          <TabsTrigger value="defaults">{t("domains.defaults")}</TabsTrigger>
          <TabsTrigger value="ssl">{t("ssl.title")}</TabsTrigger>
        </TabsList>
        <TabsContent value="ssl"><AdminSslList /></TabsContent>
        <TabsContent value="defaults"><DomainDefaultsCard /></TabsContent>
        <TabsContent value="sync"><PriceSyncCard /></TabsContent>
        <TabsContent value="fields"><TldFieldsCard /></TabsContent>
        <TabsContent value="domains"><DomainList /></TabsContent>
        <TabsContent value="pricing"><TldPricing /></TabsContent>
      </Tabs>
    </div>
  );
}

export function AdminDomainPage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "domain", id];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<DomainDetail>(`/admin/domains/${id}`) });
  const [exp, setExp] = useState<string | null>(null);
  const [notes, setNotes] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const onErr = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const done = (r: DomainDetail) => { qc.setQueryData(key, r); setError(null); setExp(null); setNotes(null); };
  const update = useMutation({ mutationFn: (body: Record<string, unknown>) => api<DomainDetail>(`/admin/domains/${id}`, { method: "PATCH", body }), onSuccess: done, onError: onErr });
  const sync = useMutation({ mutationFn: () => api<DomainDetail>(`/admin/domains/${id}/sync`, { method: "POST" }), onSuccess: done, onError: onErr });
  if (!data) return <PageSkeleton />;
  const d = data.domain;
  return (
    <div className="flex flex-col gap-4">
      <Button asChild variant="ghost" className="w-fit"><Link to="/admin/domains"><ArrowLeftIcon aria-hidden />{t("domains.title")}</Link></Button>
      <PageHeader title={d.name} actions={<Button variant="outline" onClick={() => sync.mutate()} disabled={sync.isPending}><ArrowClockwiseIcon aria-hidden />{t("domains.sync")}</Button>} />
      <FormError message={error} />
      {d.last_error && <p className="text-sm text-destructive break-words">{d.last_error}</p>}
      <AdminRegistrantCard domainId={id} v={data.registrant_verification} onDone={done} />
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader><CardTitle>{t("domains.overview")}</CardTitle></CardHeader>
          <CardContent className="grid gap-3 text-sm">
            <div className="grid gap-2"><Label>{t("billing.status")}</Label>
              <Select value={d.status} onValueChange={(v) => update.mutate({ status: v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{DOMAIN_STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`domains.status.${s}`)}</SelectItem>)}</SelectContent>
              </Select></div>
            <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); if (exp) update.mutate({ expires_on: exp }); }}>
              <Label htmlFor="a-exp">{t("domains.expires")}</Label>
              <div className="flex gap-2"><Input id="a-exp" type="date" value={exp ?? d.expires_on ?? ""} onChange={(e) => setExp(e.target.value)} />
                <Button type="submit" disabled={!exp || update.isPending}>{t("common.save")}</Button></div>
            </form>
            <div className="flex items-center justify-between gap-2"><Label htmlFor="a-ar">{t("domains.auto_renew")}</Label>
              <Switch id="a-ar" checked={d.auto_renew} onCheckedChange={(v) => update.mutate({ auto_renew: v })} /></div>
            <div className="flex items-center justify-between gap-2"><Label htmlFor="a-dns">{t("domains.dns_management")}</Label>
              <Switch id="a-dns" checked={!!d.dns_management} onCheckedChange={(v) => update.mutate({ dns_management: v })} /></div>
            <div className="flex items-center justify-between gap-2"><Label htmlFor="a-priv">{t("domains.privacy")}</Label>
              <Switch id="a-priv" checked={d.privacy} onCheckedChange={(v) => update.mutate({ privacy: v })} /></div>
            <div className="flex items-center justify-between gap-2"><Label htmlFor="a-lock">{t("domains.registrar_lock")}</Label>
              <Switch id="a-lock" checked={!!d.registrar_lock} onCheckedChange={(v) => update.mutate({ registrar_lock: v })} /></div>
            <div className="flex justify-between gap-2"><span className="text-muted-foreground">{t("domains.synced_at")}</span><span>{formatDate(d.synced_at ?? null, lang)}</span></div>
            <div className="flex justify-between gap-2"><span className="text-muted-foreground">{t("domains.registrar")}</span><span>{d.registrar}</span></div>
            <div className="flex justify-between gap-2"><span className="text-muted-foreground">{t("domains.registered")}</span><span>{formatDate(d.registered_on, lang)}</span></div>
            {d.premium && <div className="flex justify-between gap-2"><span className="text-muted-foreground">{t("domains.premium")}</span><span>{d.premium_renew_price ? `${d.premium_renew_price} / ${t("domains.year")}` : "✓"}</span></div>}
            {Object.entries(d.extra_fields ?? {}).length > 0 && <p className="pt-2 font-medium">{t("domains.extra_fields")}</p>}
            {Object.entries(d.extra_fields ?? {}).map(([k, v]) => (
              <div key={k} className="flex justify-between gap-2"><span className="text-muted-foreground font-mono">{k}</span><span className="break-all text-right">{String(v)}</span></div>
            ))}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle>{t("domains.whois")}</CardTitle></CardHeader>
          <CardContent className="grid gap-2 text-sm">
            {Object.entries(d.contact).filter(([, v]) => v).map(([k, v]) => (
              <div key={k} className="flex justify-between gap-2"><span className="text-muted-foreground">{tCode(`domains.contact.${k}`)}</span><span className="break-all text-right">{String(v)}</span></div>
            ))}
            <p className="pt-2 font-medium">{t("domains.nameservers")}</p>
            {d.nameservers.length ? d.nameservers.map((n) => <span key={n} className="break-all">{n}</span>) : <span className="text-muted-foreground">{t("domains.default_ns")}</span>}
          </CardContent>
        </Card>
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader><CardTitle>{t("domains.dns")}</CardTitle></CardHeader>
          <CardContent><DnsEditor base={`/admin/domains/${id}`} /></CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle>{t("domains.admin_notes")}</CardTitle></CardHeader>
          <CardContent>
            <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); if (notes !== null) update.mutate({ admin_notes: notes }); }}>
              <Textarea aria-label={t("domains.admin_notes")} rows={5} value={notes ?? d.admin_notes ?? ""} onChange={(e) => setNotes(e.target.value)} />
              <Button type="submit" className="w-fit" disabled={notes === null || update.isPending}>{t("common.save")}</Button>
            </form>
          </CardContent>
        </Card>
      </div>
      <Card>
        <CardHeader><CardTitle>{t("domains.renewals")}</CardTitle></CardHeader>
        <CardContent>
          <ResponsiveTable rows={data.renewals} rowKey={(x) => x.id} emptyTitle={t("domains.no_renewals")}
            columns={[
              { header: t("domains.expires"), primary: true, cell: (x) => formatDate(x.for_expiry, lang) },
              { header: t("domains.years"), cell: (x) => x.years },
              { header: t("billing.status"), cell: (x) => (x.applied ? t("domains.applied") : t("domains.awaiting_payment")) },
              { header: "", cell: (x) => x.invoice_id && <Link className="hover:underline" to={`/admin/invoices/${x.invoice_id}`}>{t("domains.invoice")}</Link> },
            ]} />
        </CardContent>
      </Card>
    </div>
  );
}
