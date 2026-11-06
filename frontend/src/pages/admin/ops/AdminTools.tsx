import { useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, PlusIcon, TrashIcon, WarningIcon, XCircleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { NOTIFY_PROVIDERS, overallStatus, providerFields, type NotificationDelivery, type NotificationRule, type SystemHealth } from "@/lib/admin/adminTools";
import { formatDateTime } from "@/lib/format";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { useI18n } from "@/lib/i18n";
import { UpdatesCard } from "@/components/admin/UpdateCheck";

function StatusIcon({ s }: { s: string }) {
  if (s === "ok") return <CheckCircleIcon className="size-5 text-primary" weight="fill" />;
  if (s === "warning") return <WarningIcon className="size-5 text-muted-foreground" weight="fill" />;
  return <XCircleIcon className="size-5 text-destructive" weight="fill" />;
}

/** WHMCS-style System Health: checks plus raw metrics. */
export function AdminSystemHealthPage() {
  const { t, tCode } = useI18n();
  const q = useQuery({ queryKey: ["admin", "system-health"], queryFn: () => api<SystemHealth>("/admin/system-health") });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("health.title")} description={t("health.description")} />
      {!q.data ? <Skeleton className="h-48" /> : (
        <>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2"><StatusIcon s={overallStatus(q.data.checks)} />{tCode(`health.overall.${overallStatus(q.data.checks)}`)}</CardTitle>
              <CardDescription>PeltyCMS {q.data.app_version} · PostgreSQL {String(q.data.metrics.db_version)}</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-3 sm:grid-cols-2">
              {q.data.checks.map((c) => (
                <div key={c.key} className="flex min-w-0 items-center gap-3 rounded-lg border p-3">
                  <StatusIcon s={c.status} />
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{tCode(`health.check.${c.key}`)}</div>
                    {c.value !== undefined && <div className="text-sm text-muted-foreground">{c.value}</div>}
                  </div>
                  <Badge variant={c.status === "ok" ? "secondary" : "destructive"}>{tCode(`health.overall.${c.status}`)}</Badge>
                </div>
              ))}
            </CardContent>
          </Card>
          <Card>
            <CardHeader><CardTitle>{t("health.metrics")}</CardTitle></CardHeader>
            <CardContent className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {Object.entries(q.data.metrics).map(([k, v]) => (
                <div key={k} className="flex min-w-0 justify-between gap-2 border-b py-1 text-sm">
                  <span className="truncate text-muted-foreground">{tCode(`health.metric.${k}`)}</span><span className="font-mono">{String(v)}</span>
                </div>
              ))}
            </CardContent>
          </Card>
          <UpdatesCard />
        </>
      )}
    </div>
  );
}

/** Personal scratchpad for the signed-in staff member. */
export function AdminMyNotesPage() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["admin", "my-notes"], queryFn: () => api<{ body: string }>("/admin/my-notes") });
  const [body, setBody] = useState("");
  const [seen, setSeen] = useState(q.data);
  if (q.data !== seen) {
    setSeen(q.data);
    if (q.data !== undefined) setBody(q.data.body);
  }
  const save = useMutation({
    mutationFn: () => api("/admin/my-notes", { method: "PUT", body: { body } }),
    onSuccess: () => toast.success(t("common.saved")),
  });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("my_notes.title")} description={t("my_notes.description")} />
      <Card>
        <CardContent className="flex flex-col gap-3 pt-6">
          <Textarea rows={14} value={body} onChange={(e) => setBody(e.target.value)} maxLength={20000} aria-label={t("my_notes.title")} />
          <div className="flex justify-end"><Button onClick={() => save.mutate()} disabled={save.isPending || !q.data}>{t("common.save")}</Button></div>
        </CardContent>
      </Card>
    </div>
  );
}

interface RulesResp { items: NotificationRule[]; events: string[]; critical?: string[]; providers: string[] }

function RuleDialog({ events, critical = [], rule, onDone }: { events: string[]; critical?: string[]; rule?: NotificationRule; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(rule?.name ?? "");
  const [provider, setProvider] = useState<string>(rule?.provider ?? "slack");
  const [url, setUrl] = useState("");
  const [target, setTarget] = useState(rule?.target ?? "");
  const [sel, setSel] = useState<string[]>(rule?.events ?? critical);
  const [err, setErr] = useState<string | null>(null);
  const [secret, setSecret] = useState<string | null>(null);
  const pf = providerFields(provider);
  const create = useMutation({
    mutationFn: () => rule
      ? api<{ id: string; secret: string | null }>(`/admin/notification-rules/${rule.id}`, { method: "PUT", body: { name, provider, url: pf.url ? url : "", target: pf.target ? target : "", events: sel, enabled: rule.enabled } }).then(() => ({ id: rule.id, secret: null }))
      : api<{ id: string; secret: string | null }>("/admin/notification-rules", { method: "POST", body: { name, provider, url: pf.url ? url : "", target: pf.target ? target : "", events: sel, enabled: true } }),
    onSuccess: (r) => { if (!rule) { setName(""); setUrl(""); setTarget(""); setSel(critical); } setErr(null); onDone(); if (r.secret) setSecret(r.secret); else setOpen(false); },
    onError: (e) => setErr(e instanceof ApiError ? e.code : "error.internal"),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) setSecret(null); }}>
      <DialogTrigger asChild>{rule ? <Button variant="outline" size="sm">{t("common.edit")}</Button> : <Button><PlusIcon />{t("notify.add")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{rule ? rule.name : t("notify.add")}</DialogTitle></DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-2"><Label htmlFor="nr-name">{t("notify.name")}</Label><Input id="nr-name" value={name} onChange={(e) => setName(e.target.value)} /></div>
          <div className="flex flex-col gap-2">
            <Label>{t("notify.provider")}</Label>
            <Select value={provider} onValueChange={setProvider} disabled={!!rule}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>{NOTIFY_PROVIDERS.map((p) => <SelectItem key={p} value={p}>{tCode(`notify.provider.${p}`)}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          {secret && <Alert><AlertTitle>{t("notify.secret_once")}</AlertTitle><AlertDescription><code className="break-all font-mono text-xs">{secret}</code></AlertDescription></Alert>}
          {pf.url && <div className="flex flex-col gap-2"><Label htmlFor="nr-url">{provider === "telegram" ? t("notify.url_telegram") : provider === "sms_webhook" ? t("notify.url_sms") : t("notify.url")}</Label><Input id="nr-url" type="url" placeholder={rule ? `${rule.url_masked ?? "https://"} (${t("notify.keep_url")})` : "https://"} value={url} onChange={(e) => setUrl(e.target.value)} /></div>}
          {pf.target && <div className="flex flex-col gap-2"><Label htmlFor="nr-target">{tCode(`notify.target_${provider}`)}</Label><Input id="nr-target" value={target} onChange={(e) => setTarget(e.target.value)} /></div>}
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <Label>{t("notify.events")}</Label>
              <div className="flex gap-1">
                {critical.length > 0 && <Button type="button" variant="outline" size="sm" data-testid="notify-critical" onClick={() => setSel(critical)}>{t("notify.preset_critical")}</Button>}
                <Button type="button" variant="ghost" size="sm" onClick={() => setSel(events)}>{t("notify.preset_all")}</Button>
                <Button type="button" variant="ghost" size="sm" onClick={() => setSel([])}>{t("notify.preset_none")}</Button>
              </div>
            </div>
            <div className="grid gap-2 sm:grid-cols-2">
              {events.map((ev) => (
                <label key={ev} className="flex min-h-11 items-center gap-2 text-sm md:min-h-0">
                  <Checkbox checked={sel.includes(ev)} onCheckedChange={(c) => setSel((s) => (c ? [...s, ev] : s.filter((x) => x !== ev)))} />
                  {tCode(`notify.event.${ev}`)}
                  {critical.includes(ev) && <Badge variant="destructive" className="ml-auto text-[10px]">{t("notify.critical")}</Badge>}
                </label>
              ))}
            </div>
          </div>
          <FormError message={err ? tCode(err) : null} />
        </div>
        <DialogFooter><Button onClick={() => create.mutate()} disabled={create.isPending}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Staff notifications to Slack / Discord / Microsoft Teams / generic webhook. */
export function AdminNotificationsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "notification-rules"], queryFn: () => api<RulesResp>("/admin/notification-rules") });
  const refresh = () => qc.invalidateQueries({ queryKey: ["admin", "notification-rules"] });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/notification-rules/${id}`, { method: "DELETE" }), onSuccess: refresh });
  const test = useMutation({ mutationFn: (id: string) => api(`/admin/notification-rules/${id}/test`, { method: "POST" }), onSuccess: () => { toast.success(t("notify.test_queued")); refresh(); }, onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)) });
  const toggle = useMutation({
    mutationFn: (r: NotificationRule) => api(`/admin/notification-rules/${r.id}`, { method: "PUT", body: { name: r.name, provider: r.provider, url: "", target: r.target ?? "", events: r.events, enabled: !r.enabled } }),
    onSuccess: refresh,
    onError: () => toast.error(t("notify.toggle_needs_url")),
  });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("notify.title")} description={t("notify.description")} actions={q.data && <RuleDialog events={q.data.events} critical={q.data.critical} onDone={refresh} />} />
      {!q.data ? <Skeleton className="h-32" /> : q.data.items.length === 0 ? <EmptyState title={t("notify.empty")} /> : (
        <div className="grid gap-3">
          {q.data.items.map((r) => (
            <Card key={r.id}>
              <CardContent className="flex flex-col gap-3 pt-6 sm:flex-row sm:items-center">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2 font-medium">{r.name}<Badge variant="secondary">{tCode(`notify.provider.${r.provider}`)}</Badge>{r.signed && <Badge variant="outline">{t("notify.signed")}</Badge>}</div>
                  <div className="truncate text-sm text-muted-foreground">{[r.url_masked, r.target].filter(Boolean).join(" · ")}</div>
                  {r.stats && <div className="text-xs text-muted-foreground">{t("notify.status.delivered")}: {r.stats.delivered} · {t("notify.status.failed")}: {r.stats.failed} · {t("notify.status.pending")}: {r.stats.pending}</div>}
                  <div className="mt-2 flex flex-wrap gap-1">{r.events.map((e) => <Badge key={e} variant="outline">{tCode(`notify.event.${e}`)}</Badge>)}</div>
                </div>
                <div className="flex items-center gap-2">
                  <Switch checked={r.enabled} disabled={toggle.isPending} onCheckedChange={() => toggle.mutate(r)} aria-label={t("notify.enabled")} />
                  {q.data && <RuleDialog key={`${r.id}-${r.events.join()}`} events={q.data.events} critical={q.data.critical} rule={r} onDone={refresh} />}
                  <Button variant="outline" size="sm" onClick={() => test.mutate(r.id)}>{t("notify.test")}</Button>
                  <DeliveriesDialog rule={r} />
                  <Button variant="ghost" size="icon" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => del.mutate(r.id)}><TrashIcon /></Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

/** Compact dashboard widget: overall health with a link to the full page. */
export function HealthWidget() {
  const { t, tCode } = useI18n();
  const q = useQuery({ queryKey: ["admin", "system-health"], queryFn: () => api<SystemHealth>("/admin/system-health"), retry: false });
  const s = q.data ? overallStatus(q.data.checks) : null;
  return (
    <Card className="h-full">
      <CardHeader><CardTitle>{t("health.title")}</CardTitle></CardHeader>
      <CardContent className="flex flex-col gap-2">
        {!s ? <Skeleton className="h-10" /> : <div className="flex items-center gap-2"><StatusIcon s={s} /><span>{tCode(`health.overall.${s}`)}</span></div>}
        <Button asChild variant="link" className="self-start px-0"><Link to="/admin/system-health">{t("common.view_all")}</Link></Button>
      </CardContent>
    </Card>
  );
}

/** Per-rule delivery log, with secret rotation for signed webhooks. */
function DeliveriesDialog({ rule }: { rule: NotificationRule }) {
  const { t, tCode, lang } = useI18n();
  const [open, setOpen] = useState(false);
  const [secret, setSecret] = useState<string | null>(null);
  const q = useQuery({ enabled: open, queryKey: ["admin", "notification-deliveries", rule.id], queryFn: () => api<{ items: NotificationDelivery[] }>(`/admin/notification-rules/${rule.id}/deliveries`), refetchInterval: open ? 5000 : false });
  const rotate = useMutation({ mutationFn: () => api<{ secret: string }>(`/admin/notification-rules/${rule.id}/secret`, { method: "POST" }), onSuccess: (r) => setSecret(r.secret) });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) setSecret(null); }}>
      <DialogTrigger asChild><Button variant="outline" size="sm">{t("notify.deliveries")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("notify.deliveries")} · {rule.name}</DialogTitle></DialogHeader>
        {rule.provider === "webhook" && (
          <div className="flex flex-col gap-2">
            <Button variant="outline" size="sm" className="self-start" disabled={rotate.isPending} onClick={() => rotate.mutate()}>{t("notify.rotate")}</Button>
            {secret && <Alert><AlertTitle>{t("notify.secret_once")}</AlertTitle><AlertDescription><code className="break-all font-mono text-xs">{secret}</code></AlertDescription></Alert>}
          </div>
        )}
        {!q.data ? <Skeleton className="h-24" /> : q.data.items.length === 0 ? <p className="text-sm text-muted-foreground">{t("notify.no_deliveries")}</p> : (
          <div className="grid gap-2">
            {q.data.items.map((d) => (
              <div key={d.id} className="flex flex-col gap-1 rounded-md border p-3 text-sm sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2"><Badge variant={d.status === "failed" ? "destructive" : d.status === "delivered" ? "secondary" : "outline"}>{tCode(`notify.status.${d.status}`)}</Badge><span>{tCode(`notify.event.${d.event}`)}</span></div>
                  {d.last_error && <div className="truncate text-xs text-muted-foreground">{d.last_error}</div>}
                </div>
                <div className="text-xs text-muted-foreground">{t("notify.attempts")}: {d.attempts}{d.response_status ? ` · HTTP ${d.response_status}` : ""} · {formatDateTime(d.created_at, lang)}</div>
              </div>
            ))}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
