import { useState } from "react";
import { Link } from "react-router";
import { GlobeIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { PageHeader } from "@/components/PageHeader";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { PublicCheck, Tld, WhoisInfo } from "@/lib/store/domains";

/** Storefront domain search, TLD price list and WHOIS (no login needed). */
export function DomainSearchPage() {
  const { t, tCode, lang } = useI18n();
  const [q, setQ] = useState("");
  const [w, setW] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { data: tlds, isLoading } = useQuery({ queryKey: ["public", "tlds"], queryFn: () => api<{ items: Tld[] }>("/public/domains/tlds") });
  const err = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const check = useMutation({
    mutationFn: () => {
      let name = q.trim().toLowerCase();
      if (name && !name.includes(".") && tlds?.items[0]) name = `${name}.${tlds.items[0].tld}`;
      return api<{ supported: boolean; results: PublicCheck[] }>(`/public/domains/check?suggest=true&name=${encodeURIComponent(name)}`);
    },
    onSuccess: () => setError(null), onError: err,
  });
  const whois = useMutation({ mutationFn: () => api<{ whois: WhoisInfo }>(`/public/domains/whois?name=${encodeURIComponent(w.trim())}`), onSuccess: () => setError(null), onError: err });
  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-4 px-4 py-8">
      <PageHeader title={t("domains.find_title")} description={t("domains.find_hint")} />
      <FormError message={error} />
      <Tabs defaultValue="search">
        <TabsList className="w-full sm:w-auto">
          <TabsTrigger value="search">{t("domains.check")}</TabsTrigger>
          <TabsTrigger value="whois">{t("domains.whois_lookup")}</TabsTrigger>
        </TabsList>
        <TabsContent value="search" className="flex flex-col gap-4">
          <form className="flex flex-col gap-2 sm:flex-row" onSubmit={(e) => { e.preventDefault(); if (q.trim()) check.mutate(); }}>
            <Input aria-label={t("domains.search")} placeholder={t("domains.search_placeholder")} value={q} onChange={(e) => setQ(e.target.value)} />
            <Button type="submit" disabled={check.isPending}><MagnifyingGlassIcon aria-hidden />{t("domains.check")}</Button>
          </form>
          {check.data && !check.data.supported && <p className="text-sm text-muted-foreground">{t("error.tld_unsupported")}</p>}
          {check.data?.results.map((r) => (
            <Card key={r.name}>
              <CardContent className="flex flex-col gap-2 pt-6 sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0">
                  <p className="break-all font-medium">{r.name}</p>
                  <div className="mt-1 flex flex-wrap gap-1.5">
                    <Badge variant={r.available ? "default" : "outline"}>{r.available ? t("domains.available") : t("domains.taken")}</Badge>
                    {r.pricing?.tld_group && <Badge variant="muted">{tCode(`domains.group.${r.pricing.tld_group}`)}</Badge>}
                  </div>
                </div>
                {r.pricing && (
                  <div className="flex items-center gap-3">
                    <span className="text-sm">{formatMoney(r.available ? r.pricing.register_price : r.pricing.transfer_price, r.pricing.currency, lang)}</span>
                    <Button asChild size="sm" variant={r.available ? "default" : "outline"}><Link to={`/domains?name=${encodeURIComponent(r.name)}`}>{r.available ? t("domains.order") : t("domains.kind.transfer")}</Link></Button>
                  </div>
                )}
              </CardContent>
            </Card>
          ))}
          <Card>
            <CardHeader><CardTitle>{t("domains.pricing")}</CardTitle></CardHeader>
            <CardContent>
              <ResponsiveTable<Tld> rows={tlds?.items} loading={isLoading} rowKey={(x) => `${x.tld}-${x.currency}`} emptyIcon={GlobeIcon} emptyTitle={t("domains.no_tlds")}
                columns={[
                  { header: t("domains.tld"), primary: true, cell: (x) => <span>.{x.tld}{x.tld_group && <Badge variant="muted" className="ml-2">{tCode(`domains.group.${x.tld_group}`)}</Badge>}</span> },
                  { header: t("domains.kind.register"), cell: (x) => formatMoney(x.register_price, x.currency, lang) },
                  { header: t("domains.renew"), cell: (x) => formatMoney(x.renew_price, x.currency, lang) },
                  { header: t("domains.kind.transfer"), cell: (x) => formatMoney(x.transfer_price, x.currency, lang) },
                ]} />
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="whois" className="flex flex-col gap-4">
          <form className="flex flex-col gap-2 sm:flex-row" onSubmit={(e) => { e.preventDefault(); if (w.trim()) whois.mutate(); }}>
            <Input aria-label={t("domains.whois_lookup")} placeholder="example.com" value={w} onChange={(e) => setW(e.target.value)} />
            <Button type="submit" disabled={whois.isPending}><MagnifyingGlassIcon aria-hidden />{t("domains.lookup")}</Button>
          </form>
          {whois.data && (
            <Card>
              <CardHeader><CardTitle className="break-all">{w.trim().toLowerCase()}</CardTitle>
                <CardDescription>{whois.data.whois.registered ? t("domains.taken") : t("domains.available")}</CardDescription></CardHeader>
              {whois.data.whois.registered && (
                <CardContent className="grid gap-2 text-sm">
                  {[["domains.registrar", whois.data.whois.registrar], ["domains.registered", formatDate(whois.data.whois.created ?? null, lang)], ["domains.expires", formatDate(whois.data.whois.expires ?? null, lang)], ["billing.status", whois.data.whois.status.join(", ")]].map(([k, v]) => (
                    <div key={k} className="flex justify-between gap-2"><span className="text-muted-foreground">{tCode(k as string)}</span><span className="break-all text-right">{v || "-"}</span></div>
                  ))}
                  <p className="pt-2 font-medium">{t("domains.nameservers")}</p>
                  {whois.data.whois.nameservers.map((n) => <span key={n} className="break-all">{n}</span>)}
                </CardContent>
              )}
            </Card>
          )}
        </TabsContent>
      </Tabs>
    </div>
  );
}
