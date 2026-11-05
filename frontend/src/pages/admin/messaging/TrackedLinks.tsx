import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CopyIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Link { id: string; slug: string; name: string; target_url: string; clicks: number; last_click_at: string | null; created_at: string }

/** Public URL of a tracked link. */
export function trackedUrl(slug: string, origin = window.location.origin) {
  return `${origin}/api/v1/go/${slug}`;
}

/** Embed snippet for a public data feed (WHMCS Data Feeds). */
export function feedSnippet(kind: "product" | "tld", key: string, field: string, currency: string, cycle: string, origin = window.location.origin) {
  const k = encodeURIComponent(key.trim().replace(/^\./, ""));
  const qs = new URLSearchParams({ currency: currency.trim().toUpperCase() || "EUR" });
  if (kind === "product") { qs.set("field", field); if (field === "price") qs.set("cycle", cycle); }
  return `<script src="${origin}/api/v1/feeds/${kind === "product" ? "products" : "tlds"}/${k}?${qs.toString()}"></script>`;
}

function DataFeedsCard() {
  const { t } = useI18n();
  const [kind, setKind] = useState<"product" | "tld">("product");
  const [key, setKey] = useState("");
  const [field, setField] = useState("price");
  const [currency, setCurrency] = useState("EUR");
  const [cycle, setCycle] = useState("monthly");
  const snippet = key.trim() ? feedSnippet(kind, key, field, currency, cycle) : "";
  return (
    <Card>
      <CardHeader><CardTitle>{t("feeds.title")}</CardTitle><CardDescription>{t("feeds.description")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="grid gap-3 sm:grid-cols-5">
          <div className="flex flex-col gap-2"><Label htmlFor="fd-kind">{t("feeds.kind")}</Label>
            <Select value={kind} onValueChange={(v) => setKind(v as "product" | "tld")}><SelectTrigger id="fd-kind" className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="product">{t("feeds.kind_product")}</SelectItem><SelectItem value="tld">{t("feeds.kind_tld")}</SelectItem></SelectContent></Select></div>
          <div className="flex flex-col gap-2"><Label htmlFor="fd-key">{kind === "product" ? t("feeds.slug") : t("feeds.tld")}</Label><Input id="fd-key" value={key} placeholder={kind === "product" ? "web-pro" : "com"} onChange={(e) => setKey(e.target.value)} /></div>
          {kind === "product" && (
            <div className="flex flex-col gap-2"><Label htmlFor="fd-field">{t("feeds.field")}</Label>
              <Select value={field} onValueChange={setField}><SelectTrigger id="fd-field" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="price">{t("feeds.field_price")}</SelectItem><SelectItem value="name">{t("feeds.field_name")}</SelectItem><SelectItem value="description">{t("feeds.field_description")}</SelectItem></SelectContent></Select></div>
          )}
          {kind === "product" && field === "price" && (
            <div className="flex flex-col gap-2"><Label htmlFor="fd-cycle">{t("feeds.cycle")}</Label>
              <Select value={cycle} onValueChange={setCycle}><SelectTrigger id="fd-cycle" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{["monthly", "quarterly", "semiannual", "annual", "biennial", "triennial", "one_time"].map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent></Select></div>
          )}
          <div className="flex flex-col gap-2"><Label htmlFor="fd-cur">{t("feeds.currency")}</Label><Input id="fd-cur" value={currency} maxLength={3} onChange={(e) => setCurrency(e.target.value)} /></div>
        </div>
        {snippet && (
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
            <code className="min-w-0 flex-1 break-all rounded-md bg-muted p-2 text-xs">{snippet}</code>
            <Button size="sm" variant="outline" className="min-h-11 sm:min-h-9" onClick={() => { void navigator.clipboard.writeText(snippet); toast.success(t("links.copied")); }}><CopyIcon aria-hidden />{t("links.copy")}</Button>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

/** Link tracking (WHMCS Products > Link Tracking): short redirect links with click counts. */
export function AdminTrackedLinksPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "tracked-links"], queryFn: () => api<{ items: Link[] }>("/admin/tracked-links") });
  const [f, setF] = useState({ slug: "", name: "", target_url: "" });
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "tracked-links"] });
  const add = useMutation({
    mutationFn: () => api("/admin/tracked-links", { method: "POST", body: f }),
    onSuccess: () => { setF({ slug: "", name: "", target_url: "" }); refresh(); toast.success(t("common.saved")); },
    onError: fail,
  });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/tracked-links/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement>) => setF({ ...f, [k]: e.target.value });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("links.title")} description={t("links.description")} />
      <Card>
        <CardContent className="pt-6">
          <form className="grid gap-3 sm:grid-cols-[1fr_1fr_2fr_auto] sm:items-end" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
            <div className="flex flex-col gap-2"><Label htmlFor="ln-name">{t("links.name")}</Label><Input id="ln-name" value={f.name} maxLength={200} onChange={set("name")} /></div>
            <div className="flex flex-col gap-2"><Label htmlFor="ln-slug">{t("links.slug")}</Label><Input id="ln-slug" value={f.slug} maxLength={63} placeholder="spring-promo" onChange={set("slug")} /></div>
            <div className="flex flex-col gap-2"><Label htmlFor="ln-url">{t("links.target")}</Label><Input id="ln-url" type="url" value={f.target_url} placeholder="https://" onChange={set("target_url")} /></div>
            <Button type="submit" className="min-h-11 sm:min-h-9" disabled={!f.name.trim() || !f.slug.trim() || !f.target_url.trim() || add.isPending}>{t("common.add")}</Button>
          </form>
        </CardContent>
      </Card>
      {q.isLoading ? <Skeleton className="h-32" /> : (q.data?.items.length ?? 0) === 0 ? <EmptyState title={t("links.empty")} /> : (
        <ul className="flex flex-col gap-3">
          {q.data?.items.map((l) => (
            <li key={l.id}>
              <Card>
                <CardContent className="flex flex-col gap-2 pt-6 sm:flex-row sm:items-center sm:justify-between">
                  <div className="flex min-w-0 flex-col gap-1">
                    <span className="font-medium">{l.name}</span>
                    <code className="break-all text-xs">{trackedUrl(l.slug)}</code>
                    <span className="break-all text-xs text-muted-foreground">→ {l.target_url}</span>
                  </div>
                  <div className="flex items-center gap-3">
                    <div className="text-right text-sm">
                      <div className="font-semibold tabular-nums">{t("links.clicks", { count: l.clicks })}</div>
                      <div className="text-xs text-muted-foreground">{l.last_click_at ? formatDateTime(l.last_click_at, lang) : t("links.never")}</div>
                    </div>
                    <Button size="sm" variant="ghost" aria-label={t("links.copy")} onClick={() => { void navigator.clipboard.writeText(trackedUrl(l.slug)); toast.success(t("links.copied")); }}><CopyIcon aria-hidden /></Button>
                    <ConfirmAction description={t("links.delete_confirm")} onConfirm={() => del.mutate(l.id)}>
                      <Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
                    </ConfirmAction>
                  </div>
                </CardContent>
              </Card>
            </li>
          ))}
        </ul>
      )}
      <DataFeedsCard />
    </div>
  );
}
