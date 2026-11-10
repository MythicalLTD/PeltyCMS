import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowCounterClockwiseIcon, CopyIcon, DownloadSimpleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { activationLabel, activationTarget, fileSize, type DigitalVersion, type LicenseView } from "@/lib/digital";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

/** Client: license keys and downloads of owned digital products. */
export function DigitalProductsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const products = useQuery({ queryKey: ["client", "digital", "products"], queryFn: () => api<{ items: { product_id: string; name: string; versions: DigitalVersion[] }[] }>("/client/digital/products") });
  const licenses = useQuery({ queryKey: ["client", "digital", "licenses"], queryFn: () => api<{ items: LicenseView[] }>("/client/digital/licenses") });
  const link = useMutation({
    mutationFn: (id: string) => api<{ url: string }>(`/client/digital/versions/${id}/link`, { method: "POST" }),
    onSuccess: (r) => window.location.assign(r.url),
    onError: fail,
  });
  const reset = useMutation({
    mutationFn: (id: string) => api(`/client/digital/licenses/${id}/reset`, { method: "POST" }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["client", "digital", "licenses"] }); toast.success(t("digital.reset_done")); },
    onError: fail,
  });
  const copy = (k: string) => void navigator.clipboard.writeText(k).then(() => toast.success(t("digital.copied")));
  if (!products.data || !licenses.data) return <Skeleton className="h-48 w-full" />;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("digital.my_title")} description={t("digital.my_description")} />
      {products.data.items.length === 0 && licenses.data.items.length === 0 && <EmptyState title={t("digital.my_empty")} />}
      {licenses.data.items.length > 0 && (
        <section className="flex flex-col gap-3" aria-label={t("digital.licenses")}>
          <h2 className="text-lg font-semibold">{t("digital.licenses")}</h2>
          <div className="grid gap-3 md:grid-cols-2">
            {licenses.data.items.map(({ license: l, key, activations }) => (
              <Card key={l.id} data-license={l.status}>
                <CardHeader>
                  <CardTitle className="text-base">{l.product}</CardTitle>
                  <CardDescription className="flex flex-wrap items-center gap-2">
                    <Badge variant={l.status === "active" ? "secondary" : l.status === "revoked" ? "destructive" : "outline"}>{t(`digital.st.${l.status}` as TranslationKey)}</Badge>
                    <span>{t("digital.activations")}: {activationLabel(l.activations, l.max_activations)}</span>
                    {l.expires_at && <span>{t("digital.expires", { date: new Date(l.expires_at).toLocaleDateString() })}</span>}
                  </CardDescription>
                </CardHeader>
                <CardContent className="flex flex-col gap-2 text-sm">
                  <div className="flex items-center gap-2">
                    <code className="flex-1 break-all rounded bg-muted px-2 py-1 font-mono">{key ?? l.key_hint}</code>
                    {key && <Button size="sm" variant="ghost" aria-label={t("digital.copy")} onClick={() => copy(key)}><CopyIcon aria-hidden /></Button>}
                  </div>
                  {activations.map((a) => <div key={a.id} className="text-muted-foreground">{activationTarget(a)}</div>)}
                  {activations.length > 0 && <Button size="sm" variant="outline" className="self-start" onClick={() => reset.mutate(l.id)}><ArrowCounterClockwiseIcon aria-hidden />{t("digital.reset")}</Button>}
                </CardContent>
              </Card>
            ))}
          </div>
        </section>
      )}
      {products.data.items.map((p) => (
        <Card key={p.product_id}>
          <CardHeader><CardTitle className="text-base">{p.name}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            {p.versions.length === 0 && <p className="text-muted-foreground">{t("digital.no_versions")}</p>}
            {p.versions.map((v, i) => (
              <div key={v.id} className="flex flex-wrap items-start justify-between gap-2 border-b pb-2 last:border-0">
                <div>
                  <div className="font-medium">v{v.version} {i === 0 && <Badge variant="secondary">{t("digital.latest")}</Badge>} <span className="text-muted-foreground">· {fileSize(v.size_bytes)} · {new Date(v.created_at).toLocaleDateString()}</span></div>
                  {v.changelog && <p className="whitespace-pre-wrap text-muted-foreground">{v.changelog}</p>}
                </div>
                <Button size="sm" variant={i === 0 ? "default" : "outline"} disabled={link.isPending} onClick={() => link.mutate(v.id)}><DownloadSimpleIcon aria-hidden />{t("digital.download")}</Button>
              </div>
            ))}
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
