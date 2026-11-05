import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, PlusIcon, PulseIcon, TrashIcon, WarningIcon, WrenchIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { FormError } from "@/components/Field";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "@/components/ui/alert-dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export const KINDS = ["outage", "degraded", "maintenance"] as const;
export const PRIORITIES = ["low", "medium", "high", "critical"] as const;
export const STATUSES = ["scheduled", "investigating", "identified", "monitoring", "resolved"] as const;
export interface IssueUpdate { id: string; status: string; message: string; created_at: string }
export interface NetworkIssue { id: string; title: string; description: string; component: string; kind: string; priority: string; status: string; starts_at: string; ends_at: string | null; created_at: string; updated_at: string; updates: IssueUpdate[] }

/** Split issues for display: active (not resolved, not future-scheduled), scheduled, resolved. */
export function groupIssues(items: NetworkIssue[], now = Date.now()) {
  const active = items.filter((i) => i.status !== "resolved" && !(i.status === "scheduled" && Date.parse(i.starts_at) > now));
  const scheduled = items.filter((i) => i.status === "scheduled" && Date.parse(i.starts_at) > now);
  const resolved = items.filter((i) => i.status === "resolved");
  return { active, scheduled, resolved };
}

function IssueCard({ i, actions }: { i: NetworkIssue; actions?: React.ReactNode }) {
  const { tCode, lang } = useI18n();
  const Icon = i.kind === "maintenance" ? WrenchIcon : i.status === "resolved" ? CheckCircleIcon : WarningIcon;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex flex-wrap items-center gap-2"><Icon aria-hidden className="size-5 text-muted-foreground" />{i.title}</CardTitle>
        <CardDescription className="flex flex-wrap items-center gap-2">
          <Badge variant={i.status === "resolved" ? "secondary" : "outline"}>{tCode(`netstatus.status.${i.status}`)}</Badge>
          <Badge variant="outline">{tCode(`netstatus.kind.${i.kind}`)}</Badge>
          {i.component && <span>{i.component}</span>}
          <span>{formatDateTime(i.starts_at, lang)}{i.ends_at ? `, ${formatDateTime(i.ends_at, lang)}` : ""}</span>
        </CardDescription>
        {actions && <CardAction>{actions}</CardAction>}
      </CardHeader>
      {(i.description || i.updates.length > 0) && (
        <CardContent className="grid gap-3 text-sm">
          {i.description && <p className="whitespace-pre-wrap">{i.description}</p>}
          {i.updates.length > 0 && (
            <ol className="grid gap-2 border-l pl-4">
              {i.updates.map((u) => (
                <li key={u.id} className="grid gap-0.5">
                  <span className="text-xs text-muted-foreground">{tCode(`netstatus.status.${u.status}`)} · {formatDateTime(u.created_at, lang)}</span>
                  <span className="whitespace-pre-wrap">{u.message}</span>
                </li>
              ))}
            </ol>
          )}
        </CardContent>
      )}
    </Card>
  );
}

export function NetworkStatusPage() {
  const { t } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["netstatus"], queryFn: () => api<{ operational: boolean; items: NetworkIssue[] }>("/public/network-status"), refetchInterval: 60_000 });
  const g = groupIssues(data?.items ?? []);
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("netstatus.title")} description={t("netstatus.subtitle")} />
      {isLoading ? <Skeleton className="h-16 w-full" /> : (
        <Alert variant={data?.operational ? "default" : "destructive"}>
          {data?.operational ? <CheckCircleIcon aria-hidden /> : <WarningIcon aria-hidden />}
          <AlertTitle>{data?.operational ? t("netstatus.all_ok") : t("netstatus.has_issues")}</AlertTitle>
          <AlertDescription>{t("netstatus.auto_refresh")}</AlertDescription>
        </Alert>
      )}
      {g.active.map((i) => <IssueCard key={i.id} i={i} />)}
      {g.scheduled.length > 0 && <h2 className="text-lg font-semibold">{t("netstatus.scheduled")}</h2>}
      {g.scheduled.map((i) => <IssueCard key={i.id} i={i} />)}
      {g.resolved.length > 0 && <h2 className="text-lg font-semibold">{t("netstatus.recent")}</h2>}
      {g.resolved.map((i) => <IssueCard key={i.id} i={i} />)}
    </div>
  );
}

const toLocalInput = (iso: string | null) => (iso ? new Date(new Date(iso).getTime() - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 16) : "");
const fromLocalInput = (v: string) => (v ? new Date(v).toISOString() : null);

function EnumSelect({ id, value, onChange, values, prefix }: { id: string; value: string; onChange: (v: string) => void; values: readonly string[]; prefix: string }) {
  const { tCode } = useI18n();
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger id={id} className="w-full"><SelectValue /></SelectTrigger>
      <SelectContent>{values.map((v) => <SelectItem key={v} value={v}>{tCode(`${prefix}.${v}`)}</SelectItem>)}</SelectContent>
    </Select>
  );
}

function IssueDialog({ issue, onDone }: { issue?: NetworkIssue; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const init = () => ({ title: issue?.title ?? "", description: issue?.description ?? "", component: issue?.component ?? "", kind: issue?.kind ?? "outage", priority: issue?.priority ?? "medium", status: issue?.status ?? "investigating", starts_at: toLocalInput(issue?.starts_at ?? null), ends_at: toLocalInput(issue?.ends_at ?? null) });
  const [f, setF] = useState(init);
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api(issue ? `/admin/network-issues/${issue.id}` : "/admin/network-issues", { method: issue ? "PUT" : "POST", body: { ...f, starts_at: fromLocalInput(f.starts_at), ends_at: fromLocalInput(f.ends_at) } }),
    onSuccess: () => { setOpen(false); setError(null); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) setF(init()); }}>
      <DialogTrigger asChild>{issue ? <Button size="sm" variant="outline">{t("common.edit")}</Button> : <Button size="sm"><PlusIcon aria-hidden />{t("netstatus.new")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{issue ? t("common.edit") : t("netstatus.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="ni-title">{t("netstatus.issue_title")}</Label><Input id="ni-title" required maxLength={200} value={f.title} onChange={(e) => setF({ ...f, title: e.target.value })} /></div>
          <div className="grid gap-2"><Label htmlFor="ni-comp">{t("netstatus.component")}</Label><Input id="ni-comp" maxLength={120} value={f.component} onChange={(e) => setF({ ...f, component: e.target.value })} /></div>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-3">
            <div className="grid gap-2"><Label htmlFor="ni-kind">{t("netstatus.kind")}</Label><EnumSelect id="ni-kind" value={f.kind} onChange={(v) => setF({ ...f, kind: v })} values={KINDS} prefix="netstatus.kind" /></div>
            <div className="grid gap-2"><Label htmlFor="ni-prio">{t("netstatus.priority")}</Label><EnumSelect id="ni-prio" value={f.priority} onChange={(v) => setF({ ...f, priority: v })} values={PRIORITIES} prefix="netstatus.priority" /></div>
            <div className="grid gap-2"><Label htmlFor="ni-status">{t("netstatus.status")}</Label><EnumSelect id="ni-status" value={f.status} onChange={(v) => setF({ ...f, status: v })} values={STATUSES} prefix="netstatus.status" /></div>
          </div>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
            <div className="grid gap-2"><Label htmlFor="ni-start">{t("netstatus.starts_at")}</Label><Input id="ni-start" type="datetime-local" value={f.starts_at} onChange={(e) => setF({ ...f, starts_at: e.target.value })} /></div>
            <div className="grid gap-2"><Label htmlFor="ni-end">{t("netstatus.ends_at")}</Label><Input id="ni-end" type="datetime-local" value={f.ends_at} onChange={(e) => setF({ ...f, ends_at: e.target.value })} /></div>
          </div>
          <div className="grid gap-2"><Label htmlFor="ni-desc">{t("billing.description")}</Label><Textarea id="ni-desc" rows={4} value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} /></div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function PostUpdateDialog({ issue, onDone }: { issue: NetworkIssue; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState(issue.status);
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const post = useMutation({
    mutationFn: () => api(`/admin/network-issues/${issue.id}/updates`, { method: "POST", body: { status, message } }),
    onSuccess: () => { setOpen(false); setMessage(""); setError(null); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline">{t("netstatus.post_update")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("netstatus.post_update")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); post.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="nu-status">{t("netstatus.status")}</Label><EnumSelect id="nu-status" value={status} onChange={setStatus} values={STATUSES} prefix="netstatus.status" /></div>
          <div className="grid gap-2"><Label htmlFor="nu-msg">{t("netstatus.message")}</Label><Textarea id="nu-msg" required rows={4} maxLength={5000} value={message} onChange={(e) => setMessage(e.target.value)} /></div>
          <DialogFooter><Button type="submit" disabled={post.isPending}>{t("netstatus.post_update")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AdminNetworkStatusPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "netstatus"], queryFn: () => api<{ items: NetworkIssue[] }>("/admin/network-issues") });
  const done = () => { void qc.invalidateQueries({ queryKey: ["admin", "netstatus"] }); void qc.invalidateQueries({ queryKey: ["netstatus"] }); };
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/network-issues/${id}`, { method: "DELETE" }), onSuccess: done });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("netstatus.title")} description={t("netstatus.admin_subtitle")} actions={<IssueDialog onDone={done} />} />
      {isLoading && <Skeleton className="h-24 w-full" />}
      {data && data.items.length === 0 && <EmptyState icon={PulseIcon} title={t("netstatus.none")} />}
      {data?.items.map((i) => (
        <IssueCard key={i.id} i={i} actions={
          <div className="flex flex-wrap justify-end gap-2">
            <PostUpdateDialog issue={i} onDone={done} />
            <IssueDialog issue={i} onDone={done} />
            <AlertDialog>
              <AlertDialogTrigger asChild><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader><AlertDialogTitle>{t("netstatus.delete_confirm")}</AlertDialogTitle></AlertDialogHeader>
                <AlertDialogFooter><AlertDialogCancel>{t("action.cancel")}</AlertDialogCancel><AlertDialogAction onClick={() => del.mutate(i.id)}>{t("common.delete")}</AlertDialogAction></AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        } />
      ))}
    </div>
  );
}
