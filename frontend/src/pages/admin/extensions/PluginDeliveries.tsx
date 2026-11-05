import { useState } from "react";
import { Link, useParams } from "react-router";
import { ArrowClockwiseIcon, KeyIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { DataTable } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface Delivery { id: string; event: string; status: "pending" | "delivered" | "failed"; attempts: number; response_status: number | null; last_error: string | null; url: string | null; created_at: string; delivered_at: string | null }
export interface WebhookInfo { manifest_url: string; url_override: string | null; secret_set: boolean; hooks: string[]; enabled: boolean }

export const DELIVERY_STATUSES = ["pending", "delivered", "failed"] as const;

/** Query path for the delivery log, with an optional status filter. */
export function deliveriesPath(id: string, status: string): string {
  const base = `/admin/plugins/${encodeURIComponent(id)}/deliveries`;
  return status ? `${base}?status=${encodeURIComponent(status)}` : base;
}

/** Admin: one external plugin's webhook endpoint, signing secret and delivery log. */
export function AdminPluginDeliveriesPage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const enc = encodeURIComponent(id);
  const [status, setStatus] = useState("");
  const [url, setUrl] = useState("");
  const [secret, setSecret] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const info = useQuery({ queryKey: ["admin", "plugin-webhook", id], queryFn: () => api<WebhookInfo>(`/admin/plugins/${enc}/webhook`) });
  const [seenInfo, setSeenInfo] = useState(info.data);
  if (info.data !== seenInfo) {
    setSeenInfo(info.data);
    if (info.data !== undefined) setUrl(info.data.url_override ?? "");
  }
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const ok = () => { setError(null); void qc.invalidateQueries({ queryKey: ["admin", "plugin-webhook", id] }); void qc.invalidateQueries({ queryKey: ["admin", "plugin-deliveries", id] }); };
  const save = useMutation({ mutationFn: () => api<void>(`/admin/plugins/${enc}/webhook`, { method: "PUT", body: { url: url.trim() || null } }), onSuccess: ok, onError: fail });
  const rotate = useMutation({ mutationFn: () => api<{ secret: string }>(`/admin/plugins/${enc}/webhook/secret`, { method: "POST" }), onSuccess: (r) => { setSecret(r.secret); ok(); }, onError: fail });
  const redeliver = useMutation({ mutationFn: (did: string) => api<void>(`/admin/plugins/${enc}/deliveries/${did}/redeliver`, { method: "POST" }), onSuccess: ok, onError: fail });
  return (
    <div className="flex flex-col gap-4">
      <Link to={`/admin/plugins/${enc}`} className="text-sm text-muted-foreground hover:text-foreground">← {id}</Link>
      <FormError message={error} />
      <Card>
        <CardHeader>
          <CardTitle>{t("plugins.webhook.title")}</CardTitle>
          <CardDescription>{t("plugins.webhook.description")}</CardDescription>
        </CardHeader>
        <CardContent className="flex max-w-xl flex-col gap-4">
          {info.isError ? <p className="text-sm text-muted-foreground">{t("plugins.webhook.none")}</p> : <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-muted-foreground">{t("plugins.webhook.manifest_url")}</dt><dd className="break-all">{info.data?.manifest_url}</dd>
              <dt className="text-muted-foreground">{t("plugins.webhook.events")}</dt><dd>{info.data?.hooks.join(", ")}</dd>
            </dl>
            <form className="flex flex-col gap-2" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
              <Label htmlFor="hook-url">{t("plugins.webhook.override")}</Label>
              <div className="flex gap-2">
                <Input id="hook-url" type="url" maxLength={2000} placeholder={info.data?.manifest_url} value={url} onChange={(e) => setUrl(e.target.value)} />
                <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
              </div>
            </form>
            <div className="flex flex-col gap-2">
              <p className="text-sm text-muted-foreground">{info.data?.secret_set ? t("plugins.webhook.secret_set") : t("plugins.webhook.secret_unset")}</p>
              {secret && (
                <div className="flex flex-col gap-1">
                  <Label htmlFor="hook-secret">{t("plugins.webhook.secret_once")}</Label>
                  <Input id="hook-secret" readOnly value={secret} className="font-mono" onFocus={(e) => e.currentTarget.select()} />
                </div>
              )}
              <div>
                <ConfirmAction description={t("plugins.webhook.rotate_confirm")} onConfirm={() => rotate.mutate()}>
                  <Button variant="outline" size="sm" disabled={rotate.isPending}><KeyIcon aria-hidden />{t("plugins.webhook.rotate")}</Button>
                </ConfirmAction>
              </div>
            </div>
          </>}
        </CardContent>
      </Card>
      <DataTable<Delivery>
        title={t("plugins.webhook.deliveries")}
        path={deliveriesPath(id, status)}
        queryKey={["admin", "plugin-deliveries", id, status]}
        emptyTitle={t("plugins.webhook.empty")}
        toolbar={
          <Select value={status || "all"} onValueChange={(v) => setStatus(v === "all" ? "" : v)}>
            <SelectTrigger aria-label={t("billing.status")} className="h-11 w-40 md:h-8"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("common.all")}</SelectItem>
              {DELIVERY_STATUSES.map((s) => <SelectItem key={s} value={s}>{t(`plugins.webhook.status.${s}`)}</SelectItem>)}
            </SelectContent>
          </Select>
        }
        columns={[
          { header: t("plugins.webhook.event"), cell: (d) => <span className="font-mono text-xs">{d.event}</span> },
          { header: t("billing.status"), cell: (d) => <StatusBadge status={d.status} /> },
          { header: t("plugins.webhook.attempts"), cell: (d) => <span className="tabular-nums">{d.attempts}</span> },
          { header: t("plugins.webhook.response"), cell: (d) => d.response_status ?? (d.last_error ? <span className="text-muted-foreground">{d.last_error}</span> : "-") },
          { header: t("plugins.webhook.created"), cell: (d) => formatDateTime(d.created_at, lang) },
          { header: "", cell: (d) => (
            <Button size="sm" variant="ghost" disabled={redeliver.isPending || d.status === "pending"} onClick={() => redeliver.mutate(d.id)}>
              <ArrowClockwiseIcon aria-hidden />{t("plugins.webhook.redeliver")}
            </Button>
          ) },
        ]}
      />
    </div>
  );
}
