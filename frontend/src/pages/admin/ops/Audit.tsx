import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { DownloadSimpleIcon, FunnelXIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { DataTable } from "@/components/DataTable";
import { api } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { auditLabel } from "@/lib/admin/audit";
import { useI18n } from "@/lib/i18n";

interface Entry { id: number; actor_id: string | null; actor_email: string | null; actor_name: string | null; action: string; target: string | null; ip: string | null; metadata?: Record<string, unknown>; created_at: string }

export interface AuditFilters { action: string; actor: string; ip: string; q: string; target: string; from: string; to: string; system: boolean }
export const EMPTY_FILTERS: AuditFilters = { action: "", actor: "", ip: "", q: "", target: "", from: "", to: "", system: false };

/** Query string for the filters (blank fields dropped). */
export function auditQuery(f: Partial<AuditFilters>): string {
  const q = new URLSearchParams();
  for (const k of ["action", "actor", "ip", "q", "target", "from", "to"] as const) {
    const v = f[k]?.trim();
    if (v) q.set(k, v);
  }
  if (f.system) q.set("system", "true");
  return q.toString();
}

/** Build the audit query path from the filter inputs (blank = no filter). */
export function auditPath(action: string, actor: string, more: Partial<AuditFilters> = {}) {
  const s = auditQuery({ ...more, action, actor });
  return s ? `/admin/audit-logs?${s}` : "/admin/audit-logs";
}

/** Debounce text filters so typing doesn't fire a query per key. */
function useDebounced<T>(v: T, ms = 300): T {
  const [d, setD] = useState(v);
  useEffect(() => {
    const id = setTimeout(() => setD(v), ms);
    return () => clearTimeout(id);
  }, [v, ms]);
  return d;
}

export function AdminAuditPage() {
  const { t, tCode, lang } = useI18n();
  const [f, setF] = useState<AuditFilters>(EMPTY_FILTERS);
  const [open, setOpen] = useState<Entry | null>(null);
  const df = useDebounced(f);
  const set = (k: keyof AuditFilters, v: string | boolean) => setF((p) => ({ ...p, [k]: v }));
  const actions = useQuery({ queryKey: ["admin", "audit", "actions"], queryFn: () => api<{ items: { action: string; count: number }[] }>("/admin/audit-logs/actions"), staleTime: 60_000 });
  const qs = auditQuery(df);
  const active = qs !== "";
  const prefixes = [...new Set((actions.data?.items ?? []).map((a) => `${a.action.split(".")[0]}.`))].toSorted();
  return (
    <>
      <DataTable<Entry>
        title={t("admin.audit.title")} description={t("admin.audit.description")}
        path={qs ? `/admin/audit-logs?${qs}` : "/admin/audit-logs"} queryKey={["admin", "audit", qs]}
        toolbar={
          <div className="flex w-full flex-col gap-2">
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-4">
              <div className="relative">
                <MagnifyingGlassIcon aria-hidden className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
                <Input className="pl-8" placeholder={t("admin.audit.filter_q")} aria-label={t("admin.audit.filter_q")} value={f.q} onChange={(e) => set("q", e.target.value)} />
              </div>
              <Input list="audit-actions" placeholder={t("admin.audit.filter_action")} aria-label={t("admin.audit.filter_action")} value={f.action} onChange={(e) => set("action", e.target.value)} />
              <datalist id="audit-actions">
                {prefixes.map((p) => <option key={p} value={p}>{p}</option>)}
                {(actions.data?.items ?? []).map((a) => <option key={a.action} value={a.action}>{auditLabel(tCode, a.action)} ({a.count})</option>)}
              </datalist>
              <Input placeholder={t("admin.audit.filter_actor")} aria-label={t("admin.audit.filter_actor")} value={f.actor} disabled={f.system} onChange={(e) => set("actor", e.target.value)} />
              <Input placeholder={t("admin.audit.filter_ip")} aria-label={t("admin.audit.filter_ip")} value={f.ip} onChange={(e) => set("ip", e.target.value)} className="font-mono" />
            </div>
            <div className="flex flex-wrap items-end gap-2">
              <div className="grid gap-1"><Label htmlFor="audit-from" className="text-xs">{t("admin.audit.from")}</Label><Input id="audit-from" type="date" className="w-40" value={f.from} max={f.to || undefined} onChange={(e) => set("from", e.target.value)} /></div>
              <div className="grid gap-1"><Label htmlFor="audit-to" className="text-xs">{t("admin.audit.to")}</Label><Input id="audit-to" type="date" className="w-40" value={f.to} min={f.from || undefined} onChange={(e) => set("to", e.target.value)} /></div>
              <Input placeholder={t("admin.audit.filter_target")} aria-label={t("admin.audit.filter_target")} value={f.target} onChange={(e) => set("target", e.target.value)} className="w-56 font-mono text-xs" />
              <label className="flex min-h-9 items-center gap-2 text-sm"><Checkbox checked={f.system} onCheckedChange={(v) => set("system", v === true)} />{t("admin.audit.system_only")}</label>
              <div className="ml-auto flex gap-2">
                {active && <Button type="button" variant="ghost" size="sm" onClick={() => setF(EMPTY_FILTERS)}><FunnelXIcon aria-hidden />{t("admin.audit.clear")}</Button>}
                <Button asChild variant="outline" size="sm"><a href={`/api/v1/admin/audit-logs/export.csv${qs ? `?${qs}` : ""}`} download><DownloadSimpleIcon aria-hidden />{t("admin.audit.export")}</a></Button>
              </div>
            </div>
          </div>
        }
        columns={[
          { header: t("admin.audit.action"), cell: (e) => (
            <button type="button" className="flex flex-col text-left hover:underline" onClick={() => setOpen(e)}>
              <span className="text-sm">{auditLabel(tCode, e.action)}</span>
              <span className="font-mono text-xs text-muted-foreground">{e.action}</span>
            </button>
          ), primary: true },
          { header: t("admin.audit.when"), cell: (e) => <span className="whitespace-nowrap text-muted-foreground">{formatDateTime(e.created_at, lang)}</span> },
          { header: t("admin.audit.actor"), cell: (e) => e.actor_email
            ? <button type="button" className="text-left hover:underline" title={e.actor_id ?? undefined} onClick={() => setF((p) => ({ ...p, actor: e.actor_email ?? "", system: false }))}>{e.actor_name ? <>{e.actor_name} <span className="text-xs text-muted-foreground">{e.actor_email}</span></> : e.actor_email}</button>
            : <span className="text-muted-foreground">{e.actor_id ? t("admin.audit.deleted_user") : t("admin.audit.system")}</span> },
          { header: t("admin.audit.target"), cell: (e) => e.target ? <button type="button" className="max-w-48 truncate font-mono text-xs hover:underline" title={e.target} onClick={() => set("target", e.target ?? "")}>{e.target}</button> : <span className="text-muted-foreground">-</span> },
          { header: t("admin.audit.ip"), cell: (e) => e.ip ? <button type="button" className="font-mono text-xs hover:underline" onClick={() => set("ip", e.ip ?? "")}>{e.ip}</button> : <span className="text-muted-foreground">-</span> },
        ]}
      />
      <Dialog open={open !== null} onOpenChange={(o) => { if (!o) setOpen(null); }}>
        <DialogContent className="max-w-2xl">
          {open && (
            <>
              <DialogHeader>
                <DialogTitle>{auditLabel(tCode, open.action)}</DialogTitle>
                <DialogDescription className="font-mono text-xs">{open.action} · #{open.id} · {formatDateTime(open.created_at, lang)}</DialogDescription>
              </DialogHeader>
              <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
                <dt className="text-muted-foreground">{t("admin.audit.actor")}</dt><dd className="break-all">{open.actor_email ?? (open.actor_id ? t("admin.audit.deleted_user") : t("admin.audit.system"))}{open.actor_id && <span className="ml-2 font-mono text-xs text-muted-foreground">{open.actor_id}</span>}</dd>
                <dt className="text-muted-foreground">{t("admin.audit.target")}</dt><dd className="break-all font-mono text-xs">{open.target ?? "-"}</dd>
                <dt className="text-muted-foreground">{t("admin.audit.ip")}</dt><dd className="font-mono text-xs">{open.ip ?? "-"}</dd>
              </dl>
              <div className="grid gap-1">
                <span className="text-sm text-muted-foreground">{t("admin.audit.details")}</span>
                <pre className="max-h-80 overflow-auto rounded-md bg-muted p-3 text-xs">{JSON.stringify(open.metadata ?? {}, null, 2)}</pre>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
