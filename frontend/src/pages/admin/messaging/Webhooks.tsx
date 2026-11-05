import { useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { ArrowClockwiseIcon, KeyIcon, PaperPlaneTiltIcon, PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DataTable } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface WebhookEndpoint { id: string; name: string; url: string; events: string[]; enabled: boolean; failure_streak: number; disabled_reason: string; created_at: string; last_delivery_at: string | null }
export interface CatalogEvent { name: string; category: string; description: string }
export interface WebhookDelivery { id: string; endpoint_id: string; event: string; payload: unknown; status: "pending" | "delivered" | "failed"; attempts: number; response_status: number | null; last_error: string | null; duration_ms: number | null; redelivery_of: string | null; created_at: string; delivered_at: string | null }
interface ListResp { items: WebhookEndpoint[]; events: CatalogEvent[]; signing_available: boolean }

/** Group catalog events by category, categories sorted. */
export function groupEvents(events: CatalogEvent[]): [string, CatalogEvent[]][] {
  const m = new Map<string, CatalogEvent[]>();
  for (const e of events) m.set(e.category, [...(m.get(e.category) ?? []), e]);
  return [...m.entries()].toSorted(([a], [b]) => a.localeCompare(b));
}

/** Toggle a filter entry; picking `*` or `cat.*` drops the entries it covers. */
export function toggleFilter(sel: string[], f: string, on: boolean): string[] {
  if (!on) return sel.filter((x) => x !== f);
  if (f === "*") return ["*"];
  if (f.endsWith(".*")) {
    const p = f.slice(0, -1);
    return [...sel.filter((x) => !x.startsWith(p)), f];
  }
  return [...sel, f];
}

/** Delivery-log query path with optional filters. */
export function deliveriesQuery(endpoint: string, status: string): string {
  const q = new URLSearchParams();
  if (endpoint) q.set("endpoint_id", endpoint);
  if (status) q.set("status", status);
  const s = q.toString();
  return s ? `/admin/webhook-deliveries?${s}` : "/admin/webhook-deliveries";
}

interface Draft { id?: string; name: string; url: string; events: string[]; enabled: boolean }
const EMPTY: Draft = { name: "", url: "https://", events: [], enabled: true };

function EventPicker({ events, value, onChange }: { events: CatalogEvent[]; value: string[]; onChange: (v: string[]) => void }) {
  const { t } = useI18n();
  const all = value.includes("*");
  return (
    <div className="flex max-h-80 flex-col gap-3 overflow-y-auto rounded-md border p-3 text-sm">
      <label className="flex items-center gap-2 font-medium">
        <Checkbox checked={all} onCheckedChange={(v) => onChange(toggleFilter(value, "*", !!v))} />{t("webhooks.all_events")}
      </label>
      {!all && groupEvents(events).map(([cat, list]) => {
        const wild = value.includes(`${cat}.*`);
        return (
          <fieldset key={cat} className="flex flex-col gap-1">
            <label className="flex items-center gap-2 font-medium">
              <Checkbox checked={wild} onCheckedChange={(v) => onChange(toggleFilter(value, `${cat}.*`, !!v))} /><span className="font-mono text-xs">{cat}.*</span>
            </label>
            {!wild && <div className="grid gap-1 pl-6 sm:grid-cols-2">
              {list.map((e) => (
                <label key={e.name} className="flex items-center gap-2" title={e.description}>
                  <Checkbox checked={value.includes(e.name)} onCheckedChange={(v) => onChange(toggleFilter(value, e.name, !!v))} /><span className="font-mono text-xs">{e.name}</span>
                </label>
              ))}
            </div>}
          </fieldset>
        );
      })}
    </div>
  );
}

/** Admin: outgoing webhook endpoints, their event filters and the delivery log. */
export function AdminWebhooksPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [secret, setSecret] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [endpoint, setEndpoint] = useState("");
  const [status, setStatus] = useState("");
  const [detail, setDetail] = useState<string | null>(null);
  const list = useQuery({ queryKey: ["admin", "webhook-endpoints"], queryFn: () => api<ListResp>("/admin/webhook-endpoints") });
  const one = useQuery({ queryKey: ["admin", "webhook-delivery", detail], enabled: !!detail, queryFn: () => api<{ delivery: WebhookDelivery }>(`/admin/webhook-deliveries/${detail}`) });
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => { setError(null); void qc.invalidateQueries({ queryKey: ["admin", "webhook-endpoints"] }); void qc.invalidateQueries({ queryKey: ["admin", "webhook-deliveries"] }); };
  const save = useMutation({
    mutationFn: (d: Draft) => {
      const body = { name: d.name.trim(), url: d.url.trim(), events: d.events, enabled: d.enabled };
      return d.id ? api<{ endpoint: WebhookEndpoint; secret?: string }>(`/admin/webhook-endpoints/${d.id}`, { method: "PUT", body }) : api<{ endpoint: WebhookEndpoint; secret?: string }>("/admin/webhook-endpoints", { method: "POST", body });
    },
    onSuccess: (r: { secret?: string }) => { if (r.secret) setSecret(r.secret); setDraft(null); refresh(); },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (id: string) => api<void>(`/admin/webhook-endpoints/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const rotate = useMutation({ mutationFn: (id: string) => api<{ secret: string }>(`/admin/webhook-endpoints/${id}/secret`, { method: "POST" }), onSuccess: (r) => { setSecret(r.secret); refresh(); }, onError: fail });
  const test = useMutation({ mutationFn: (id: string) => api<{ delivery_id: string }>(`/admin/webhook-endpoints/${id}/test`, { method: "POST" }), onSuccess: refresh, onError: fail });
  const redeliver = useMutation({ mutationFn: (id: string) => api<{ delivery_id: string }>(`/admin/webhook-deliveries/${id}/redeliver`, { method: "POST" }), onSuccess: refresh, onError: fail });
  const items = list.data?.items ?? [];
  const nameOf = (id: string) => items.find((e) => e.id === id)?.name ?? id.slice(0, 8);

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("nav.webhooks")} description={t("webhooks.description")}
        actions={<Button onClick={() => { setSecret(null); setDraft({ ...EMPTY }); }}><PlusIcon aria-hidden />{t("webhooks.add")}</Button>} />
      <FormError message={error} />
      {list.data && !list.data.signing_available && <FormError message={t("webhooks.no_encryption_key")} />}
      <Card>
        <CardHeader><CardTitle>{t("webhooks.title")}</CardTitle></CardHeader>
        <CardContent className="flex flex-col gap-3">
          {secret && (
            <div className="flex flex-col gap-1">
              <Label htmlFor="wh-secret">{t("webhooks.secret_once")}</Label>
              <Input id="wh-secret" readOnly value={secret} className="font-mono" onFocus={(e) => e.currentTarget.select()} />
            </div>
          )}
          {items.length === 0 && !list.isLoading && <p className="text-sm text-muted-foreground">{t("webhooks.empty")}</p>}
          {items.map((e) => (
            <div key={e.id} className="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex min-w-0 flex-col gap-0.5">
                <div className="flex items-center gap-2 font-medium">{e.name}<StatusBadge status={e.enabled ? "active" : "disabled"} /></div>
                <span className="break-all font-mono text-xs text-muted-foreground">{e.url}</span>
                <span className="text-xs text-muted-foreground">{e.events.join(", ")}</span>
                {!e.enabled && e.disabled_reason && <span className="text-xs text-destructive">{e.disabled_reason}</span>}
                {e.failure_streak > 0 && <span className="text-xs text-muted-foreground">{t("webhooks.failure_streak", { n: String(e.failure_streak) })}</span>}
              </div>
              <div className="flex flex-wrap gap-1">
                <Button size="sm" variant="outline" disabled={test.isPending} onClick={() => test.mutate(e.id)}><PaperPlaneTiltIcon aria-hidden />{t("webhooks.test")}</Button>
                <Button size="sm" variant="outline" onClick={() => { setSecret(null); setDraft({ id: e.id, name: e.name, url: e.url, events: e.events, enabled: e.enabled }); }}><PencilSimpleIcon aria-hidden />{t("webhooks.edit")}</Button>
                <ConfirmAction description={t("webhooks.rotate_confirm")} onConfirm={() => rotate.mutate(e.id)}>
                  <Button size="sm" variant="outline"><KeyIcon aria-hidden />{t("webhooks.rotate")}</Button>
                </ConfirmAction>
                <ConfirmAction description={t("webhooks.delete_confirm")} onConfirm={() => remove.mutate(e.id)}>
                  <Button size="sm" variant="ghost" aria-label={t("webhooks.delete")}><TrashIcon aria-hidden /></Button>
                </ConfirmAction>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <Dialog open={!!draft} onOpenChange={(o) => !o && setDraft(null)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader><DialogTitle>{draft?.id ? t("webhooks.edit") : t("webhooks.add")}</DialogTitle></DialogHeader>
          {draft && (
            <form className="flex flex-col gap-3" onSubmit={(ev) => { ev.preventDefault(); save.mutate(draft); }}>
              <Label htmlFor="wh-name">{t("webhooks.name")}</Label>
              <Input id="wh-name" required maxLength={100} value={draft.name} onChange={(ev) => setDraft({ ...draft, name: ev.target.value })} />
              <Label htmlFor="wh-url">{t("webhooks.url")}</Label>
              <Input id="wh-url" type="url" required maxLength={500} value={draft.url} onChange={(ev) => setDraft({ ...draft, url: ev.target.value })} />
              <Label>{t("webhooks.events")}</Label>
              <EventPicker events={list.data?.events ?? []} value={draft.events} onChange={(events) => setDraft({ ...draft, events })} />
              <label className="flex items-center gap-2 text-sm"><Switch checked={draft.enabled} onCheckedChange={(v) => setDraft({ ...draft, enabled: v })} />{t("webhooks.enabled")}</label>
              <div className="flex justify-end gap-2">
                <Button type="button" variant="ghost" onClick={() => setDraft(null)}>{t("common.cancel")}</Button>
                <Button type="submit" disabled={save.isPending || draft.events.length === 0}>{t("common.save")}</Button>
              </div>
            </form>
          )}
        </DialogContent>
      </Dialog>

      <DataTable<WebhookDelivery>
        title={t("webhooks.deliveries")}
        path={deliveriesQuery(endpoint, status)}
        queryKey={["admin", "webhook-deliveries", endpoint, status]}
        emptyTitle={t("webhooks.no_deliveries")}
        toolbar={
          <div className="flex flex-wrap gap-2">
            <Select value={endpoint || "all"} onValueChange={(v) => setEndpoint(v === "all" ? "" : v)}>
              <SelectTrigger aria-label={t("webhooks.endpoint")} className="h-11 w-44 md:h-8"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("common.all")}</SelectItem>
                {items.map((e) => <SelectItem key={e.id} value={e.id}>{e.name}</SelectItem>)}
              </SelectContent>
            </Select>
            <Select value={status || "all"} onValueChange={(v) => setStatus(v === "all" ? "" : v)}>
              <SelectTrigger aria-label={t("billing.status")} className="h-11 w-36 md:h-8"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("common.all")}</SelectItem>
                {(["pending", "delivered", "failed"] as const).map((s) => <SelectItem key={s} value={s}>{t(`plugins.webhook.status.${s}`)}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
        }
        columns={[
          { header: t("webhooks.event"), cell: (d) => <button type="button" className="font-mono text-xs underline-offset-2 hover:underline" onClick={() => setDetail(d.id)}>{d.event}</button> },
          { header: t("webhooks.endpoint"), cell: (d) => nameOf(d.endpoint_id) },
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

      <Dialog open={!!detail} onOpenChange={(o) => !o && setDetail(null)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader><DialogTitle>{one.data?.delivery.event ?? "…"}</DialogTitle></DialogHeader>
          {one.data && (
            <div className="flex flex-col gap-2 text-sm">
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
                <dt className="text-muted-foreground">ID</dt><dd className="font-mono text-xs">{one.data.delivery.id}</dd>
                <dt className="text-muted-foreground">{t("billing.status")}</dt><dd><StatusBadge status={one.data.delivery.status} /></dd>
                <dt className="text-muted-foreground">{t("webhooks.duration")}</dt><dd>{one.data.delivery.duration_ms ?? "-"} ms</dd>
                {one.data.delivery.redelivery_of && <><dt className="text-muted-foreground">{t("webhooks.redelivery_of")}</dt><dd className="font-mono text-xs">{one.data.delivery.redelivery_of}</dd></>}
              </dl>
              <Label>{t("webhooks.payload")}</Label>
              <pre className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs">{JSON.stringify(one.data.delivery.payload, null, 2)}</pre>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
