import { Fragment, useState } from "react";
import { ArrowsClockwiseIcon, PulseIcon, WarningIcon, ListBulletsIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardAction, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { ListCard } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { PageHeader } from "@/components/PageHeader";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

interface Health { server: { id: string; name: string; module: string; enabled: boolean; max_services: number | null; active: number; suspended: number; failed: number; last_check_ok: boolean | null; last_check_error: string; last_checked_at: string | null }; usage_percent: number | null }
interface LogRow { id: number; service_id: string | null; module: string; action: string; source: string; ok: boolean; error: string; duration_ms: number; created_at: string }
interface Failed { service_id: string; product_name: string; customer_email: string; module: string | null; provision_error: string; status: string }

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => (e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

/** Per-server health: last connection test and fill level. */
export function ServerHealthCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "servers", "health"];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<{ data: Health[] }>("/admin/servers/health") });
  const check = useMutation({ mutationFn: () => api("/admin/servers/health/check", { method: "POST" }), onSuccess: () => void qc.invalidateQueries({ queryKey: key }) });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader>
          <CardTitle>{t("modx.health")}</CardTitle>
          <CardAction><Button size="sm" variant="outline" disabled={check.isPending} onClick={() => check.mutate()}><PulseIcon aria-hidden />{t("modx.check_all")}</Button></CardAction>
        </CardHeader>
      </Card>
      <ListCard<Health>
        rows={data?.data} rowKey={(h) => h.server.id} loading={isLoading}
        columns={[
          { header: t("servers.name"), primary: true, cell: (h) => <span className="font-medium">{h.server.name}</span> },
          { header: t("modx.state"), cell: (h) => h.server.last_check_ok === null ? <Badge variant="outline">{t("modx.never_checked")}</Badge> : h.server.last_check_ok ? <Badge>{t("modx.online")}</Badge> : <Badge variant="destructive" className="h-auto max-w-full shrink whitespace-normal break-words text-left">{tCode(h.server.last_check_error || "error.server_unreachable")}</Badge> },
          { header: t("modx.usage"), cell: (h) => <span className="tabular-nums">{h.server.active + h.server.suspended} / {h.server.max_services ?? "∞"}{h.usage_percent !== null ? ` · ${h.usage_percent}%` : ""}</span> },
          { header: t("modx.failed"), cell: (h) => <span className="tabular-nums">{h.server.failed}</span> },
          { header: t("modx.checked_at"), cell: (h) => (h.server.last_checked_at ? formatDate(h.server.last_checked_at, lang) : "-") },
        ]}
      />
    </div>
  );
}

/** Failed provisioning (dead jobs) with one-click retry. */
export function ModuleQueueCard() {
  const { t } = useI18n();
  const errOf = useErr();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const key = ["admin", "module-queue"];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<{ data: Failed[] }>("/admin/module-queue") });
  const [filter, setFilter] = useState("");
  const f = filter.trim().toLowerCase();
  const rows = data?.data.filter((x) => !f || [x.module ?? "", x.product_name, x.customer_email, x.provision_error].some((v) => v.toLowerCase().includes(f)));
  const retry = useMutation({ mutationFn: (id: string) => api(`/admin/module-queue/${id}/retry`, { method: "POST" }), onSuccess: () => { setError(null); void qc.invalidateQueries({ queryKey: key }); }, onError: (e) => setError(errOf(e)) });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader>
          <CardTitle>{t("modx.queue")}</CardTitle>
          <CardAction><Input className="h-8 w-32 sm:w-56" placeholder={t("modx.filter")} aria-label={t("modx.filter")} value={filter} onChange={(e) => setFilter(e.target.value)} /></CardAction>
        </CardHeader>
        <CardContent><FormError message={error} /></CardContent>
      </Card>
      <ListCard<Failed>
        rows={rows} rowKey={(f) => f.service_id} loading={isLoading} emptyIcon={WarningIcon} emptyTitle={t("modx.queue_empty")}
        columns={[
          { header: t("modx.service"), primary: true, cell: (f) => <><div className="font-medium">{f.product_name}</div><div className="text-xs text-muted-foreground">{f.customer_email}</div></> },
          { header: t("servers.module"), cell: (f) => <Badge variant="outline">{f.module ?? "-"}</Badge> },
          { header: t("provisioning.last_error"), cell: (f) => <span className="break-words font-mono text-xs text-destructive">{f.provision_error}</span> },
          { header: "", cell: (f) => <div className="flex justify-end"><Button size="sm" variant="outline" disabled={retry.isPending} onClick={() => retry.mutate(f.service_id)}><ArrowsClockwiseIcon aria-hidden />{t("modx.retry")}</Button></div> },
        ]}
      />
    </div>
  );
}

/** Module call log (secrets redacted server-side). */
export function ModuleLogCard({ serviceId }: { serviceId?: string }) {
  const { t, lang } = useI18n();
  const [errors, setErrors] = useState(false);
  const [filter, setFilter] = useState("");
  const [limit, setLimit] = useState(50);
  const q = new URLSearchParams({ limit: String(limit), ...(errors ? { errors: "true" } : {}), ...(serviceId ? { service_id: serviceId } : {}) });
  const { data, isLoading } = useQuery({ queryKey: ["admin", "module-log", serviceId ?? "", errors, limit], queryFn: () => api<{ data: LogRow[] }>(`/admin/module-log?${q}`) });
  const f = filter.trim().toLowerCase();
  const rows = data?.data.filter((r) => !f || [r.module, r.action, r.source, r.error, r.service_id ?? ""].some((v) => v.toLowerCase().includes(f)));
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader>
          <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between"><CardTitle>{t("modx.log")}</CardTitle>
          <div className="flex flex-wrap items-center gap-2"><Input className="h-8 w-full sm:w-56" placeholder={t("modx.filter")} aria-label={t("modx.filter")} value={filter} onChange={(e) => setFilter(e.target.value)} /><label className="flex items-center gap-2 text-sm"><Checkbox checked={errors} onCheckedChange={(c) => setErrors(!!c)} />{t("modx.only_errors")}</label></div></div>
        </CardHeader>
      </Card>
      <ListCard<LogRow>
        rows={rows} rowKey={(r) => String(r.id)} loading={isLoading} emptyIcon={ListBulletsIcon} emptyTitle={t("modx.log_empty")}
        columns={[
          { header: t("modx.when"), primary: true, cell: (r) => formatDate(r.created_at, lang) },
          { header: t("servers.module"), cell: (r) => <span className="font-mono text-xs">{r.module} · {r.action}</span> },
          { header: t("modx.source"), cell: (r) => <Badge variant="outline">{r.source}</Badge> },
          { header: t("billing.status"), cell: (r) => <Badge variant={r.ok ? "default" : "destructive"}>{r.ok ? "ok" : t("modx.error")}</Badge> },
          { header: t("modx.details"), cell: (r) => <span className="break-words font-mono text-xs text-muted-foreground">{r.error || `${r.duration_ms} ms`}</span> },
        ]}
      />
      {data && data.data.length >= limit && limit < 500 && <Button size="sm" variant="outline" className="self-start" onClick={() => setLimit((l) => Math.min(l + 100, 500))}>{t("modx.load_more")}</Button>}
    </div>
  );
}

/** Admin custom module buttons (module-declared actions) + live usage. */
export function ServiceCustomActions({ serviceId }: { serviceId: string }) {
  const { t, tCode } = useI18n();
  const errOf = useErr();
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [pw, setPw] = useState("");
  const { data } = useQuery({ queryKey: ["admin", "services", serviceId, "module-usage"], queryFn: () => api<{ module: string | null; actions: string[]; usage: Record<string, unknown> | null }>(`/admin/services/${serviceId}/module/usage`) });
  const run = useMutation({
    mutationFn: (action: string) => api<{ result: Record<string, unknown> }>(`/admin/services/${serviceId}/module/custom`, { method: "POST", body: { action, params: action === "change_password" && pw ? { password: pw } : {} } }),
    onSuccess: (r) => {
      const url = typeof r.result?.url === "string" ? r.result.url : null;
      setMsg({ ok: true, text: url ?? t("modx.action_done") });
    },
    onError: (e) => setMsg({ ok: false, text: errOf(e) }),
  });
  if (!data || data.actions.length === 0) return null;
  return (
    <CardContent className="grid gap-2 px-0">
      <p className="text-sm font-medium">{t("modx.custom_actions")}</p>
      {msg && (msg.ok ? <p role="status" className="break-all text-sm text-muted-foreground">{msg.text}</p> : <FormError message={msg.text} />)}
      {data.actions.includes("change_password") && <Input type="password" autoComplete="new-password" placeholder={t("modx.new_password_optional")} value={pw} onChange={(e) => setPw(e.target.value)} />}
      <div className="flex flex-wrap gap-1">
        {data.actions.map((a) => <Button key={a} size="sm" variant="outline" disabled={run.isPending} onClick={() => run.mutate(a)}>{tCode(`modx.action.${a}`)}</Button>)}
      </div>
      {data.usage && <UsageList usage={data.usage} />}
    </CardContent>
  );
}

export function UsageList({ usage }: { usage: Record<string, unknown> }) {
  return (
    <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
      {Object.entries(usage).filter(([, v]) => typeof v !== "object").map(([k, v]) => <Fragment key={k}><dt className="text-muted-foreground">{k}</dt><dd className="tabular-nums">{String(v)}</dd></Fragment>)}
    </dl>
  );
}

/** Standalone admin page: failed module actions with retry. */
export function AdminModuleQueuePage() {
  const { t } = useI18n();
  return <div className="flex flex-col gap-6"><PageHeader title={t("nav.module_queue")} description={t("modx.queue_desc")} /><ModuleQueueCard /></div>;
}

/** Standalone admin page: module call log. */
export function AdminModuleLogPage() {
  const { t } = useI18n();
  return <div className="flex flex-col gap-6"><PageHeader title={t("nav.module_log")} description={t("modx.log_desc")} /><ModuleLogCard /></div>;
}
