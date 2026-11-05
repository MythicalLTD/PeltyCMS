import { useState } from "react";
import { Link, useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KanbanIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { CustomerPicker } from "@/components/admin/CustomerPicker";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { duration, parseMinutes, progress, PROJECT_STATUSES, unbilledMinutes, type ProjectDetail, type ProjectStatus, type ProjectSummary } from "@/lib/support/projects";

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

export function ProjectStatusBadge({ status }: { status: ProjectStatus }) {
  const { t } = useI18n();
  const v = status === "active" ? "default" : status === "completed" ? "secondary" : status === "cancelled" ? "destructive" : "outline";
  return <Badge variant={v}>{t(`project.status.${status}`)}</Badge>;
}

export function AdminProjectsPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const q = useQuery({ queryKey: ["admin", "projects"], queryFn: () => api<{ items: ProjectSummary[] }>("/admin/projects") });
  const [title, setTitle] = useState("");
  const [customer, setCustomer] = useState("");
  const [rate, setRate] = useState("0.00");
  const create = useMutation({
    mutationFn: () => api<{ id: string }>("/admin/projects", { method: "POST", body: { title, customer_id: customer || null, hourly_rate: rate || "0" } }),
    onSuccess: () => { setTitle(""); setCustomer(""); qc.invalidateQueries({ queryKey: ["admin", "projects"] }); toast.success(t("common.saved")); },
    onError: err,
  });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("nav.projects")} description={t("navdesc.admin_projects")} />
      <Card>
        <CardHeader><CardTitle>{t("project.new")}</CardTitle></CardHeader>
        <CardContent>
          <form className="grid gap-4 md:grid-cols-[2fr_2fr_1fr_auto] md:items-end" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
            <div className="flex flex-col gap-2"><Label htmlFor="pj-title">{t("project.title")}</Label><Input id="pj-title" required maxLength={200} value={title} onChange={(e) => setTitle(e.target.value)} /></div>
            <div className="flex flex-col gap-2"><Label>{t("project.client")}</Label><CustomerPicker value={customer} onChange={setCustomer} /></div>
            <div className="flex flex-col gap-2"><Label htmlFor="pj-rate">{t("project.hourly_rate")}</Label><Input id="pj-rate" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} /></div>
            <Button type="submit" disabled={create.isPending || !title.trim()}><PlusIcon data-icon="inline-start" />{t("common.add")}</Button>
          </form>
        </CardContent>
      </Card>
      {q.isPending ? <Skeleton className="h-40" /> : !q.data?.items.length ? (
        <EmptyState icon={KanbanIcon} title={t("project.empty")} />
      ) : (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {q.data.items.map((p) => (
            <Card key={p.id}>
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <CardTitle className="min-w-0 break-words"><Link className="hover:underline" to={`/admin/projects/${p.id}`}>{p.title}</Link></CardTitle>
                <ProjectStatusBadge status={p.status} />
              </CardHeader>
              <CardContent className="flex flex-col gap-1 text-sm text-muted-foreground">
                <p className="break-all">{p.customer_email ?? t("project.no_client")}</p>
                <p>{t("project.tasks_progress", { done: p.tasks_done, total: p.tasks_total, pct: progress(p.tasks_done, p.tasks_total) })}</p>
                <p>{t("project.time_logged", { time: duration(p.minutes_total) })}</p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

export function AdminProjectDetailPage() {
  const { id = "" } = useParams();
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const key = ["admin", "projects", id];
  const q = useQuery({ queryKey: key, queryFn: () => api<ProjectDetail>(`/admin/projects/${id}`) });
  const refresh = () => qc.invalidateQueries({ queryKey: ["admin", "projects"] });
  const base = `/admin/projects/${id}`;
  const [task, setTask] = useState("");
  const [mins, setMins] = useState("");
  const [note, setNote] = useState("");
  const [billable, setBillable] = useState(true);
  const [linkKind, setLinkKind] = useState<"ticket" | "invoice">("ticket");
  const [linkId, setLinkId] = useState("");
  const save = useMutation({ mutationFn: (b: object) => api(base, { method: "PUT", body: b }), onSuccess: () => { refresh(); toast.success(t("common.saved")); }, onError: err });
  const addTask = useMutation({ mutationFn: () => api(`${base}/tasks`, { method: "POST", body: { title: task } }), onSuccess: () => { setTask(""); refresh(); }, onError: err });
  const toggle = useMutation({ mutationFn: (v: { id: string; done: boolean }) => api(`${base}/tasks/${v.id}`, { method: "PUT", body: { done: v.done } }), onSuccess: refresh, onError: err });
  const delTask = useMutation({ mutationFn: (tid: string) => api(`${base}/tasks/${tid}`, { method: "DELETE" }), onSuccess: refresh, onError: err });
  const addTime = useMutation({
    mutationFn: (m: number) => api(`${base}/time`, { method: "POST", body: { minutes: m, note, billable } }),
    onSuccess: () => { setMins(""); setNote(""); refresh(); }, onError: err,
  });
  const delTime = useMutation({ mutationFn: (eid: string) => api(`${base}/time/${eid}`, { method: "DELETE" }), onSuccess: refresh, onError: err });
  const bill = useMutation({ mutationFn: () => api<{ invoice_id: string }>(`${base}/invoice-time`, { method: "POST" }), onSuccess: () => { refresh(); toast.success(t("project.invoiced")); }, onError: err });
  const link = useMutation({ mutationFn: () => api(`${base}/links`, { method: "POST", body: { kind: linkKind, target_id: linkId.trim() } }), onSuccess: () => { setLinkId(""); refresh(); }, onError: err });
  if (q.isPending) return <Skeleton className="h-64" />;
  if (!q.data) return <EmptyState icon={KanbanIcon} title={t("error.not_found")} />;
  const { project: p, tasks, time, links } = q.data;
  const put = (patch: Partial<typeof p>) => { const n = { ...p, ...patch }; save.mutate({ title: n.title, customer_id: n.customer_id, description: n.description, status: n.status, due_date: n.due_date, client_visible: n.client_visible, hourly_rate: n.hourly_rate }); };
  const unbilled = unbilledMinutes(time);
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={p.title} description={p.customer_email ?? t("project.no_client")} actions={<ProjectStatusBadge status={p.status} />} />
      <Card>
        <CardHeader><CardTitle>{t("project.details")}</CardTitle></CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-4 sm:grid-cols-3">
            <div className="flex flex-col gap-2">
              <Label>{t("project.status_label")}</Label>
              <Select value={p.status} onValueChange={(v) => put({ status: v as ProjectStatus })}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>{PROJECT_STATUSES.map((s) => <SelectItem key={s} value={s}>{t(`project.status.${s}`)}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-2"><Label htmlFor="pj-due">{t("project.due_date")}</Label><Input id="pj-due" type="date" defaultValue={p.due_date ?? ""} onBlur={(e) => put({ due_date: e.target.value || null })} /></div>
            <div className="flex flex-col gap-2"><Label htmlFor="pj-r">{t("project.hourly_rate")}</Label><Input id="pj-r" inputMode="decimal" defaultValue={p.hourly_rate} onBlur={(e) => e.target.value !== p.hourly_rate && put({ hourly_rate: e.target.value })} /></div>
          </div>
          <div className="flex flex-col gap-2"><Label htmlFor="pj-desc">{t("project.description")}</Label><Textarea id="pj-desc" rows={3} defaultValue={p.description} onBlur={(e) => e.target.value !== p.description && put({ description: e.target.value })} /></div>
          <div className="flex items-center gap-2"><Switch id="pj-vis" checked={p.client_visible} disabled={!p.customer_id} onCheckedChange={(c) => put({ client_visible: c })} /><Label htmlFor="pj-vis">{t("project.client_visible")}</Label></div>
          <div><ConfirmAction description={t("project.delete_confirm")} onConfirm={() => api(base, { method: "DELETE" }).then(() => { refresh(); window.history.back(); }).catch(err)}><Button variant="destructive"><TrashIcon data-icon="inline-start" />{t("common.delete")}</Button></ConfirmAction></div>
        </CardContent>
      </Card>
      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader><CardTitle>{t("project.tasks")} ({progress(p.tasks_done, p.tasks_total)}%)</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-3">
            <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); addTask.mutate(); }}>
              <Input aria-label={t("project.new_task")} placeholder={t("project.new_task")} maxLength={300} value={task} onChange={(e) => setTask(e.target.value)} />
              <Button type="submit" disabled={!task.trim() || addTask.isPending}><PlusIcon data-icon="inline-start" />{t("common.add")}</Button>
            </form>
            {tasks.map((x) => (
              <div key={x.id} className="flex items-center gap-2">
                <Checkbox id={`tk-${x.id}`} checked={x.done} onCheckedChange={(c) => toggle.mutate({ id: x.id, done: c === true })} />
                <Label htmlFor={`tk-${x.id}`} className={x.done ? "flex-1 text-muted-foreground line-through" : "flex-1"}>{x.title}</Label>
                <Button variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => delTask.mutate(x.id)}><TrashIcon /></Button>
              </div>
            ))}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle>{t("project.time")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-3">
            <form className="grid gap-2 sm:grid-cols-[6rem_1fr_auto]" onSubmit={(e) => { e.preventDefault(); const m = parseMinutes(mins); if (m == null) toast.error(t("project.bad_duration")); else addTime.mutate(m); }}>
              <Input aria-label={t("project.duration")} placeholder="1:30" value={mins} onChange={(e) => setMins(e.target.value)} />
              <Input aria-label={t("project.note")} placeholder={t("project.note")} maxLength={500} value={note} onChange={(e) => setNote(e.target.value)} />
              <Button type="submit" disabled={!mins.trim() || addTime.isPending}>{t("project.log_time")}</Button>
              <div className="flex items-center gap-2 sm:col-span-3"><Switch id="pj-bill" checked={billable} onCheckedChange={setBillable} /><Label htmlFor="pj-bill">{t("project.billable")}</Label></div>
            </form>
            {time.map((e) => (
              <div key={e.id} className="flex flex-wrap items-center gap-2 text-sm">
                <span className="w-16 font-medium">{duration(e.minutes)}</span>
                <span className="text-muted-foreground">{e.spent_on}</span>
                <span className="min-w-0 flex-1 break-words">{e.note}</span>
                {e.invoice_id ? <Badge variant="secondary">{t("project.billed")}</Badge> : !e.billable ? <Badge variant="outline">{t("project.non_billable")}</Badge> : null}
                {!e.invoice_id && <Button variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => delTime.mutate(e.id)}><TrashIcon /></Button>}
              </div>
            ))}
            <div className="flex flex-wrap items-center justify-between gap-2 border-t pt-3 text-sm">
              <span>{t("project.unbilled", { time: duration(unbilled) })}</span>
              <Button variant="outline" disabled={!unbilled || !p.customer_id || bill.isPending} onClick={() => bill.mutate()}>{t("project.invoice_time")}</Button>
            </div>
          </CardContent>
        </Card>
      </div>
      <Card>
        <CardHeader><CardTitle>{t("project.links")}</CardTitle></CardHeader>
        <CardContent className="flex flex-col gap-3">
          <form className="grid gap-2 sm:grid-cols-[10rem_1fr_auto]" onSubmit={(e) => { e.preventDefault(); link.mutate(); }}>
            <Select value={linkKind} onValueChange={(v) => setLinkKind(v as "ticket" | "invoice")}>
              <SelectTrigger aria-label={t("project.link_kind")}><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="ticket">{t("project.link_ticket")}</SelectItem><SelectItem value="invoice">{t("project.link_invoice")}</SelectItem></SelectContent>
            </Select>
            <Input aria-label={t("project.link_id")} placeholder={t("project.link_id")} value={linkId} onChange={(e) => setLinkId(e.target.value)} />
            <Button type="submit" disabled={!linkId.trim() || link.isPending}>{t("project.link")}</Button>
          </form>
          {links.map((l) => (
            <Link key={`${l.kind}-${l.target_id}`} className="text-sm hover:underline" to={l.kind === "ticket" ? `/admin/tickets/${l.target_id}` : `/admin/invoices/${l.target_id}`}>
              {t(l.kind === "ticket" ? "project.link_ticket" : "project.link_invoice")}: {l.label ?? l.target_id}
            </Link>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
