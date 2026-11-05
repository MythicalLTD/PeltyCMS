import { useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownIcon, ArrowUpIcon, CaretLeftIcon, CaretRightIcon, DatabaseIcon, DownloadSimpleIcon, GearSixIcon, LinkSimpleIcon, PaperPlaneTiltIcon, PlusIcon, TrashIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ListPage, ListCard } from "@/components/ListPage";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { formatBytes, linkOk, monthGrid, move, shiftMonth, toggle, WIDGETS, type Backup, type CalendarItem, type EmailLogEntry, type LoginEntry, type OnlineStaff, type Prefs, type QuickLink } from "@/lib/admin/adminOps";
import { formatDate, formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export function usePrefs() {
  return useQuery({ queryKey: ["admin", "prefs"], queryFn: () => api<{ prefs: Prefs }>("/admin/prefs").then((r) => r.prefs) });
}

const thisMonth = () => new Date().toISOString().slice(0, 7);

/** Dialog to choose, order and configure dashboard widgets and quick links. */
export function CustomizeDashboard({ prefs }: { prefs: Prefs }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [w, setW] = useState(prefs.widgets);
  const [links, setLinks] = useState<QuickLink[]>(prefs.quick_links);
  const [gate, setGate] = useState({ open, prefs });
  if (open !== gate.open || prefs !== gate.prefs) {
    setGate({ open, prefs });
    if (open) { setW(prefs.widgets); setLinks(prefs.quick_links); }
  }
  const save = useMutation({
    mutationFn: () => api("/admin/prefs", { method: "PUT", body: { widgets: w, quick_links: links } }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["admin", "prefs"] }); setOpen(false); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  const order = [...w, ...WIDGETS.filter((x) => !w.includes(x))];
  const valid = links.every((l) => l.label.trim() && linkOk(l.url));
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button variant="outline"><GearSixIcon aria-hidden />{t("ops.customize")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{t("ops.customize")}</DialogTitle><DialogDescription>{t("ops.customize_hint")}</DialogDescription></DialogHeader>
        <ul className="flex flex-col gap-1">
          {order.map((x) => {
            const i = w.indexOf(x);
            return (
              <li key={x} className="flex items-center gap-2 rounded-md border p-2">
                <Checkbox id={`w-${x}`} checked={i >= 0} onCheckedChange={() => setW(toggle(w, x))} />
                <Label htmlFor={`w-${x}`} className="flex-1">{tCode(`ops.widget.${x}`)}</Label>
                <Button size="icon" variant="ghost" className="size-11 md:size-9" disabled={i <= 0} aria-label={t("ops.up")} onClick={() => setW(move(w, i, -1))}><ArrowUpIcon /></Button>
                <Button size="icon" variant="ghost" className="size-11 md:size-9" disabled={i < 0 || i >= w.length - 1} aria-label={t("ops.down")} onClick={() => setW(move(w, i, 1))}><ArrowDownIcon /></Button>
              </li>
            );
          })}
        </ul>
        <div className="flex flex-col gap-2">
          <Label>{t("ops.quick_links")}</Label>
          {links.map((l, i) => (
            <div key={i} className="flex gap-2">
              <Input aria-label={t("ops.link_label")} placeholder={t("ops.link_label")} value={l.label} onChange={(e) => setLinks(links.map((x, j) => j === i ? { ...x, label: e.target.value } : x))} />
              <Input aria-label="URL" placeholder="/admin/billing" aria-invalid={!linkOk(l.url)} value={l.url} onChange={(e) => setLinks(links.map((x, j) => j === i ? { ...x, url: e.target.value } : x))} />
              <Button size="icon" variant="ghost" className="size-11 shrink-0 md:size-9" aria-label={t("common.delete")} onClick={() => setLinks(links.filter((_, j) => j !== i))}><TrashIcon /></Button>
            </div>
          ))}
          <Button variant="outline" size="sm" className="self-start" disabled={links.length >= 20} onClick={() => setLinks([...links, { label: "", url: "/admin/" }])}><PlusIcon />{t("ops.add_link")}</Button>
        </div>
        <DialogFooter><Button disabled={!valid || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function StaffOnlineCard() {
  const { t, lang } = useI18n();
  const q = useQuery({ queryKey: ["admin", "staff-online"], queryFn: () => api<{ items: OnlineStaff[] }>("/admin/staff-online"), refetchInterval: 60_000 });
  return (
    <Card className="h-full">
      <CardHeader><CardTitle>{t("ops.widget.staff_online")}</CardTitle><CardDescription>{t("ops.online_hint")}</CardDescription></CardHeader>
      <CardContent>
        {!q.data ? <Skeleton className="h-16" /> : q.data.items.length === 0 ? <EmptyState icon={UsersThreeIcon} title={t("common.no_data")} /> : (
          <ul className="flex flex-col gap-2">{q.data.items.map((s) => (
            <li key={s.user_id} className="flex items-center justify-between gap-2 text-sm"><span className="flex min-w-0 items-center gap-2"><span className="size-2 shrink-0 rounded-full bg-primary" aria-hidden /><span className="truncate">{s.display_name || s.email}</span></span><time className="shrink-0 text-xs text-muted-foreground">{formatDateTime(s.last_seen_at, lang)}</time></li>
          ))}</ul>
        )}
      </CardContent>
    </Card>
  );
}

export function QuickLinksCard({ links }: { links: QuickLink[] }) {
  const { t } = useI18n();
  return (
    <Card className="h-full">
      <CardHeader><CardTitle>{t("ops.quick_links")}</CardTitle></CardHeader>
      <CardContent>
        {links.length === 0 ? <EmptyState icon={LinkSimpleIcon} title={t("ops.no_links")} /> : (
          <div className="flex flex-wrap gap-2">{links.map((l, i) => l.url.startsWith("/")
            ? <Button key={i} asChild variant="outline" size="sm"><Link to={l.url}>{l.label}</Link></Button>
            : <Button key={i} asChild variant="outline" size="sm"><a href={l.url} target="_blank" rel="noopener noreferrer">{l.label}</a></Button>)}</div>
        )}
      </CardContent>
    </Card>
  );
}

export function TodosCard() {
  const { t, lang } = useI18n();
  const q = useQuery({ queryKey: ["admin", "todos", "pending"], queryFn: () => api<{ items: { id: string; title: string; status: string; due_date: string | null }[] }>("/admin/todos?status=pending") });
  return (
    <Card className="h-full">
      <CardHeader><CardTitle>{t("ops.widget.todos")}</CardTitle></CardHeader>
      <CardContent>
        {!q.data ? <Skeleton className="h-16" /> : q.data.items.length === 0 ? <p className="text-sm text-muted-foreground">{t("common.empty")}</p> : (
          <ul className="flex flex-col gap-2 text-sm">{q.data.items.slice(0, 6).map((x) => <li key={x.id} className="flex justify-between gap-2"><span className="truncate">{x.title}</span><span className="shrink-0 text-xs text-muted-foreground">{x.due_date ? formatDate(x.due_date, lang) : ""}</span></li>)}</ul>
        )}
        <Button asChild variant="link" className="px-0"><Link to="/admin/todos">{t("common.view_all")}</Link></Button>
      </CardContent>
    </Card>
  );
}

function useCalendar(month: string) {
  return useQuery({ queryKey: ["admin", "calendar", month], queryFn: () => api<{ items: CalendarItem[] }>(`/admin/calendar?month=${month}`) });
}

function itemLink(x: CalendarItem): string {
  if (x.kind === "todo") return "/admin/todos";
  if (x.kind === "invoice") return `/admin/invoices/${x.id}`;
  return x.customer_id ? `/admin/customers/${x.customer_id}` : "/admin";
}

export function UpcomingCard() {
  const { t, tCode, lang } = useI18n();
  const [today] = useState(() => new Date().toISOString().slice(0, 10));
  const q = useCalendar(thisMonth());
  const items = (q.data?.items ?? []).filter((x) => x.date >= today).slice(0, 8);
  return (
    <Card className="h-full">
      <CardHeader><CardTitle>{t("ops.widget.calendar")}</CardTitle></CardHeader>
      <CardContent>
        {!q.data ? <Skeleton className="h-16" /> : items.length === 0 ? <p className="text-sm text-muted-foreground">{t("common.empty")}</p> : (
          <ul className="flex flex-col gap-2 text-sm">{items.map((x) => <li key={x.kind + x.id} className="flex items-center justify-between gap-2"><Link to={itemLink(x)} className="flex min-w-0 items-center gap-2 hover:underline"><Badge variant="secondary">{tCode(`ops.kind.${x.kind}`)}</Badge><span className="truncate">{x.title}</span></Link><span className="shrink-0 text-xs text-muted-foreground">{formatDate(x.date, lang)}</span></li>)}</ul>
        )}
        <Button asChild variant="link" className="px-0"><Link to="/admin/calendar">{t("common.view_all")}</Link></Button>
      </CardContent>
    </Card>
  );
}

export function AdminCalendarPage() {
  const { t, tCode, lang } = useI18n();
  const [month, setMonth] = useState(thisMonth());
  const q = useCalendar(month);
  const byDay = new Map<string, CalendarItem[]>();
  for (const x of q.data?.items ?? []) byDay.set(x.date, [...(byDay.get(x.date) ?? []), x]);
  const label = new Intl.DateTimeFormat(lang, { month: "long", year: "numeric", timeZone: "UTC" }).format(new Date(`${month}-01T00:00:00Z`));
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("ops.calendar")} description={t("ops.calendar_hint")} />
      <div className="flex items-center gap-2">
        <Button size="icon" variant="outline" className="size-11 md:size-9" aria-label={t("ops.prev")} onClick={() => setMonth(shiftMonth(month, -1))}><CaretLeftIcon /></Button>
        <span className="min-w-40 text-center font-medium capitalize">{label}</span>
        <Button size="icon" variant="outline" className="size-11 md:size-9" aria-label={t("ops.next")} onClick={() => setMonth(shiftMonth(month, 1))}><CaretRightIcon /></Button>
      </div>
      {/* Desktop: month grid. Mobile: agenda list. */}
      <div className="hidden grid-cols-7 gap-1 md:grid">
        {monthGrid(month).map((d, i) => (
          <div key={d ?? `b${i}`} className={d ? "min-h-24 rounded-md border p-1 text-xs" : ""}>
            {d && <><div className="mb-1 text-muted-foreground">{Number(d.slice(8))}</div>
              {(byDay.get(d) ?? []).map((x) => <Link key={x.kind + x.id} to={itemLink(x)} className="mb-0.5 block truncate rounded bg-muted px-1 hover:underline" title={x.title}>{tCode(`ops.kind.${x.kind}`)}: {x.title}</Link>)}</>}
          </div>
        ))}
      </div>
      <ul className="flex flex-col gap-2 md:hidden">
        {(q.data?.items ?? []).length === 0 && <p className="text-sm text-muted-foreground">{t("common.empty")}</p>}
        {(q.data?.items ?? []).map((x) => <li key={x.kind + x.id}><Link to={itemLink(x)} className="flex items-center justify-between gap-2 rounded-md border p-3 text-sm"><span className="flex min-w-0 items-center gap-2"><Badge variant="secondary">{tCode(`ops.kind.${x.kind}`)}</Badge><span className="truncate">{x.title}</span></span><span className="shrink-0 text-xs text-muted-foreground">{formatDate(x.date, lang)}</span></Link></li>)}
      </ul>
    </div>
  );
}

export function AdminLoginHistoryPage() {
  const { t, tCode, lang } = useI18n();
  const q = useQuery({ queryKey: ["admin", "login-history"], queryFn: () => api<{ items: LoginEntry[] }>("/admin/login-history?limit=200") });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("ops.login_history")} description={t("ops.login_history_hint")} />
      <StaffOnlineCard />
      <ListCard rows={q.data?.items} loading={q.isLoading} rowKey={(x) => x.id}
        columns={[
          { header: t("ops.staff"), cell: (x) => x.display_name || x.email, primary: true },
          { header: t("client.date"), cell: (x) => formatDateTime(x.created_at, lang) },
          { header: t("ops.last_seen"), cell: (x) => formatDateTime(x.last_seen_at, lang) },
          { header: "IP", cell: (x) => x.ip ?? "-" },
          { header: t("ops.agent"), cell: (x) => <span className="line-clamp-1 max-w-64 text-xs text-muted-foreground" title={x.user_agent ?? ""}>{x.user_agent ?? "-"}</span> },
          { header: t("billing.status"), cell: (x) => <Badge variant={x.state === "active" ? "default" : "secondary"}>{tCode(`ops.state.${x.state}`)}</Badge> },
        ]} />
    </div>
  );
}

const backupMutationErr = (e: unknown) => toast.error(e instanceof ApiError ? e.message : String(e));

export function AdminBackupsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "backups"], queryFn: () => api<{ items: Backup[] }>("/admin/backups"), refetchInterval: (x) => x.state.data?.items.some((b) => b.status === "queued") ? 3000 : false });
  const inv = () => qc.invalidateQueries({ queryKey: ["admin", "backups"] });
  const create = useMutation({ mutationFn: () => api("/admin/backups", { method: "POST" }), onSuccess: () => { inv(); toast.success(t("ops.backup_queued")); }, onError: backupMutationErr });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/backups/${id}`, { method: "DELETE" }), onSuccess: inv, onError: backupMutationErr });
  return (
    <ListPage title={t("ops.backups")} description={t("ops.backups_hint")} actions={<Button disabled={create.isPending} onClick={() => create.mutate()}><DatabaseIcon aria-hidden />{t("ops.backup_now")}</Button>}
      rows={q.data?.items} loading={q.isLoading} rowKey={(x) => x.id} emptyIcon={DatabaseIcon} emptyTitle={t("ops.no_backups")}
      columns={[
        { header: t("ops.file"), cell: (x) => <span className="font-mono text-xs">{x.file_name}</span>, primary: true },
        { header: t("client.date"), cell: (x) => formatDateTime(x.created_at, lang) },
        { header: t("ops.size"), cell: (x) => formatBytes(x.size_bytes) },
        { header: t("billing.status"), cell: (x) => <span title={x.error ?? ""}><Badge variant={x.status === "failed" ? "destructive" : x.status === "done" ? "default" : "secondary"}>{tCode(`ops.backup_status.${x.status}`)}</Badge></span> },
        { header: "", cell: (x) => (
          <div className="flex justify-end gap-1">
            {x.status === "done" && <Button asChild size="icon" variant="ghost" className="size-11 md:size-9" aria-label={t("ops.download")}><a href={`/api/v1/admin/backups/${x.id}/download`} download><DownloadSimpleIcon /></a></Button>}
            <Button size="icon" variant="ghost" className="size-11 md:size-9" aria-label={t("common.delete")} onClick={() => { if (confirm(t("ops.delete_backup_confirm"))) del.mutate(x.id); }}><TrashIcon /></Button>
          </div>
        ) },
      ]}
    />
  );
}

/** Per-client email log (from the mail queue) with resend. */
export function ClientEmailLog({ customerId }: { customerId: string }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "customer-emails", customerId], queryFn: () => api<{ items: EmailLogEntry[] }>(`/admin/customers/${customerId}/emails`) });
  const resend = useMutation({
    mutationFn: (id: number) => api(`/admin/emails/${id}/resend`, { method: "POST" }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["admin", "customer-emails", customerId] }); toast.success(t("ops.resent")); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  return (
    <ResponsiveTable rows={q.data?.items} loading={q.isLoading} rowKey={(x) => x.id} columns={[
      { header: t("ops.template"), cell: (x) => tCode(`email.name.${x.template}`), primary: true },
      { header: t("client.date"), cell: (x) => formatDateTime(x.created_at, lang) },
      { header: t("billing.status"), cell: (x) => <span title={x.last_error ?? ""}><Badge variant={x.status === "dead" ? "destructive" : "secondary"}>{tCode(`ops.mail_status.${x.status}`)}</Badge></span> },
      { header: "", cell: (x) => <Button variant="outline" size="sm" className="h-11 md:h-8" disabled={resend.isPending} onClick={() => resend.mutate(x.id)}><PaperPlaneTiltIcon />{t("ops.resend")}</Button> },
    ]} />
  );
}
