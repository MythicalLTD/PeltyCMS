import { useState } from "react";
import { Link } from "react-router";
import { PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ListCard } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import {
  blankSlaPolicy, minutes, minutesInput, minutesUntil, PRIORITIES, slaPolicyValid, slaSpecificity, slaStateVariant,
  type Department, type SlaAtRisk, type SlaPolicy, type SlaPolicyDraft, type SlaTarget, type Staff, type TicketSlaInfo,
} from "@/lib/support/support";

const NONE = "none";
const opt = (v: string | null) => v ?? NONE;
const unopt = (v: string) => (v === NONE ? null : v);
const POLICIES = ["admin", "ticket-sla-policies"];

function useLists() {
  const deps = useQuery({ queryKey: ["admin", "ticket-departments"], queryFn: () => api<{ items: Department[] }>("/admin/ticket-departments") }).data?.items ?? [];
  const staff = useQuery({ queryKey: ["admin", "ticket-staff"], queryFn: () => api<{ items: Staff[] }>("/admin/ticket-staff") }).data?.items ?? [];
  return { deps, staff };
}

function PolicyDialog({ initial, trigger }: { initial?: SlaPolicy; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { deps, staff } = useLists();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<SlaPolicyDraft>(initial ?? blankSlaPolicy);
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/ticket-sla/policies/${initial.id}` : "/admin/ticket-sla/policies", { method: initial ? "PUT" : "POST", body: d }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ predicate: (q) => String(q.queryKey[1]).startsWith("ticket-") }); },
    onError: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const set = <K extends keyof SlaPolicyDraft>(k: K, v: SlaPolicyDraft[K]) => setD((x) => ({ ...x, [k]: v }));
  const pick = (k: "department_id" | "escalate_department" | "priority" | "escalate_priority" | "escalate_to", any: string) => {
    const items = k === "priority" || k === "escalate_priority"
      ? PRIORITIES.map((p) => ({ id: p, name: tCode(`tickets.priority.${p}`) }))
      : k === "escalate_to" ? staff.map((u) => ({ id: u.id, name: u.display_name || u.email })) : deps;
    return (
      <Select value={opt(d[k])} onValueChange={(v) => set(k, unopt(v))}>
        <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
        <SelectContent><SelectItem value={NONE}>{any}</SelectItem>{items.map((i) => <SelectItem key={i.id} value={i.id}>{i.name}</SelectItem>)}</SelectContent>
      </Select>
    );
  };
  const valid = slaPolicyValid(d);
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setError(null); setD(initial ?? blankSlaPolicy); } }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{initial ? t("sla.policy_edit") : t("sla.policy_new")}</DialogTitle></DialogHeader>
        <form id="sla-form" className="grid gap-4" onSubmit={(e) => { e.preventDefault(); if (valid) save.mutate(); }}>
          <FormError message={error} />
          <p className="text-sm font-medium">{t("sla.applies_to")}</p>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("tickets.department")}</Label>{pick("department_id", t("support.all_departments"))}</div>
            <div className="grid gap-2"><Label>{t("tickets.priority_label")}</Label>{pick("priority", t("support.any_priority"))}</div>
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div className="grid gap-2"><Label htmlFor="sla-fr">{t("sla.first_response_min")}</Label>
              <Input id="sla-fr" type="number" min={1} value={d.first_response_minutes ?? ""} onChange={(e) => set("first_response_minutes", minutesInput(e.target.value))} /></div>
            <div className="grid gap-2"><Label htmlFor="sla-res">{t("sla.resolution_min")}</Label>
              <Input id="sla-res" type="number" min={1} value={d.resolution_minutes ?? ""} onChange={(e) => set("resolution_minutes", minutesInput(e.target.value))} /></div>
            <div className="grid gap-2"><Label htmlFor="sla-warn">{t("sla.warn_percent")}</Label>
              <Input id="sla-warn" type="number" min={1} max={99} value={d.warn_percent} onChange={(e) => set("warn_percent", Number(e.target.value))} /></div>
          </div>
          <p className="text-xs text-muted-foreground">{t("sla.targets_hint")}</p>
          <p className="text-sm font-medium">{t("sla.on_breach")}</p>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div className="grid gap-2"><Label>{t("support.set_priority")}</Label>{pick("escalate_priority", t("support.keep"))}</div>
            <div className="grid gap-2"><Label>{t("tickets.assignee")}</Label>{pick("escalate_to", t("support.keep"))}</div>
            <div className="grid gap-2"><Label>{t("support.move_department")}</Label>{pick("escalate_department", t("support.keep"))}</div>
          </div>
        </form>
        <DialogFooter><Button type="submit" form="sla-form" disabled={!valid || save.isPending}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function SlaPoliciesCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { deps, staff } = useLists();
  const { data, isLoading } = useQuery({ queryKey: POLICIES, queryFn: () => api<{ items: SlaPolicy[] }>("/admin/ticket-sla/policies") });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/ticket-sla/policies/${id}`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: POLICIES }) });
  const dep = (id: string | null) => (id ? deps.find((x) => x.id === id)?.name ?? "-" : t("support.all_departments"));
  const rows = (data?.items ?? []).toSorted((a, b) => slaSpecificity(b) - slaSpecificity(a) || dep(a.department_id).localeCompare(dep(b.department_id)));
  const escalation = (p: SlaPolicy) => [
    p.escalate_priority && tCode(`tickets.priority.${p.escalate_priority}`),
    p.escalate_to && (staff.find((u) => u.id === p.escalate_to)?.display_name || t("tickets.assignee")),
    p.escalate_department && dep(p.escalate_department),
  ].filter(Boolean).join(" · ") || "-";
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("sla.policies")}</CardTitle><CardDescription>{t("sla.policies_hint")}</CardDescription>
          <CardAction><PolicyDialog trigger={<Button size="sm"><PlusIcon aria-hidden />{t("sla.policy_new")}</Button>} /></CardAction></CardHeader>
      </Card>
      <ListCard<SlaPolicy> rows={rows} rowKey={(p) => p.id} loading={isLoading} emptyTitle={t("sla.no_policies")} columns={[
        { header: t("tickets.department"), primary: true, cell: (p) => <span className="font-medium">{dep(p.department_id)}</span> },
        { header: t("tickets.priority_label"), cell: (p) => (p.priority ? tCode(`tickets.priority.${p.priority}`) : t("support.any_priority")) },
        { header: t("sla.first_response"), cell: (p) => minutes(p.first_response_minutes) },
        { header: t("sla.resolution"), cell: (p) => minutes(p.resolution_minutes) },
        { header: t("sla.warn_at"), cell: (p) => `${p.warn_percent}%` },
        { header: t("sla.on_breach"), cell: escalation },
        { header: "", cell: (p) => (
          <div className="flex justify-end gap-1">
            <PolicyDialog initial={p} trigger={<Button size="icon" variant="ghost" aria-label={t("common.edit")}><PencilSimpleIcon aria-hidden /></Button>} />
            <ConfirmAction description={t("sla.policy_delete")} onConfirm={() => del.mutate(p.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>
        ) },
      ]} />
    </div>
  );
}

function Due({ due }: { due: string | null }) {
  const { t } = useI18n();
  if (!due) return <>-</>;
  const m = minutesUntil(due);
  return <span className={m < 0 ? "text-destructive" : undefined}>{m < 0 ? t("sla.overdue_by", { time: minutes(-m) }) : t("sla.due_in", { time: minutes(m) })}</span>;
}

export function SlaAtRiskCard() {
  const { t, tCode } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "ticket-sla-at-risk"], queryFn: () => api<{ items: SlaAtRisk[] }>("/admin/ticket-sla/at-risk"), refetchInterval: 60_000 });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("sla.at_risk")}</CardTitle><CardDescription>{t("sla.at_risk_hint")}</CardDescription></CardHeader>
      </Card>
      <ListCard<SlaAtRisk> rows={data?.items} rowKey={(r) => r.ticket_id} loading={isLoading} emptyTitle={t("sla.none_at_risk")} columns={[
        { header: t("tickets.subject"), primary: true, cell: (r) => <Link className="font-medium underline-offset-4 hover:underline" to={`/admin/tickets/${r.ticket_id}`}>#{r.number} {r.subject}</Link> },
        { header: t("tickets.priority_label"), cell: (r) => tCode(`tickets.priority.${r.priority}`) },
        { header: t("sla.first_response"), cell: (r) => <Due due={r.fr_due} /> },
        { header: t("sla.resolution"), cell: (r) => <Due due={r.res_due} /> },
        { header: t("billing.status"), cell: (r) => <Badge variant={slaStateVariant(r.state)}>{tCode(`sla.state.${r.state}`)}</Badge> },
      ]} />
    </div>
  );
}

function TargetBadge({ label, target }: { label: string; target: SlaTarget | null }) {
  const { tCode } = useI18n();
  if (!target) return null;
  const open = target.state === "ok" || target.state === "warning" || target.state === "breached";
  return (
    <Badge variant={slaStateVariant(target.state)} title={new Date(target.due_at).toLocaleString()}>
      {label}: {tCode(`sla.state.${target.state}`)}{open && target.state !== "breached" ? ` (${minutes(Math.max(0, minutesUntil(target.due_at)))})` : ""}
    </Badge>
  );
}

/** SLA badges for the ticket header; renders nothing when no policy applies. */
export function TicketSlaBadges({ ticketId, version }: { ticketId: string; version?: unknown }) {
  const { t } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "ticket-sla", ticketId, version], queryFn: () => api<TicketSlaInfo>(`/admin/tickets/${ticketId}/sla`), refetchInterval: 60_000 });
  if (!data?.policy_id) return null;
  return (
    <>
      <TargetBadge label={t("sla.first_response")} target={data.first_response} />
      <TargetBadge label={t("sla.resolution")} target={data.resolution} />
      {data.paused && <Badge variant="outline">{t("sla.paused")}</Badge>}
    </>
  );
}
