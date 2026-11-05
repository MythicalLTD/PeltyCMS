import { useState } from "react";
import { ArrowSquareOutIcon, ArrowsClockwiseIcon, RocketLaunchIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface UpdateStatus { enabled: boolean; url: string; current: string; latest: string | null; release_url: string | null; notes: string | null; published_at: string | null; checked_at: string | null; error: string | null; update_available: boolean }
const KEY = ["admin", "updates"];

/** Dashboard banner; renders nothing unless a newer release is known. */
export function UpdateBanner() {
  const { t } = useI18n();
  const { data } = useQuery({ queryKey: KEY, queryFn: () => api<UpdateStatus>("/admin/system/updates"), retry: false, staleTime: 3_600_000 });
  if (!data?.update_available) return null;
  return (
    <Alert data-testid="update-banner">
      <RocketLaunchIcon aria-hidden />
      <AlertTitle>{t("updates.available", { version: data.latest ?? "" })}</AlertTitle>
      <AlertDescription className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <span>{t("updates.available_desc", { current: data.current })}</span>
        {data.release_url && <Button asChild size="sm" variant="outline"><a href={data.release_url} target="_blank" rel="noreferrer noopener">{t("updates.notes")}<ArrowSquareOutIcon aria-hidden /></a></Button>}
      </AlertDescription>
    </Alert>
  );
}

export function UpdatesCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: KEY, queryFn: () => api<UpdateStatus>("/admin/system/updates") });
  const [url, setUrl] = useState(data?.url ?? "");
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setUrl(data.url);
  }
  const [err, setErr] = useState<string | null>(null);
  const onErr = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({ mutationFn: (enabled: boolean) => api<UpdateStatus>("/admin/system/updates", { method: "PUT", body: { enabled, url } }), onSuccess: (r) => { qc.setQueryData(KEY, r); setErr(null); toast.success(t("common.saved")); }, onError: onErr });
  const check = useMutation({ mutationFn: () => api<UpdateStatus>("/admin/system/updates/check", { method: "POST" }), onSuccess: (r) => { qc.setQueryData(KEY, r); setErr(null); }, onError: onErr });
  if (!data) return <BlockSkeleton />;
  return (
    <Card>
      <CardHeader><CardTitle>{t("updates.title")}</CardTitle><CardDescription>{t("updates.desc")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        <UpdateBanner />
        <FormError message={err ?? (data.error ? tCode(data.error) : null)} />
        <div className="grid gap-2 sm:grid-cols-3">
          <div><div className="text-muted-foreground">{t("updates.current")}</div><div className="font-mono">{data.current}</div></div>
          <div><div className="text-muted-foreground">{t("updates.latest")}</div><div className="font-mono">{data.latest || "-"}</div></div>
          <div><div className="text-muted-foreground">{t("updates.checked")}</div><div>{data.checked_at ? formatDateTime(data.checked_at, lang) : t("updates.never")}</div></div>
        </div>
        {data.latest && !data.update_available && !data.error && <p className="text-muted-foreground">{t("updates.up_to_date")}</p>}
        <div className="flex items-center gap-2"><Switch id="up-on" checked={data.enabled} disabled={save.isPending} onCheckedChange={(v) => save.mutate(v)} /><Label htmlFor="up-on">{t("updates.enabled")}</Label></div>
        <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); save.mutate(data.enabled); }}>
          <Label htmlFor="up-url">{t("updates.url")}</Label>
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input id="up-url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://" className="min-w-0 flex-1" />
            <Button type="submit" variant="outline" disabled={save.isPending}>{t("common.save")}</Button>
            <Button type="button" onClick={() => check.mutate()} disabled={check.isPending}><ArrowsClockwiseIcon aria-hidden />{t("updates.check_now")}</Button>
          </div>
          <p className="text-xs text-muted-foreground">{t("updates.url_hint")}</p>
        </form>
      </CardContent>
    </Card>
  );
}
