import { useState } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { ArrowLeftIcon, CopyIcon, GlobeIcon, KeyIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { FormError } from "@/components/Field";
import { SslBadge, useClientSsl } from "@/components/domains/SslStatus";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { DnsEditor } from "@/components/domains/DnsEditor";
import { effectivePricing, missingFields, orderTotal, RENEWABLE, parseNameservers, statusVariant, yearsRange, type CheckResult, type Contact, type Domain, type DomainDetail, type Tld, type TldField } from "@/lib/store/domains";
import { Captcha } from "@/components/Captcha";
import { RegistrantAlert } from "@/components/domains/RegistrantVerification";
import { DomainBulkBar } from "@/components/domains/DomainBulkBar";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";

export function DomainStatus({ status }: { status: string }) {
  const { tCode } = useI18n();
  return <Badge variant={statusVariant(status)}>{tCode(`domains.status.${status}`)}</Badge>;
}

const CONTACT_FIELDS: (keyof Contact)[] = ["name", "organization", "email", "phone", "address", "city", "postal_code", "country"];

export function ContactFields({ value, onChange, idPrefix }: { value: Partial<Contact>; onChange: (c: Partial<Contact>) => void; idPrefix: string }) {
  const { tCode } = useI18n();
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {CONTACT_FIELDS.map((f) => (
        <div key={f} className="grid gap-2">
          <Label htmlFor={`${idPrefix}-${f}`}>{tCode(`domains.contact.${f}`)}</Label>
          <Input id={`${idPrefix}-${f}`} required={f === "name" || f === "email"} type={f === "email" ? "email" : "text"} maxLength={f === "country" ? 2 : 200}
            value={value[f] ?? ""} onChange={(e) => onChange({ ...value, [f]: f === "country" ? e.target.value.toUpperCase() : e.target.value })} />
        </div>
      ))}
    </div>
  );
}

function OrderForm({ check, onDone }: { check: CheckResult & { pricing: Tld }; onDone: (id: string, invoice: string) => void }) {
  const { t, tCode, lang } = useI18n();
  const [kind, setKind] = useState<"register" | "transfer">(check.available ? "register" : "transfer");
  const [years, setYears] = useState(check.pricing.min_years);
  const [ns, setNs] = useState("");
  const [auth, setAuth] = useState("");
  const [privacy, setPrivacy] = useState(false);
  const [dns, setDns] = useState(false);
  const [contact, setContact] = useState<Partial<Contact>>({});
  const [extra, setExtra] = useState<Record<string, string | boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const p = kind === "register" ? effectivePricing(check.pricing, check.premium) : check.pricing;
  const { data: fields = [] } = useQuery({ queryKey: ["domain-fields", check.tld], queryFn: () => api<TldField[]>(`/client/domains/fields?tld=${encodeURIComponent(check.tld)}`) });
  const order = useMutation({
    mutationFn: () => api<{ domain: Domain; invoice_id: string }>("/client/domains", { method: "POST", body: { name: check.name, kind, years, nameservers: parseNameservers(ns), contact, privacy, dns_management: dns, auth_code: auth, extra_fields: extra } }),
    onSuccess: (r) => onDone(r.domain.id, r.invoice_id),
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="break-all">{check.name}</CardTitle>
        <CardDescription>{check.available ? t("domains.available") : t("domains.unavailable_transfer")}{check.premium_error && ` (${tCode(check.premium_error)})`}</CardDescription>
        {check.premium && kind === "register" && <Badge variant="secondary" className="w-fit">{t("domains.premium_badge", { price: formatMoney(check.premium.register_price, p.currency, lang), renew: formatMoney(check.premium.renew_price, p.currency, lang) })}</Badge>}
      </CardHeader>
      <CardContent>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); order.mutate(); }}>
          <FormError message={error} />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("domains.order_kind")}</Label>
              <Select value={kind} onValueChange={(v) => setKind(v as "register" | "transfer")}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  {check.available && <SelectItem value="register">{t("domains.kind.register")}</SelectItem>}
                  <SelectItem value="transfer">{t("domains.kind.transfer")}</SelectItem>
                </SelectContent>
              </Select></div>
            {kind === "register" ? (
              <div className="grid gap-2"><Label>{t("domains.years")}</Label>
                <Select value={String(years)} onValueChange={(v) => setYears(Number(v))}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{yearsRange(p).map((y) => <SelectItem key={y} value={String(y)}>{t("domains.years_n", { n: y })}</SelectItem>)}</SelectContent>
                </Select></div>
            ) : (
              <div className="grid gap-2"><Label htmlFor="o-auth">{t("domains.auth_code")}</Label>
                <Input id="o-auth" required={p.epp_required} maxLength={64} autoComplete="off" value={auth} onChange={(e) => setAuth(e.target.value)} /></div>
            )}
          </div>
          <div className="grid gap-2"><Label htmlFor="o-ns">{t("domains.nameservers")}</Label>
            <Textarea id="o-ns" rows={3} placeholder="ns1.example.com&#10;ns2.example.com" value={ns} onChange={(e) => setNs(e.target.value)} />
            <p className="text-xs text-muted-foreground">{t("domains.nameservers_hint")}</p></div>
          <h3 className="text-sm font-medium">{t("domains.registrant")}</h3>
          <ContactFields idPrefix="o" value={contact} onChange={setContact} />
          {fields.length > 0 && <h3 className="text-sm font-medium">{t("domains.extra_fields")}</h3>}
          {fields.length > 0 && <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            {fields.map((f) => f.kind === "checkbox" ? (
              <div key={f.key} className="flex items-center gap-2"><Switch id={`x-${f.key}`} checked={extra[f.key] === true} onCheckedChange={(v) => setExtra({ ...extra, [f.key]: v })} /><Label htmlFor={`x-${f.key}`}>{f.label}{f.required && " *"}</Label></div>
            ) : f.kind === "select" ? (
              <div key={f.key} className="grid gap-2"><Label>{f.label}{f.required && " *"}</Label>
                <Select value={String(extra[f.key] ?? "")} onValueChange={(v) => setExtra({ ...extra, [f.key]: v })}>
                  <SelectTrigger className="w-full" aria-label={f.label}><SelectValue /></SelectTrigger>
                  <SelectContent>{f.options.map((o) => <SelectItem key={o} value={o}>{o}</SelectItem>)}</SelectContent>
                </Select></div>
            ) : (
              <div key={f.key} className="grid gap-2"><Label htmlFor={`x-${f.key}`}>{f.label}{f.required && " *"}</Label>
                <Input id={`x-${f.key}`} required={f.required} maxLength={f.max_length} value={String(extra[f.key] ?? "")} onChange={(e) => setExtra({ ...extra, [f.key]: e.target.value })} /></div>
            ))}
          </div>}
          <div className="flex items-center gap-2"><Switch id="o-priv" checked={privacy} onCheckedChange={setPrivacy} /><Label htmlFor="o-priv">{t("domains.privacy")}{Number(p.id_protection_price) > 0 && ` (+${formatMoney(p.id_protection_price, p.currency, lang)}/${t("domains.year")})`}</Label></div>
          <div className="flex items-center gap-2"><Switch id="o-dns" checked={dns} onCheckedChange={setDns} /><Label htmlFor="o-dns">{t("domains.dns_management")}{Number(p.dns_management_price) > 0 && ` (+${formatMoney(p.dns_management_price, p.currency, lang)}/${t("domains.year")})`}</Label></div>
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <p className="text-sm">{t("domains.price_total")}: <span className="font-semibold">{formatMoney(orderTotal(p, kind, years, privacy, dns), p.currency, lang)}</span></p>
            <Button type="submit" disabled={order.isPending || missingFields(fields, extra).length > 0}>{t("domains.order")}</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

function DomainSearch() {
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const [params] = useSearchParams();
  const [q, setQ] = useState(params.get("name") ?? "");
  const [res, setRes] = useState<CheckResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { data: tlds } = useQuery({ queryKey: ["client", "tlds"], queryFn: () => api<{ items: Tld[]; currency: string }>("/client/domains/tlds") });
  const check = useMutation({
    mutationFn: () => api<CheckResult>(`/client/domains/check?name=${encodeURIComponent(q.trim())}`, { captcha: true }),
    onSuccess: (r) => { setRes(r); setError(null); },
    onError: (e) => { setRes(null); setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")); },
  });
  return (
    <div className="flex flex-col gap-4">
      <form className="flex flex-col gap-2 sm:flex-row" onSubmit={(e) => { e.preventDefault(); if (q.trim()) check.mutate(); }}>
        <Input aria-label={t("domains.search")} placeholder={t("domains.search_placeholder")} value={q} onChange={(e) => setQ(e.target.value)} />
        <Button type="submit" disabled={check.isPending}><MagnifyingGlassIcon aria-hidden />{t("domains.check")}</Button>
      </form>
      <Captcha form="domain_lookup" signedIn />
      <FormError message={error} />
      {res && !res.supported && <p className="text-sm text-muted-foreground">{t("error.tld_unsupported")}</p>}
      {res?.supported && res.pricing && <OrderForm key={res.name} check={res as CheckResult & { pricing: Tld }} onDone={(_, inv) => nav(`/invoices/${inv}`)} />}
      {(tlds?.items.length ?? 0) > 0 && (
        <Card>
          <CardHeader><CardTitle>{t("domains.pricing")}</CardTitle></CardHeader>
          <CardContent>
            <ResponsiveTable<Tld> rows={tlds?.items} rowKey={(x) => x.tld}
              columns={[
                { header: t("domains.tld"), primary: true, cell: (x) => `.${x.tld}` },
                { header: t("domains.kind.register"), cell: (x) => formatMoney(x.register_price, x.currency, lang) },
                { header: t("domains.renew"), cell: (x) => formatMoney(x.renew_price, x.currency, lang) },
                { header: t("domains.kind.transfer"), cell: (x) => formatMoney(x.transfer_price, x.currency, lang) },
              ]} />
          </CardContent>
        </Card>
      )}
    </div>
  );
}

export function ClientDomainsPage() {
  const { t, lang } = useI18n();
  const [params] = useSearchParams();
  const { data, isLoading } = useQuery({ queryKey: ["client", "domains"], queryFn: () => api<{ items: Domain[] }>("/client/domains?per_page=100") });
  const ssl = useClientSsl();
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("domains.title")} description={t("domains.client_hint")} />
      <Tabs defaultValue={params.get("name") ? "new" : "mine"}>
        <TabsList className="w-full sm:w-auto">
          <TabsTrigger value="mine">{t("domains.mine")}</TabsTrigger>
          <TabsTrigger value="new">{t("domains.new")}</TabsTrigger>
        </TabsList>
        <TabsContent value="mine" className="grid gap-3">
          {(data?.items?.length ?? 0) > 0 && <DomainBulkBar domains={data!.items} />}
          <ListCard<Domain> rows={data?.items} rowKey={(x) => x.id} loading={isLoading} emptyIcon={GlobeIcon} emptyTitle={t("domains.empty")}
            columns={[
              { header: t("domains.name"), primary: true, cell: (x) => <Link className="font-medium break-all hover:underline" to={`/domains/${x.id}`}>{x.name}</Link> },
              { header: t("billing.status"), cell: (x) => <DomainStatus status={x.status} /> },
              { header: t("domains.expires"), cell: (x) => formatDate(x.expires_on, lang) },
              { header: t("ssl.title"), cell: (x) => <Link className="inline-flex" to="/ssl" title={t("nav.ssl")}><SslBadge check={ssl.get(x.id)} /></Link> },
              { header: t("domains.auto_renew"), cell: (x) => (x.auto_renew ? t("domains.on") : t("domains.off")) },
            ]} />
        </TabsContent>
        <TabsContent value="new"><DomainSearch /></TabsContent>
      </Tabs>
    </div>
  );
}

export function ClientDomainPage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const qc = useQueryClient();
  const key = ["client", "domain", id];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<DomainDetail>(`/client/domains/${id}`) });
  const [ns, setNs] = useState<string | null>(null);
  const [contact, setContact] = useState<Partial<Contact> | null>(null);
  const [privacy, setPrivacy] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const onErr = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const done = (r: DomainDetail) => { qc.setQueryData(key, r); setError(null); };
  const saveNs = useMutation({ mutationFn: () => api<DomainDetail>(`/client/domains/${id}/nameservers`, { method: "PUT", body: { nameservers: parseNameservers(ns ?? "") } }), onSuccess: (r) => { done(r); setNs(null); }, onError: onErr });
  const saveContact = useMutation({ mutationFn: () => api<DomainDetail>(`/client/domains/${id}/contact`, { method: "PUT", body: { contact, privacy } }), onSuccess: (r) => { done(r); setContact(null); setPrivacy(null); }, onError: onErr });
  const autoRenew = useMutation({ mutationFn: (v: boolean) => api<DomainDetail>(`/client/domains/${id}/auto-renew`, { method: "PUT", body: { auto_renew: v } }), onSuccess: done, onError: onErr });
  const [epp, setEpp] = useState<string | null>(null);
  const reveal = useMutation({ mutationFn: () => api<{ code: string }>(`/client/domains/${id}/epp`, { method: "POST" }), onSuccess: (r) => { setEpp(r.code); setError(null); }, onError: onErr });
  const addons = useMutation({ mutationFn: (body: Record<string, boolean>) => api<DomainDetail>(`/client/domains/${id}/addons`, { method: "PUT", body }), onSuccess: (r) => { done(r); qc.invalidateQueries({ queryKey: ["dns"] }); }, onError: onErr });
  const renew = useMutation({ mutationFn: () => api<{ invoice_id: string }>(`/client/domains/${id}/renew`, { method: "POST", body: { years: 1 } }), onSuccess: (r) => nav(`/invoices/${r.invoice_id}`), onError: onErr });
  if (!data) return <PageSkeleton />;
  const d = data.domain;
  const editable = d.status === "active" || d.status === "pending";
  return (
    <div className="flex flex-col gap-4">
      <Button asChild variant="ghost" className="w-fit"><Link to="/domains"><ArrowLeftIcon aria-hidden />{t("domains.title")}</Link></Button>
      <PageHeader title={d.name} actions={<DomainStatus status={d.status} />} />
      <FormError message={error} />
      <RegistrantAlert domainId={id} v={data.registrant_verification} />
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Replaceable section="overview" ctx={{ domain_id: d.id, status: d.status }}><Card>
          <CardHeader><CardTitle>{t("domains.overview")}</CardTitle></CardHeader>
          <CardContent className="grid gap-3 text-sm">
            <div className="flex justify-between gap-2"><span className="text-muted-foreground">{t("domains.registered")}</span><span>{formatDate(d.registered_on, lang)}</span></div>
            <div className="flex justify-between gap-2"><span className="text-muted-foreground">{t("domains.expires")}</span><span>{formatDate(d.expires_on, lang)}</span></div>
            <div className="flex items-center justify-between gap-2"><Label htmlFor="d-ar">{t("domains.auto_renew")}</Label>
              <Switch id="d-ar" checked={d.auto_renew} disabled={autoRenew.isPending} onCheckedChange={(v) => autoRenew.mutate(v)} /></div>
            {d.status === "pending" && d.invoice_id && <Button asChild variant="outline"><Link to={`/invoices/${d.invoice_id}`}>{t("domains.pay_invoice")}</Link></Button>}
            {d.status === "grace" && <p className="text-sm text-muted-foreground">{t("domains.grace_hint")}</p>}
            {d.status === "redemption" && <p className="text-sm text-destructive">{t("domains.redemption_hint")}</p>}
            {RENEWABLE.includes(d.status) && <Button onClick={() => renew.mutate()} disabled={renew.isPending}>{t("domains.renew_now")}</Button>}
            {d.status === "active" && (
              <div className="grid gap-2 border-t pt-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="flex items-center gap-2 text-muted-foreground"><KeyIcon aria-hidden />{t("domains.epp_code")}</span>
                  {!epp && <Button size="sm" variant="outline" disabled={reveal.isPending} onClick={() => reveal.mutate()}>{t("domains.epp_reveal")}</Button>}
                </div>
                {epp && (
                  <div className="flex items-center gap-2">
                    <Input readOnly value={epp} className="font-mono" aria-label={t("domains.epp_code")} onFocus={(e) => e.currentTarget.select()} />
                    <Button size="icon" variant="outline" aria-label={t("domains.epp_copy")} onClick={() => { void navigator.clipboard?.writeText(epp); toast.success(t("domains.epp_copied")); }}><CopyIcon aria-hidden /></Button>
                  </div>
                )}
                <p className="text-xs text-muted-foreground">{t("domains.epp_hint")}</p>
              </div>
            )}
          </CardContent>
        </Card></Replaceable>
        <Replaceable section="nameservers" ctx={{ domain_id: d.id, status: d.status }}><Card>
          <CardHeader><CardTitle>{t("domains.nameservers")}</CardTitle><CardDescription>{t("domains.nameservers_hint")}</CardDescription></CardHeader>
          <CardContent>
            <form className="grid gap-3" onSubmit={(e) => { e.preventDefault(); saveNs.mutate(); }}>
              <Textarea aria-label={t("domains.nameservers")} rows={4} disabled={!editable} value={ns ?? d.nameservers.join("\n")} onChange={(e) => setNs(e.target.value)} />
              <Button type="submit" className="w-fit" disabled={!editable || ns === null || saveNs.isPending}>{t("common.save")}</Button>
            </form>
          </CardContent>
        </Card></Replaceable>
      </div>
      <Replaceable section="addons" ctx={{ domain_id: d.id, status: d.status }}><Card>
        <CardHeader><CardTitle>{t("domains.addons")}</CardTitle><CardDescription>{t("domains.addons_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-3 text-sm">
          <div className="flex items-center justify-between gap-2"><Label htmlFor="d-dns">{t("domains.dns_management")}</Label>
            <Switch id="d-dns" checked={!!d.dns_management} disabled={!editable || addons.isPending} onCheckedChange={(v) => addons.mutate({ dns_management: v })} /></div>
          <div className="flex items-center justify-between gap-2"><Label htmlFor="d-lock">{t("domains.registrar_lock")}</Label>
            <Switch id="d-lock" checked={!!d.registrar_lock} disabled={!editable || addons.isPending} onCheckedChange={(v) => addons.mutate({ registrar_lock: v })} /></div>
        </CardContent>
      </Card></Replaceable>
      <Replaceable section="dns" ctx={{ domain_id: d.id, status: d.status }}><Card>
        <CardHeader><CardTitle>{t("domains.dns")}</CardTitle></CardHeader>
        <CardContent><DnsEditor base={`/client/domains/${id}`} disabled={!d.dns_management} /></CardContent>
      </Card></Replaceable>
      <Replaceable section="whois" ctx={{ domain_id: d.id, status: d.status }}><Card>
        <CardHeader><CardTitle>{t("domains.whois")}</CardTitle></CardHeader>
        <CardContent>
          <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); saveContact.mutate(); }}>
            <ContactFields idPrefix="c" value={contact ?? d.contact} onChange={setContact} />
            <div className="flex items-center gap-2"><Switch id="c-priv" checked={privacy ?? d.privacy} onCheckedChange={(v) => { setPrivacy(v); if (!contact) setContact(d.contact); }} /><Label htmlFor="c-priv">{t("domains.privacy")}</Label></div>
            <Button type="submit" className="w-fit" disabled={!contact || saveContact.isPending}>{t("common.save")}</Button>
          </form>
        </CardContent>
      </Card></Replaceable>
    </div>
  );
}
