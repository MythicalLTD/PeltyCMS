import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Input } from "@/components/ui/input";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { PageSkeleton } from "@/components/PageSkeleton";
import { ArrowCounterClockwiseIcon, ClockIcon, ListIcon, PlayIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import { formatDuration, healthVariant, intervalParts, iso, looksLikeCron, resultSummary, taskLabel, type Automation, type CronPreview, type SchedTask, type TaskRun } from "@/lib/admin/automation";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/** Scheduled tasks (built-in and addon `<addon>:<op>`): cron schedule with a
 * preview of the next runs, run now, on/off, last result and run history. */
export function AutomationPanel() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "automation", "tasks"], queryFn: () => api<{ tasks: SchedTask[]; timezone: string }>("/admin/automation/tasks"), refetchInterval: 30_000 });
  const totals = useQuery({ queryKey: ["admin", "automation"], queryFn: () => api<Automation>("/admin/automation"), refetchInterval: 30_000 });
  const [editing, setEditing] = useState<SchedTask | null>(null);
  const [historyOf, setHistoryOf] = useState<SchedTask | null>(null);
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "automation"] });
  const run = useMutation({
    mutationFn: (name: string) => api<{ duplicate: boolean }>(`/admin/automation/${encodeURIComponent(name)}/run`, { method: "POST" }),
    onSuccess: (r) => { toast.success(r.duplicate ? t("automation.already_queued") : t("automation.queued")); void qc.invalidateQueries({ queryKey: ["admin"] }); },
    onError: onErr,
  });
  const save = useMutation({
    mutationFn: (v: { name: string; enabled: boolean; run_hour: number | null }) =>
      api(`/admin/automation/${encodeURIComponent(v.name)}/settings`, { method: "PUT", body: { enabled: v.enabled, run_hour: v.run_hour } }),
    onSuccess: () => { toast.success(t("automation.saved")); refresh(); },
    onError: onErr,
  });
  const reset = useMutation({
    mutationFn: (name: string) => api(`/admin/automation/${encodeURIComponent(name)}/reset`, { method: "POST" }),
    onSuccess: () => { toast.success(t("automation.reset_done")); refresh(); },
    onError: onErr,
  });
  if (!data) return <PageSkeleton cards={4} />;
  return (
    <Card>
      <CardHeader className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-col gap-1">
          <CardTitle>{t("automation.title")}</CardTitle>
          <p className="text-xs text-muted-foreground">{t("automation.timezone", { tz: data.timezone })}</p>
        </div>
        <div className="flex flex-wrap gap-1">{Object.entries(totals.data?.totals ?? {}).map(([k, n]) => <Badge key={k} variant="outline">{tCode(`status.${k}`)}: {n}</Badge>)}</div>
      </CardHeader>
      <CardContent className="grid gap-3 *:min-w-0 sm:grid-cols-2 xl:grid-cols-3">
        {data.tasks.map((task) => {
          const [ik, n] = intervalParts(task.interval_secs);
          const label = task.source === "built-in" ? taskLabel(tCode, task.name) : task.name.slice(task.source.length + 1);
          return (
            <div key={task.name} className="flex min-w-0 flex-col gap-2 rounded-md border p-3 text-sm">
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <p className="font-medium">{label}</p>
                  <p className="truncate text-xs text-muted-foreground" title={task.name}>{task.schedule ? <span className="font-mono">{task.schedule}</span> : t(ik, { n })}</p>
                </div>
                {task.tripped_at ? <Badge variant="destructive">{t("automation.tripped")}</Badge> : <Badge variant={healthVariant(task.health)}>{tCode(`automation.health.${task.health}`)}</Badge>}
              </div>
              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
                <dt className="text-muted-foreground">{t("automation.last_run")}</dt>
                <dd>{task.last_finished_at ? `${formatDateTime(iso(task.last_finished_at), lang)} · ${task.last_status ? tCode(`status.${task.last_status}`) : ""} · ${formatDuration(task.last_duration_ms)}` : "-"}</dd>
                <dt className="text-muted-foreground">{t("automation.next_run")}</dt><dd>{task.next_run_at ? formatDateTime(iso(task.next_run_at), lang) : "-"}</dd>
                {resultSummary(task.last_result) && <><dt className="text-muted-foreground">{t("automation.result")}</dt><dd className="truncate font-mono">{resultSummary(task.last_result)}</dd></>}
              </dl>
              {task.tripped_at ? (
                <div role="alert" className="flex flex-col gap-2 rounded-md border border-destructive/50 p-2 text-xs text-destructive">
                  <p className="line-clamp-3">{t("automation.tripped_hint", { n: task.failures, error: task.tripped_error ?? "-" })}</p>
                  <Button size="sm" variant="outline" className="self-start" disabled={reset.isPending} onClick={() => reset.mutate(task.name)}><ArrowCounterClockwiseIcon aria-hidden />{t("automation.reset")}</Button>
                </div>
              ) : (
                <>
                  {task.failures > 0 && <p className="text-xs text-destructive">{t("automation.failures", { n: task.failures })}</p>}
                  {task.last_error && task.health === "failing" && <p className="line-clamp-2 text-xs text-destructive">{task.last_error}</p>}
                </>
              )}
              <div className="flex flex-wrap items-center gap-2">
                <Switch id={`en-${task.name}`} checked={task.enabled} disabled={save.isPending} onCheckedChange={(v) => save.mutate({ name: task.name, enabled: v, run_hour: task.run_hour })} />
                <Label htmlFor={`en-${task.name}`} className="text-xs">{t("automation.enabled")}</Label>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button size="sm" variant="outline" disabled={run.isPending} onClick={() => run.mutate(task.name)}><PlayIcon aria-hidden />{t("automation.run_now")}</Button>
                <Button size="sm" variant="outline" onClick={() => setEditing(task)}><ClockIcon aria-hidden />{t("automation.edit_schedule")}</Button>
                <Button size="sm" variant="ghost" onClick={() => setHistoryOf(task)}><ListIcon aria-hidden />{t("automation.history")}</Button>
              </div>
            </div>
          );
        })}
      </CardContent>
      {editing && <ScheduleDialog task={editing} timezone={data.timezone} onClose={() => { setEditing(null); refresh(); }} />}
      {historyOf && <HistoryDialog task={historyOf} onClose={() => setHistoryOf(null)} />}
    </Card>
  );
}

/** Edit a task's cron expression with a live preview of the next runs. */
function ScheduleDialog({ task, timezone, onClose }: { task: SchedTask; timezone: string; onClose: () => void }) {
  const { t, tCode, lang } = useI18n();
  const [expr, setExpr] = useState(task.schedule ?? task.default_schedule ?? "");
  const [debounced, setDebounced] = useState(expr);
  useEffect(() => { const h = setTimeout(() => setDebounced(expr), 300); return () => clearTimeout(h); }, [expr]);
  const preview = useQuery({
    queryKey: ["admin", "automation", "preview", debounced],
    queryFn: () => api<CronPreview>("/admin/automation/cron-preview", { method: "POST", body: { schedule: debounced } }),
    enabled: looksLikeCron(debounced),
    retry: false,
  });
  const put = useMutation({
    mutationFn: (schedule: string | null) => api(`/admin/automation/${encodeURIComponent(task.name)}/schedule`, { method: "PUT", body: { schedule } }),
    onSuccess: () => { toast.success(t("automation.saved")); onClose(); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const valid = looksLikeCron(expr) && preview.isSuccess && debounced === expr;
  const missed = useMutation({
    mutationFn: (m: string) => api(`/admin/automation/${encodeURIComponent(task.name)}/missed`, { method: "PUT", body: { missed: m } }),
    onSuccess: () => toast.success(t("automation.saved")),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("automation.edit_schedule")}: {task.source === "built-in" ? taskLabel(tCode, task.name) : task.name}</DialogTitle>
          <DialogDescription>{t("automation.schedule_hint", { tz: timezone })}</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <Label htmlFor="cron-expr">{t("automation.schedule")}</Label>
          <Input id="cron-expr" className="font-mono" value={expr} placeholder="0 3 * * *" onChange={(e) => setExpr(e.target.value)} aria-invalid={looksLikeCron(debounced) && preview.isError} />
          <div className="flex flex-wrap gap-1">
            {["*/5 * * * *", "0 * * * *", "0 3 * * *", "0 6 * * 1", "0 0 1 * *"].map((p) => <Button key={p} type="button" size="sm" variant="outline" className="font-mono" onClick={() => setExpr(p)}>{p}</Button>)}
          </div>
          {task.default_schedule && <p className="text-xs text-muted-foreground">{t("automation.default_schedule", { cron: task.default_schedule })}</p>}
          {preview.isError && looksLikeCron(debounced) ? <p className="text-sm text-destructive">{tCode("error.invalid_cron")}</p> : !looksLikeCron(expr) ? <p className="text-sm text-muted-foreground">{tCode("error.invalid_cron")}</p> : null}
          {preview.data && (
            <div className="rounded-md border p-2 text-sm">
              <p className="mb-1">{preview.data.description}</p>
              <p className="text-xs text-muted-foreground">{t("automation.next_runs")}</p>
              <ul className="text-xs">{preview.data.next_runs.map((r) => <li key={r}>{formatDateTime(iso(r), lang)}</li>)}</ul>
            </div>
          )}
          <Label htmlFor="cron-missed" className="mt-2">{t("automation.missed")}</Label>
          <Select defaultValue={task.missed} onValueChange={(v) => missed.mutate(v)}>
            <SelectTrigger id="cron-missed" className="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="skip">{t("automation.missed_skip")}</SelectItem>
              <SelectItem value="run_once">{t("automation.missed_run_once")}</SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">{t("automation.missed_hint")}</p>
        </div>
        <DialogFooter className="gap-2">
          {task.schedule && <Button variant="ghost" disabled={put.isPending} onClick={() => put.mutate(null)}>{t("automation.use_default")}</Button>}
          <Button disabled={!valid || put.isPending} onClick={() => put.mutate(expr.trim())}>{t("automation.save_schedule")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The last runs of one task. */
function HistoryDialog({ task, onClose }: { task: SchedTask; onClose: () => void }) {
  const { t, tCode, lang } = useI18n();
  const q = useQuery({ queryKey: ["admin", "automation", "history", task.name], queryFn: () => api<{ runs: TaskRun[] }>(`/admin/automation/${encodeURIComponent(task.name)}/history?limit=25`) });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("automation.history")}: {task.name}</DialogTitle>
          <DialogDescription>{t("automation.history_hint")}</DialogDescription>
        </DialogHeader>
        {!q.data ? <PageSkeleton cards={1} /> : q.data.runs.length === 0 ? <p className="text-sm text-muted-foreground">{t("automation.no_runs")}</p> : (
          <div className="max-h-96 overflow-auto">
            <ResponsiveTable<TaskRun> rows={q.data.runs} rowKey={(r) => r.id} columns={[
              { header: "#", cell: (r) => <span className="font-mono">{r.id}</span> },
              { header: t("automation.started"), primary: true, cell: (r) => formatDateTime(iso(r.created_at), lang) },
              { header: t("automation.status"), cell: (r) => <Badge variant={r.status === "succeeded" ? "default" : r.status === "failed" || r.status === "dead" ? "destructive" : "outline"}>{tCode(`status.${r.status}`)}</Badge> },
              { header: t("automation.duration"), cell: (r) => formatDuration(r.duration_ms) },
              { header: t("automation.result"), className: "font-mono break-all", cell: (r) => r.error ? <span className="text-destructive">{r.error}</span> : resultSummary(r.result) },
            ]} />
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

type Housekeeping = { prune_attachments_days: number; client_status_sync: boolean; ssl_monitoring: boolean; log_retention_days: number };

export function HousekeepingCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "housekeeping"], queryFn: () => api<Housekeeping>("/admin/settings/housekeeping") });
  const [v, setV] = useState<Housekeeping | null>(null);
  useEffect(() => { if (q.data) setV(q.data); }, [q.data]);
  const save = useMutation({
    mutationFn: (b: Housekeeping) => api<Housekeeping>("/admin/settings/housekeeping", { method: "PUT", body: b }),
    onSuccess: (d) => { qc.setQueryData(["admin", "housekeeping"], d); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!v) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("housekeeping.title")}</CardTitle></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(v); }}>
          <div className="flex max-w-xs flex-col gap-2">
            <Label htmlFor="hk-prune">{t("housekeeping.prune_attachments_days")}</Label>
            <Input id="hk-prune" type="number" min={0} max={3650} value={v.prune_attachments_days} onChange={(e) => setV({ ...v, prune_attachments_days: Number(e.target.value) || 0 })} />
            <p className="text-sm text-muted-foreground">{t("housekeeping.prune_hint")}</p>
          </div>
          <div className="flex max-w-xs flex-col gap-2">
            <Label htmlFor="hk-logs">{t("housekeeping.log_retention_days")}</Label>
            <Input id="hk-logs" type="number" min={0} max={3650} value={v.log_retention_days ?? 0} onChange={(e) => setV({ ...v, log_retention_days: Number(e.target.value) || 0 })} />
            <p className="text-sm text-muted-foreground">{t("housekeeping.log_retention_hint")}</p>
          </div>
          <div className="flex items-start gap-2">
            <Switch id="hk-sync" checked={v.client_status_sync} onCheckedChange={(c) => setV({ ...v, client_status_sync: c })} />
            <div className="flex flex-col gap-1"><Label htmlFor="hk-sync">{t("housekeeping.client_status_sync")}</Label><p className="text-sm text-muted-foreground">{t("housekeeping.client_status_hint")}</p></div>
          </div>
          <div className="flex items-start gap-2">
            <Switch id="hk-ssl" checked={v.ssl_monitoring} onCheckedChange={(c) => setV({ ...v, ssl_monitoring: c })} />
            <div className="flex flex-col gap-1"><Label htmlFor="hk-ssl">{t("housekeeping.ssl_monitoring")}</Label><p className="text-sm text-muted-foreground">{t("housekeeping.ssl_hint")}</p></div>
          </div>
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

type Inactive = { enabled: boolean; inactive_days: number; max_per_run: number };
type Candidate = { customer_id: string; email: string; last_activity: string };

/** Retention-based anonymisation of inactive clients, with a dry run. */
export function InactiveClientsCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "settings", "inactive"], queryFn: () => api<Inactive>("/admin/settings/inactive-clients") });
  const [v, setV] = useState<Inactive | null>(null);
  const [preview, setPreview] = useState<Candidate[] | null>(null);
  useEffect(() => { if (q.data) setV({ ...q.data, inactive_days: q.data.inactive_days || 730, max_per_run: q.data.max_per_run || 50 }); }, [q.data]);
  const err = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({
    mutationFn: (b: Inactive) => api<Inactive>("/admin/settings/inactive-clients", { method: "PUT", body: b }),
    onSuccess: (d) => { qc.setQueryData(["admin", "settings", "inactive"], d); toast.success(t("common.saved")); },
    onError: err,
  });
  const dry = useMutation({
    mutationFn: (days: number) => api<{ items: Candidate[] }>(`/admin/inactive-clients/preview?days=${days}`),
    onSuccess: (d) => setPreview(d.items),
    onError: err,
  });
  if (!v) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("inactive.title")}</CardTitle></CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(v); }}>
          <p className="text-sm text-muted-foreground">{t("inactive.hint")}</p>
          <div className="flex items-center gap-2">
            <Switch id="ic-on" checked={v.enabled} onCheckedChange={(c) => setV({ ...v, enabled: c })} />
            <Label htmlFor="ic-on">{t("inactive.enabled")}</Label>
          </div>
          <div className="grid max-w-md gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label htmlFor="ic-days">{t("inactive.days")}</Label>
              <Input id="ic-days" type="number" min={365} max={3650} value={v.inactive_days} onChange={(e) => setV({ ...v, inactive_days: Number(e.target.value) || 0 })} />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="ic-max">{t("inactive.max_per_run")}</Label>
              <Input id="ic-max" type="number" min={1} max={500} value={v.max_per_run} onChange={(e) => setV({ ...v, max_per_run: Number(e.target.value) || 0 })} />
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
            <Button type="button" variant="outline" disabled={dry.isPending} onClick={() => dry.mutate(v.inactive_days)}>{t("inactive.dry_run")}</Button>
          </div>
          {preview && (
            <div className="flex flex-col gap-1 text-sm">
              <p className="font-medium">{t("inactive.would_erase", { count: preview.length })}</p>
              {preview.slice(0, 20).map((c) => (
                <p key={c.customer_id} className="flex flex-wrap justify-between gap-2 text-muted-foreground"><span className="break-all">{c.email}</span><span>{formatDateTime(c.last_activity, lang)}</span></p>
              ))}
            </div>
          )}
        </form>
      </CardContent>
    </Card>
  );
}

/** WHMCS on-demand (early) renewals. */
export function OnDemandRenewalsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  type R = { enabled: boolean; max_days_ahead: number };
  const q = useQuery({ queryKey: ["admin", "settings", "odr"], queryFn: () => api<R>("/admin/settings/on-demand-renewals") });
  const [days, setDays] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (b: R) => api<R>("/admin/settings/on-demand-renewals", { method: "PUT", body: b }),
    onSuccess: (d) => { qc.setQueryData(["admin", "settings", "odr"], d); setDays(null); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data) return null;
  const d = q.data;
  return (
    <Card>
      <CardHeader><CardTitle>{t("renew.settings_title")}</CardTitle></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start gap-2">
          <Switch id="odr-on" checked={d.enabled} disabled={save.isPending} onCheckedChange={(c) => save.mutate({ ...d, enabled: c })} />
          <div className="flex flex-col gap-1"><Label htmlFor="odr-on">{t("renew.settings_enabled")}</Label><p className="text-sm text-muted-foreground">{t("renew.settings_hint")}</p></div>
        </div>
        <form className="flex flex-col gap-2 sm:flex-row sm:items-end" onSubmit={(e) => { e.preventDefault(); save.mutate({ ...d, max_days_ahead: Number(days ?? d.max_days_ahead) }); }}>
          <div className="flex flex-col gap-2"><Label htmlFor="odr-days">{t("renew.settings_days")}</Label>
            <Input id="odr-days" type="number" min={0} max={365} className="sm:w-32" value={days ?? String(d.max_days_ahead)} onChange={(e) => setDays(e.target.value)} /></div>
          <Button type="submit" variant="outline" className="min-h-11 sm:min-h-9" disabled={save.isPending || days === null}>{t("common.save")}</Button>
        </form>
      </CardContent>
    </Card>
  );
}

export function ReversalCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  type R = { reopen_invoice: boolean; suspend_services: boolean };
  const q = useQuery({ queryKey: ["admin", "settings", "reversal"], queryFn: () => api<R>("/admin/settings/reversal") });
  const save = useMutation({
    mutationFn: (b: R) => api<R>("/admin/settings/reversal", { method: "PUT", body: b }),
    onSuccess: (d) => { qc.setQueryData(["admin", "settings", "reversal"], d); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data) return null;
  const d = q.data;
  return (
    <Card>
      <CardHeader><CardTitle>{t("reversal.title")}</CardTitle></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start gap-2">
          <Switch id="rev-reopen" checked={d.reopen_invoice} disabled={save.isPending} onCheckedChange={(c) => save.mutate({ reopen_invoice: c, suspend_services: c && d.suspend_services })} />
          <div className="flex flex-col gap-1"><Label htmlFor="rev-reopen">{t("refunds.reopen")}</Label><p className="text-sm text-muted-foreground">{t("reversal.reopen_hint")}</p></div>
        </div>
        <div className="flex items-start gap-2">
          <Switch id="rev-suspend" checked={d.suspend_services} disabled={save.isPending || !d.reopen_invoice} onCheckedChange={(c) => save.mutate({ ...d, suspend_services: c })} />
          <Label htmlFor="rev-suspend">{t("refunds.suspend")}</Label>
        </div>
      </CardContent>
    </Card>
  );
}

/** WHMCS "Sequential Paid Invoice Numbering": unpaid invoices stay proforma. */
export function ProformaCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "proforma"], queryFn: () => api<{ enabled: boolean }>("/admin/settings/proforma") });
  const save = useMutation({
    mutationFn: (enabled: boolean) => api<{ enabled: boolean }>("/admin/settings/proforma", { method: "PUT", body: { enabled } }),
    onSuccess: (d) => { qc.setQueryData(["admin", "proforma"], d); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!q.data) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("proforma.title")}</CardTitle></CardHeader>
      <CardContent>
        <div className="flex items-start gap-2">
          <Switch id="proforma" checked={q.data.enabled} disabled={save.isPending} onCheckedChange={(c) => save.mutate(c)} />
          <div className="flex flex-col gap-1"><Label htmlFor="proforma">{t("proforma.enabled")}</Label><p className="text-sm text-muted-foreground">{t("proforma.hint")}</p></div>
        </div>
      </CardContent>
    </Card>
  );
}
