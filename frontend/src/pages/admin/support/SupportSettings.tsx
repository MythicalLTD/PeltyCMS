import { useState } from "react";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { PlayIcon, PlusIcon, StarIcon, TrashIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { formatDateTime } from "@/lib/format";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ContactTab, MailboxesTab, SpamTab } from "@/pages/admin/support/SupportExt";
import { useTicketStatuses } from "@/components/support/TicketStatuses";
import { SlaAtRiskCard, SlaPoliciesCard } from "@/pages/admin/support/TicketSla";
import { MacrosTab } from "@/pages/admin/support/TicketMacros";
import { minutes, PRIORITIES, slaPercent, statusSlugValid, distribution, type BadgeVariant, type CsatGroup, type CsatReport, type TicketStatusRow, type Canned, type Department, type Rule, type SlaRow, type Staff } from "@/lib/support/support";

const NONE = "none";
const opt = (v: string | null) => v ?? NONE;
const unopt = (v: string) => (v === NONE ? null : v);

function useErr() {
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return { error, setError, onError: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) };
}

function useDeps() {
  return useQuery({ queryKey: ["admin", "ticket-departments"], queryFn: () => api<{ items: Department[] }>("/admin/ticket-departments") }).data?.items ?? [];
}

function DeptSelect({ value, onChange, anyLabel }: { value: string | null; onChange: (v: string | null) => void; anyLabel: string }) {
  const deps = useDeps();
  return (
    <Select value={opt(value)} onValueChange={(v) => onChange(unopt(v))}>
      <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
      <SelectContent><SelectItem value={NONE}>{anyLabel}</SelectItem>{deps.map((d) => <SelectItem key={d.id} value={d.id}>{d.name}</SelectItem>)}</SelectContent>
    </Select>
  );
}

function CannedDialog({ initial, onDone, trigger }: { initial?: Canned; onDone: () => void; trigger: React.ReactNode }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const blank = { title: "", body: "", department_id: null as string | null };
  const [d, setD] = useState(initial ?? blank);
  const { error, setError, onError } = useErr();
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/ticket-canned/${initial.id}` : "/admin/ticket-canned", { method: initial ? "PUT" : "POST", body: d }),
    onSuccess: () => { setOpen(false); setError(null); onDone(); }, onError,
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) setD(initial ?? blank); }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{initial ? t("support.canned_edit") : t("support.canned_new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="cn-t">{t("support.title_field")}</Label><Input id="cn-t" required maxLength={150} value={d.title} onChange={(e) => setD({ ...d, title: e.target.value })} /></div>
          <div className="grid gap-2"><Label>{t("tickets.department")}</Label><DeptSelect value={d.department_id} onChange={(v) => setD({ ...d, department_id: v })} anyLabel={t("support.all_departments")} /></div>
          <div className="grid gap-2"><Label htmlFor="cn-b">{t("tickets.message")}</Label><Textarea id="cn-b" required rows={6} value={d.body} onChange={(e) => setD({ ...d, body: e.target.value })} /></div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CannedTab() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const deps = useDeps();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "ticket-canned", "all"], queryFn: () => api<{ items: Canned[] }>("/admin/ticket-canned") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "ticket-canned"] });
  const { error, onError } = useErr();
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/ticket-canned/${id}`, { method: "DELETE" }), onSuccess: refresh, onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("support.canned")}</CardTitle><CardDescription>{t("support.canned_hint")}</CardDescription>
          <CardAction><CannedDialog onDone={refresh} trigger={<Button size="sm"><PlusIcon aria-hidden />{t("support.canned_new")}</Button>} /></CardAction></CardHeader>
        <CardContent><FormError message={error} /></CardContent>
      </Card>
      <ListCard<Canned> rows={data?.items} rowKey={(c) => c.id} loading={isLoading} columns={[
        { header: t("support.title_field"), primary: true, cell: (c) => <span className="font-medium">{c.title}</span> },
        { header: t("tickets.department"), cell: (c) => deps.find((d) => d.id === c.department_id)?.name ?? t("support.all_departments") },
        { header: "", cell: (c) => (
          <div className="flex justify-end gap-1">
            <CannedDialog initial={c} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
            <ConfirmAction description={t("support.canned_delete_confirm")} onConfirm={() => del.mutate(c.id)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>) },
      ]} />
    </div>
  );
}

type RuleDraft = Omit<Rule, "id">;
const blankRule: RuleDraft = { name: "", department_id: null, statuses: ["open", "customer_reply"], priority: null, after_minutes: 60, set_priority: "high", set_department: null, assign_to: null, add_tag: null, is_active: true };

function RuleDialog({ initial, onDone, trigger }: { initial?: Rule; onDone: () => void; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<RuleDraft>(initial ?? blankRule);
  const { error, setError, onError } = useErr();
  const { data: staff } = useQuery({ queryKey: ["admin", "ticket-staff"], queryFn: () => api<{ items: Staff[] }>("/admin/ticket-staff"), enabled: open });
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/ticket-escalations/${initial.id}` : "/admin/ticket-escalations", { method: initial ? "PUT" : "POST", body: d }),
    onSuccess: () => { setOpen(false); setError(null); onDone(); }, onError,
  });
  const set = <K extends keyof RuleDraft>(k: K, v: RuleDraft[K]) => setD((x) => ({ ...x, [k]: v }));
  const prio = (k: "priority" | "set_priority", any: string) => (
    <Select value={opt(d[k])} onValueChange={(v) => set(k, unopt(v))}>
      <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
      <SelectContent><SelectItem value={NONE}>{any}</SelectItem>{PRIORITIES.map((p) => <SelectItem key={p} value={p}>{tCode(`tickets.priority.${p}`)}</SelectItem>)}</SelectContent>
    </Select>
  );
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) setD(initial ?? blankRule); }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{initial ? t("support.rule_edit") : t("support.rule_new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="r-n">{t("support.rule_name")}</Label><Input id="r-n" required value={d.name} onChange={(e) => set("name", e.target.value)} /></div>
          <p className="text-sm font-medium">{t("support.rule_when")}</p>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("tickets.department")}</Label><DeptSelect value={d.department_id} onChange={(v) => set("department_id", v)} anyLabel={t("support.all_departments")} /></div>
            <div className="grid gap-2"><Label>{t("tickets.priority_label")}</Label>{prio("priority", t("support.any_priority"))}</div>
          </div>
          <div className="grid gap-2"><Label htmlFor="r-m">{t("support.rule_after")}</Label>
            <Input id="r-m" type="number" min={1} required value={d.after_minutes} onChange={(e) => set("after_minutes", Number(e.target.value))} /></div>
          <p className="text-sm font-medium">{t("support.rule_then")}</p>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("support.set_priority")}</Label>{prio("set_priority", t("support.keep"))}</div>
            <div className="grid gap-2"><Label>{t("support.move_department")}</Label><DeptSelect value={d.set_department} onChange={(v) => set("set_department", v)} anyLabel={t("support.keep")} /></div>
            <div className="grid gap-2"><Label>{t("tickets.assignee")}</Label>
              <Select value={opt(d.assign_to)} onValueChange={(v) => set("assign_to", unopt(v))}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value={NONE}>{t("support.keep")}</SelectItem>{(staff?.items ?? []).map((u) => <SelectItem key={u.id} value={u.id}>{u.display_name || u.email}</SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label htmlFor="r-tag">{t("support.add_tag")}</Label><Input id="r-tag" value={d.add_tag ?? ""} onChange={(e) => set("add_tag", e.target.value || null)} /></div>
          </div>
          <div className="flex items-center justify-between rounded-md border p-3"><Label htmlFor="r-act">{t("coupons.active")}</Label><Switch id="r-act" checked={d.is_active} onCheckedChange={(v) => set("is_active", v)} /></div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function RulesTab() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [ran, setRan] = useState(false);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "ticket-escalations"], queryFn: () => api<{ items: Rule[] }>("/admin/ticket-escalations") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "ticket-escalations"] });
  const { error, onError } = useErr();
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/ticket-escalations/${id}`, { method: "DELETE" }), onSuccess: refresh, onError });
  const run = useMutation({ mutationFn: () => api("/admin/ticket-escalations/run", { method: "POST" }), onSuccess: () => setRan(true), onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("support.rules")}</CardTitle><CardDescription>{t("support.rules_hint")}</CardDescription>
          <CardAction className="flex gap-2">
            <Button size="sm" variant="outline" onClick={() => run.mutate()} disabled={run.isPending}><PlayIcon aria-hidden />{t("support.run_now")}</Button>
            <RuleDialog onDone={refresh} trigger={<Button size="sm"><PlusIcon aria-hidden />{t("support.rule_new")}</Button>} />
          </CardAction></CardHeader>
        <CardContent className="flex flex-col gap-3">
          <FormError message={error} />
          {ran && <p className="text-sm text-muted-foreground">{t("support.run_queued")}</p>}
        </CardContent>
      </Card>
      <ListCard<Rule> rows={data?.items} rowKey={(r) => r.id} loading={isLoading} columns={[
        { header: t("support.rule_name"), primary: true, cell: (r) => <span className="font-medium">{r.name}</span> },
        { header: t("support.rule_after"), cell: (r) => minutes(r.after_minutes) },
        { header: t("support.set_priority"), cell: (r) => (r.set_priority ? tCode(`tickets.priority.${r.set_priority}`) : "-") },
        { header: t("billing.status"), cell: (r) => <Badge variant={r.is_active ? "default" : "secondary"}>{r.is_active ? t("coupons.active") : t("coupons.inactive")}</Badge> },
        { header: "", cell: (r) => (
          <div className="flex justify-end gap-1">
            <RuleDialog initial={r} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
            <ConfirmAction description={t("support.rule_delete_confirm")} onConfirm={() => del.mutate(r.id)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>) },
      ]} />
    </div>
  );
}

interface DeptSla { id: string; email: string; sla_first_response_minutes: number | null }

function DeptSlaRow({ dep, cur, onDone }: { dep: SlaRow; cur?: DeptSla; onDone: () => void }) {
  const { t } = useI18n();
  const [email, setEmail] = useState(cur?.email ?? "");
  const [mins, setMins] = useState(cur?.sla_first_response_minutes?.toString() ?? "");
  const { error, onError } = useErr();
  const save = useMutation({
    mutationFn: () => api(`/admin/ticket-departments/${dep.department_id}/sla`, { method: "PUT", body: { email, sla_first_response_minutes: mins ? Number(mins) : null } }),
    onSuccess: onDone, onError,
  });
  return (
    <form className="grid gap-2 rounded-md border p-3 sm:grid-cols-[10rem_1fr_8rem_auto] sm:items-end" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <p className="font-medium">{dep.department_name}</p>
      <div className="grid gap-1"><Label className="text-xs">{t("support.pipe_email")}</Label><Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="support@example.com" /></div>
      <div className="grid gap-1"><Label className="text-xs">{t("support.sla_minutes")}</Label><Input type="number" min={1} value={mins} onChange={(e) => setMins(e.target.value)} /></div>
      <Button type="submit" size="sm" variant="outline" disabled={save.isPending}>{t("common.save")}</Button>
      {error && <div className="sm:col-span-4"><FormError message={error} /></div>}
    </form>
  );
}

function SlaTab() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const [days, setDays] = useState("30");
  const { data, isLoading } = useQuery({ queryKey: ["admin", "ticket-sla", days], queryFn: () => api<{ items: SlaRow[] }>(`/admin/ticket-sla?days=${days}`) });
  const { data: cfg } = useQuery({ queryKey: ["admin", "ticket-departments-sla"], queryFn: () => api<{ items: DeptSla[] }>("/admin/ticket-departments-sla") });
  const refresh = () => void qc.invalidateQueries({ predicate: (q) => String(q.queryKey[1]).startsWith("ticket-") });
  return (
    <div className="flex flex-col gap-4">
      <SlaAtRiskCard />
      <SlaPoliciesCard />
      <div className="flex flex-col gap-3">
        <Card>
          <CardHeader><CardTitle>{t("support.sla")}</CardTitle><CardDescription>{t("support.sla_hint")}</CardDescription>
            <CardAction>
              <Select value={days} onValueChange={setDays}>
                <SelectTrigger className="w-36"><SelectValue /></SelectTrigger>
                <SelectContent>{["7", "30", "90", "365"].map((d) => <SelectItem key={d} value={d}>{t("support.last_days", { days: d })}</SelectItem>)}</SelectContent>
              </Select>
            </CardAction></CardHeader>
        </Card>
        <ListCard<SlaRow> rows={data?.items} rowKey={(r) => r.department_id} loading={isLoading} columns={[
          { header: t("tickets.department"), primary: true, cell: (r) => <span className="font-medium">{r.department_name}</span> },
          { header: t("support.tickets"), cell: (r) => <span className="tabular-nums">{r.tickets}</span> },
          { header: t("support.avg_first_response"), cell: (r) => minutes(r.avg_first_response_minutes) },
          { header: t("support.within_sla"), cell: (r) => { const p = slaPercent(r); return p == null ? "-" : `${p}%`; } },
          { header: t("support.breached_open"), cell: (r) => (r.breached_open ? <Badge variant="destructive">{r.breached_open}</Badge> : "-") },
          { header: t("support.avg_rating"), cell: (r) => (r.avg_rating ? `${r.avg_rating} / 5 (${r.ratings})` : "-") },
        ]} />
      </div>
      <Card>
        <CardHeader><CardTitle>{t("support.departments")}</CardTitle><CardDescription>{t("support.departments_hint")}</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-2">
          {cfg && data?.items.map((d) => <DeptSlaRow key={d.department_id} dep={d} cur={cfg.items.find((c) => c.id === d.department_id)} onDone={refresh} />)}
        </CardContent>
      </Card>
    </div>
  );
}

const COLORS = ["default", "secondary", "destructive", "outline"] as const;
interface StatusDraft { slug: string; name: string; color: BadgeVariant; sort_order: number; awaiting_staff: boolean }

function StatusDialog({ initial, onDone, trigger }: { initial?: TicketStatusRow; onDone: () => void; trigger: React.ReactNode }) {
  const { t } = useI18n();
  const statuses = useTicketStatuses();
  const [open, setOpen] = useState(false);
  const blank: StatusDraft = { slug: "", name: "", color: "secondary", sort_order: 50, awaiting_staff: false };
  const [d, setD] = useState<StatusDraft>(blank);
  const { error, setError, onError } = useErr();
  const valid = (initial || statusSlugValid(d.slug)) && d.name.trim().length > 0 && d.name.trim().length <= 60;
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/ticket-statuses/${initial.slug}` : "/admin/ticket-statuses", { method: initial ? "PUT" : "POST", body: { ...d, name: d.name.trim() } }),
    onSuccess: () => { setOpen(false); onDone(); },
    onError,
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setError(null); setD(initial ? { ...initial, name: initial.name || statuses.label(initial.slug) } : blank); } }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{initial ? t("ticket_statuses.edit") : t("ticket_statuses.new")}</DialogTitle></DialogHeader>
        <form id="ts-form" className="grid gap-4" onSubmit={(e) => { e.preventDefault(); if (valid) save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="ts-name">{t("ticket_statuses.name")}</Label><Input id="ts-name" value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} /></div>
          {!initial && <div className="grid gap-2"><Label htmlFor="ts-slug">{t("ticket_statuses.slug")}</Label><Input id="ts-slug" className="font-mono" value={d.slug} onChange={(e) => setD({ ...d, slug: e.target.value.toLowerCase() })} /><p className="text-xs text-muted-foreground">{t("ticket_statuses.slug_hint")}</p></div>}
          <div className="grid grid-cols-2 gap-4">
            <div className="grid gap-2"><Label>{t("ticket_statuses.color")}</Label>
              <Select value={d.color} onValueChange={(v) => setD({ ...d, color: v as BadgeVariant })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{COLORS.map((c) => <SelectItem key={c} value={c}><Badge variant={c}>{d.name || t("ticket_statuses.preview")}</Badge></SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label htmlFor="ts-order">{t("ticket_statuses.order")}</Label><Input id="ts-order" type="number" value={d.sort_order} onChange={(e) => setD({ ...d, sort_order: Number(e.target.value) || 0 })} /></div>
          </div>
          {!initial?.builtin && <Label className="flex items-center gap-2"><Switch checked={d.awaiting_staff} onCheckedChange={(v) => setD({ ...d, awaiting_staff: v })} />{t("ticket_statuses.awaiting_staff")}</Label>}
        </form>
        <DialogFooter><Button type="submit" form="ts-form" disabled={!valid || save.isPending}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function StatusesTab() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const statuses = useTicketStatuses();
  const refresh = () => void qc.invalidateQueries({ queryKey: ["ticket-statuses"] });
  const { error, onError } = useErr();
  const del = useMutation({ mutationFn: (slug: string) => api(`/admin/ticket-statuses/${slug}`, { method: "DELETE" }), onSuccess: refresh, onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("ticket_statuses.title")}</CardTitle><CardDescription>{t("ticket_statuses.hint")}</CardDescription>
          <CardAction><StatusDialog onDone={refresh} trigger={<Button size="sm"><PlusIcon aria-hidden />{t("ticket_statuses.new")}</Button>} /></CardAction></CardHeader>
        <CardContent><FormError message={error} /></CardContent>
      </Card>
      <ListCard<TicketStatusRow> rows={statuses.items} rowKey={(r) => r.slug} columns={[
        { header: t("ticket_statuses.name"), primary: true, cell: (r) => <Badge variant={r.color}>{statuses.label(r.slug)}</Badge> },
        { header: t("ticket_statuses.slug"), cell: (r) => <span className="font-mono text-xs">{r.slug}</span> },
        { header: t("ticket_statuses.type"), cell: (r) => (r.builtin ? t("ticket_statuses.builtin") : t("ticket_statuses.custom")) },
        { header: t("ticket_statuses.awaiting_staff"), cell: (r) => (r.awaiting_staff ? t("ticket_statuses.yes") : t("ticket_statuses.no")) },
        { header: "", cell: (r) => (
          <div className="flex justify-end gap-1">
            <StatusDialog initial={r} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
            {!r.builtin && <ConfirmAction description={t("ticket_statuses.delete_confirm")} onConfirm={() => del.mutate(r.slug)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>}
          </div>) },
      ]} />
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return <div className="rounded-md border p-3"><p className="text-xs text-muted-foreground">{label}</p><p className="text-2xl font-semibold tabular-nums">{value}</p></div>;
}

function SatisfactionTab() {
  const { t, lang } = useI18n();
  const [days, setDays] = useState("30");
  const [dep, setDep] = useState<string | null>(null);
  const [low, setLow] = useState(false);
  const qs = new URLSearchParams({ days, ...(dep ? { department_id: dep } : {}), ...(low ? { low: "true" } : {}) });
  const { data, isLoading } = useQuery({ queryKey: ["admin", "ticket-satisfaction", days, dep, low], queryFn: () => api<{ report: CsatReport }>(`/admin/ticket-satisfaction?${qs}`) });
  const r = data?.report;
  const dash = (v: string | null, suffix = "") => (v == null ? "-" : `${v}${suffix}`);
  const groupCols = (first: string) => [
    { header: first, primary: true, cell: (g: CsatGroup) => <span className="font-medium">{g.name || t("csat.unassigned")}</span> },
    { header: t("csat.ratings"), cell: (g: CsatGroup) => g.ratings },
    { header: t("csat.avg"), cell: (g: CsatGroup) => dash(g.avg_rating) },
    { header: t("csat.csat"), cell: (g: CsatGroup) => dash(g.csat, "%") },
  ];
  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <Select value={days} onValueChange={setDays}>
          <SelectTrigger className="w-full" aria-label={t("csat.period")}><SelectValue /></SelectTrigger>
          <SelectContent>{["7", "30", "90", "365"].map((d) => <SelectItem key={d} value={d}>{t("csat.last_days", { n: Number(d) })}</SelectItem>)}</SelectContent>
        </Select>
        <DeptSelect value={dep} onChange={setDep} anyLabel={t("csat.all_departments")} />
        <Label className="flex h-9 items-center gap-2"><Switch checked={low} onCheckedChange={setLow} />{t("csat.low_only")}</Label>
      </div>
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label={t("csat.avg")} value={dash(r?.summary.avg_rating ?? null, " / 5")} />
        <Stat label={t("csat.csat")} value={dash(r?.summary.csat ?? null, "%")} />
        <Stat label={t("csat.ratings")} value={String(r?.summary.ratings ?? 0)} />
        <Stat label={t("csat.response_rate")} value={dash(r?.summary.rated_tickets_share ?? null, "%")} />
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader><CardTitle>{t("csat.distribution")}</CardTitle><CardDescription>{t("csat.hint")}</CardDescription></CardHeader>
          <CardContent className="grid gap-2">
            {r && distribution(r.summary).map(([stars, n, pct]) => (
              <div key={stars} className="grid grid-cols-[3rem_1fr_4rem] items-center gap-2 text-sm">
                <span className="flex items-center gap-1 tabular-nums">{stars}<StarIcon weight="fill" aria-hidden className="text-primary" /></span>
                <div className="h-2 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-primary" style={{ width: `${pct}%` }} /></div>
                <span className="text-right text-muted-foreground tabular-nums">{n} · {pct}%</span>
              </div>
            ))}
          </CardContent>
        </Card>
        <div className="flex min-w-0 flex-col gap-3">
          <Card><CardHeader><CardTitle>{t("csat.by_department")}</CardTitle></CardHeader></Card>
          <ListCard<CsatGroup> rows={r?.by_department} loading={isLoading} rowKey={(g) => g.id ?? "none"} columns={groupCols(t("tickets.department"))} />
        </div>
        <div className="flex min-w-0 flex-col gap-3">
          <Card><CardHeader><CardTitle>{t("csat.by_staff")}</CardTitle></CardHeader></Card>
          <ListCard<CsatGroup> rows={r?.by_staff} loading={isLoading} rowKey={(g) => g.id ?? "none"} columns={groupCols(t("csat.staff"))} />
        </div>
        <Card className="min-w-0">
          <CardHeader><CardTitle>{t("csat.comments")}</CardTitle></CardHeader>
          <CardContent className="grid gap-3">
            {r && r.comments.length === 0 && <p className="text-sm text-muted-foreground">{t("csat.no_comments")}</p>}
            {r?.comments.map((c) => (
              <div key={c.ticket_id} className="grid gap-1 border-b pb-3 text-sm last:border-0 last:pb-0">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <Link to={`/admin/tickets/${c.ticket_id}`} className="font-medium hover:underline">#{c.number} {c.subject}</Link>
                  <Badge variant={c.rating <= 2 ? "destructive" : c.rating >= 4 ? "default" : "secondary"}>{c.rating} / 5</Badge>
                </div>
                <p className="break-words whitespace-pre-wrap">{c.comment}</p>
                <p className="text-xs text-muted-foreground">{c.customer_name}{c.staff_name ? ` → ${c.staff_name}` : ""} · {formatDateTime(c.rated_at, lang)}</p>
              </div>
            ))}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

interface AutoClose { hours: number; statuses: string[] }

function AutoCloseCard() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const statuses = useTicketStatuses();
  const { error, setError, onError } = useErr();
  const { data } = useQuery({ queryKey: ["admin", "ticket-auto-close"], queryFn: () => api<AutoClose>("/admin/settings/ticket-auto-close") });
  const [draft, setDraft] = useState<AutoClose | null>(null);
  const cur = draft ?? data;
  const save = useMutation({
    mutationFn: (b: AutoClose) => api<AutoClose>("/admin/settings/ticket-auto-close", { method: "PUT", body: b }),
    onSuccess: () => { setError(null); setDraft(null); qc.invalidateQueries({ queryKey: ["admin", "ticket-auto-close"] }); },
    onError,
  });
  if (!cur) return null;
  const toggle = (slug: string, on: boolean) => setDraft({ ...cur, statuses: on ? [...cur.statuses, slug] : cur.statuses.filter((x) => x !== slug) });
  return (
    <Card>
      <CardHeader><CardTitle>{t("support.auto_close")}</CardTitle><CardDescription>{t("support.auto_close_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex max-w-xs flex-col gap-2">
          <Label htmlFor="ac-hours">{t("support.auto_close_hours")}</Label>
          <Input id="ac-hours" type="number" min={0} value={String(cur.hours)} onChange={(e) => setDraft({ ...cur, hours: Number(e.target.value) || 0 })} />
        </div>
        <div className="flex flex-col gap-2">
          <Label>{t("support.auto_close_statuses")}</Label>
          <div className="flex flex-wrap gap-4">
            {statuses.items.filter((x) => x.slug !== "closed").map((x) => (
              <label key={x.slug} className="flex items-center gap-2 text-sm">
                <Checkbox checked={cur.statuses.includes(x.slug)} onCheckedChange={(v) => toggle(x.slug, v === true)} />
                {statuses.label(x.slug)}
              </label>
            ))}
          </div>
        </div>
        <FormError message={error} />
        <div><Button disabled={!draft || save.isPending} onClick={() => save.mutate(cur)}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}

export function AdminSupportSettingsPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("tickets.settings")} />
      <Tabs defaultValue="sla">
        <TabsList className="w-full justify-start overflow-x-auto sm:w-auto">
          <TabsTrigger value="sla">{t("support.sla")}</TabsTrigger>
          <TabsTrigger value="canned">{t("support.canned")}</TabsTrigger>
          <TabsTrigger value="macros">{t("macros.title")}</TabsTrigger>
          <TabsTrigger value="rules">{t("support.rules")}</TabsTrigger>
          <TabsTrigger value="statuses">{t("ticket_statuses.title")}</TabsTrigger>
          <TabsTrigger value="csat">{t("csat.title")}</TabsTrigger>
          <TabsTrigger value="mailboxes">{t("support_ext.mailboxes")}</TabsTrigger>
          <TabsTrigger value="spam">{t("support_ext.spam")}</TabsTrigger>
          <TabsTrigger value="contact">{t("support_ext.contact_inbox")}</TabsTrigger>
        </TabsList>
        <TabsContent value="sla"><Replaceable section="tab-sla"><SlaTab /></Replaceable></TabsContent>
        <TabsContent value="canned"><Replaceable section="tab-canned"><CannedTab /></Replaceable></TabsContent>
        <TabsContent value="macros"><Replaceable section="tab-macros"><MacrosTab /></Replaceable></TabsContent>
        <TabsContent value="rules" className="flex flex-col gap-4"><Replaceable section="tab-rules"><RulesTab /><AutoCloseCard /></Replaceable></TabsContent>
        <TabsContent value="statuses"><Replaceable section="tab-statuses"><StatusesTab /></Replaceable></TabsContent>
        <TabsContent value="csat"><Replaceable section="tab-csat"><SatisfactionTab /></Replaceable></TabsContent>
        <TabsContent value="mailboxes"><Replaceable section="tab-mailboxes"><MailboxesTab /></Replaceable></TabsContent>
        <TabsContent value="spam"><Replaceable section="tab-spam"><SpamTab /></Replaceable></TabsContent>
        <TabsContent value="contact"><Replaceable section="tab-contact"><ContactTab /></Replaceable></TabsContent>
      </Tabs>
    </div>
  );
}
