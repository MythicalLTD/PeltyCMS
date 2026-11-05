import { useState } from "react";
import { Link } from "react-router";
import { ArrowsClockwiseIcon, PlusIcon, TicketIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { ListCard } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { defaultPort, formatMinutes, PIPE_PROTOCOLS, SPAM_KINDS, type ContactMessage, type DeptSettings, type PipeProtocol, type SpamKind, type SpamRule, type TimeEntry } from "@/lib/support/supportExt";

function useErr() {
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return { error, setError, onError: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) };
}

function MailboxDialog({ dep, onDone }: { dep: DeptSettings; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [f, setF] = useState({ ...dep, pipe_password: "" });
  const [inboundToken, setInboundToken] = useState<string | null>(null);
  const { error, setError, onError } = useErr();
  const save = useMutation({
    mutationFn: () => api(`/admin/support/departments/${dep.id}`, { method: "PUT", body: { ...f, pipe_password: f.pipe_password || null } }),
    onSuccess: () => { setOpen(false); setInboundToken(null); toast.success(t("common.saved")); onDone(); },
    onError,
  });
  const rotate = useMutation({
    mutationFn: () => api<{ token: string }>("/admin/support/departments/" + dep.id + "/inbound-token", { method: "POST" }),
    onSuccess: (result) => setInboundToken(result.token), onError,
  });
  const set = <K extends keyof typeof f>(k: K, v: (typeof f)[K]) => setF((x) => ({ ...x, [k]: v }));
  const piped = f.pipe_protocol !== "none";
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); setInboundToken(null); if (o) { setF({ ...dep, pipe_password: "" }); setError(null); } }}>
      <DialogTrigger asChild><Button size="sm" variant="outline">{t("common.edit")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{dep.name}</DialogTitle></DialogHeader>
        <div className="grid gap-3">
          <div className="flex items-center gap-2"><Switch id="ar" checked={f.auto_reply} onCheckedChange={(v) => set("auto_reply", v)} /><Label htmlFor="ar">{t("support_ext.auto_reply")}</Label></div>
          {f.auto_reply && <Textarea aria-label={t("support_ext.auto_reply_text")} placeholder={t("support_ext.auto_reply_text")} value={f.auto_reply_text} onChange={(e) => set("auto_reply_text", e.target.value)} rows={3} />}
          <div className="grid gap-1.5">
            <Label>{t("support_ext.protocol")}</Label>
            <Select value={f.pipe_protocol} onValueChange={(v) => setF((x) => ({ ...x, pipe_protocol: v as PipeProtocol, pipe_port: v === "none" ? x.pipe_port : defaultPort(v as PipeProtocol, x.pipe_tls) }))}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{PIPE_PROTOCOLS.map((p) => <SelectItem key={p} value={p}>{tCode(`support_ext.protocol.${p}`)}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          {piped && (
            <>
              <div className="grid gap-3 sm:grid-cols-[1fr_7rem]">
                <div className="grid gap-1.5"><Label htmlFor="ph">{t("support_ext.host")}</Label><Input id="ph" value={f.pipe_host} onChange={(e) => set("pipe_host", e.target.value)} /></div>
                <div className="grid gap-1.5"><Label htmlFor="pp">{t("support_ext.port")}</Label><Input id="pp" type="number" inputMode="numeric" value={f.pipe_port} onChange={(e) => set("pipe_port", Number(e.target.value))} /></div>
              </div>
              <div className="flex items-center gap-2"><Switch id="tls" checked={f.pipe_tls} onCheckedChange={(v) => setF((x) => ({ ...x, pipe_tls: v, pipe_port: defaultPort(x.pipe_protocol, v) }))} /><Label htmlFor="tls">{t("support_ext.tls")}</Label></div>
              <div className="grid gap-1.5"><Label htmlFor="pu">{t("support_ext.username")}</Label><Input id="pu" autoComplete="off" value={f.pipe_username} onChange={(e) => set("pipe_username", e.target.value)} /></div>
              <div className="grid gap-1.5"><Label htmlFor="pw">{t("support_ext.password")}</Label><Input id="pw" type="password" autoComplete="new-password" placeholder={dep.has_password ? t("support_ext.password_keep") : ""} value={f.pipe_password} onChange={(e) => set("pipe_password", e.target.value)} /></div>
            </>
          )}
          <div className="flex items-center gap-2"><Switch id="guest-mail" checked={f.pipe_unknown_sender === "guest"} onCheckedChange={(v) => set("pipe_unknown_sender", v ? "guest" : "reject")} /><Label htmlFor="guest-mail">{t("support_ext.email_guests")}</Label></div>
          <p className="text-xs text-muted-foreground">{t("support_ext.email_guests_hint")}</p>
          <Button type="button" variant="outline" disabled={rotate.isPending} onClick={() => rotate.mutate()}>{t("support_ext.inbound_token_rotate")}</Button>
          {inboundToken && <><p className="text-xs text-muted-foreground">{t("support_ext.inbound_token_hint")}</p><Textarea readOnly aria-label={t("support_ext.inbound_token_rotate")} value={inboundToken} /></>}
          <FormError message={error} />
        </div>
        <DialogFooter><Button onClick={() => save.mutate()} disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function MailboxesTab() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "support-departments"];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<{ departments: DeptSettings[]; encryption: boolean }>("/admin/support/departments") });
  const refresh = () => void qc.invalidateQueries({ queryKey: key });
  const { error, onError } = useErr();
  const poll = useMutation({ mutationFn: (id: string) => api(`/admin/support/departments/${id}/pipe`, { method: "POST" }), onSuccess: () => { toast.success(t("support_ext.polled")); refresh(); }, onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("support_ext.mailboxes")}</CardTitle><CardDescription>{t("support_ext.mailboxes_hint")}</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-3">
          {data && !data.encryption && <p className="text-sm text-destructive">{t("error.encryption_not_configured")}</p>}
          <FormError message={error} />
        </CardContent>
      </Card>
      <ListCard<DeptSettings> rows={data?.departments} rowKey={(d) => d.id} loading={isLoading} columns={[
        { header: t("support_ext.department"), primary: true, cell: (d) => <div className="min-w-0"><p className="font-medium">{d.name}</p><p className="truncate text-xs text-muted-foreground">{d.email || "-"}</p></div> },
        { header: t("support_ext.protocol"), cell: (d) => <Badge variant={d.pipe_protocol === "none" ? "outline" : "default"}>{tCode(`support_ext.protocol.${d.pipe_protocol}`)}</Badge> },
        { header: t("support_ext.auto_reply"), cell: (d) => (d.auto_reply ? t("support_ext.yes") : t("support_ext.no")) },
        { header: t("automation.last_run"), cell: (d) => <div className="min-w-0 text-xs">{d.pipe_last_run ? formatDateTime(d.pipe_last_run, lang) : "-"}{d.pipe_last_error && <p className="line-clamp-2 text-destructive">{d.pipe_last_error}</p>}</div> },
        { header: "", cell: (d) => (
          <div className="flex justify-end gap-1">
            {d.pipe_protocol !== "none" && <Button size="sm" variant="ghost" aria-label={t("support_ext.poll_now")} disabled={poll.isPending} onClick={() => poll.mutate(d.id)}><ArrowsClockwiseIcon aria-hidden /></Button>}
            <MailboxDialog dep={d} onDone={refresh} />
          </div>) },
      ]} />
    </div>
  );
}

export function SpamTab() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "spam-rules"];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<{ rules: SpamRule[] }>("/admin/support/spam-rules") });
  const refresh = () => void qc.invalidateQueries({ queryKey: key });
  const [kind, setKind] = useState<SpamKind>("domain");
  const [value, setValue] = useState("");
  const { error, setError, onError } = useErr();
  const add = useMutation({ mutationFn: () => api("/admin/support/spam-rules", { method: "POST", body: { kind, value } }), onSuccess: () => { setValue(""); setError(null); refresh(); }, onError });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/support/spam-rules/${id}`, { method: "DELETE" }), onSuccess: refresh, onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("support_ext.spam")}</CardTitle><CardDescription>{t("support_ext.spam_hint")}</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-3">
          <form className="flex flex-col gap-2 sm:flex-row" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
            <Select value={kind} onValueChange={(v) => setKind(v as SpamKind)}>
              <SelectTrigger className="w-full sm:w-40" aria-label={t("support_ext.spam_kind")}><SelectValue /></SelectTrigger>
              <SelectContent>{SPAM_KINDS.map((k) => <SelectItem key={k} value={k}>{tCode(`support_ext.spam_kind.${k}`)}</SelectItem>)}</SelectContent>
            </Select>
            <Input aria-label={t("support_ext.spam_value")} placeholder={t("support_ext.spam_value")} value={value} onChange={(e) => setValue(e.target.value)} />
            <Button type="submit" disabled={add.isPending || !value.trim()}><PlusIcon aria-hidden />{t("common.add")}</Button>
          </form>
          <FormError message={error} />
        </CardContent>
      </Card>
      <ListCard<SpamRule> rows={data?.rules} rowKey={(r) => r.id} loading={isLoading} columns={[
        { header: t("support_ext.spam_value"), primary: true, cell: (r) => <span className="break-all font-mono text-sm">{r.value}</span> },
        { header: t("support_ext.spam_kind"), cell: (r) => tCode(`support_ext.spam_kind.${r.kind}`) },
        { header: t("support_ext.hits"), cell: (r) => r.hits },
        { header: "", cell: (r) => <div className="flex justify-end"><ConfirmAction description={t("support_ext.spam_delete_confirm")} onConfirm={() => del.mutate(r.id)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction></div> },
      ]} />
    </div>
  );
}

const STATUS_VARIANT = { new: "default", read: "secondary", spam: "destructive", converted: "outline" } as const;

function ConvertDialog({ msg, onDone }: { msg: ContactMessage; onDone: () => void }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [dep, setDep] = useState("");
  const deps = useQuery({ queryKey: ["admin", "ticket-departments"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/ticket-departments"), enabled: open }).data?.items ?? [];
  const { error, onError } = useErr();
  const go = useMutation({ mutationFn: () => api<{ number: number }>(`/admin/contact-messages/${msg.id}/ticket`, { method: "POST", body: { department_id: dep } }), onSuccess: (r) => { setOpen(false); toast.success(t("support_ext.converted", { n: r.number })); onDone(); }, onError });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="ghost" aria-label={t("support_ext.to_ticket")}><TicketIcon aria-hidden /></Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("support_ext.to_ticket")}</DialogTitle></DialogHeader>
        <Select value={dep} onValueChange={setDep}>
          <SelectTrigger className="w-full" aria-label={t("support_ext.department")}><SelectValue placeholder={t("support_ext.department")} /></SelectTrigger>
          <SelectContent>{deps.map((d) => <SelectItem key={d.id} value={d.id}>{d.name}</SelectItem>)}</SelectContent>
        </Select>
        <FormError message={error} />
        <DialogFooter><Button disabled={!dep || go.isPending} onClick={() => go.mutate()}>{t("support_ext.to_ticket")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ContactTab() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [status, setStatus] = useState("new");
  const key = ["admin", "contact-messages", status];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<{ messages: ContactMessage[] }>(`/admin/contact-messages${status === "all" ? "" : `?status=${status}`}`) });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "contact-messages"] });
  const { error, onError } = useErr();
  const mark = useMutation({ mutationFn: (v: { id: string; status: string }) => api(`/admin/contact-messages/${v.id}`, { method: "PUT", body: { status: v.status } }), onSuccess: refresh, onError });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/contact-messages/${id}`, { method: "DELETE" }), onSuccess: refresh, onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("support_ext.contact_inbox")}</CardTitle><CardDescription>{t("support_ext.contact_hint")}</CardDescription>
          <CardAction>
            <Select value={status} onValueChange={setStatus}>
              <SelectTrigger size="sm" className="w-36" aria-label={t("billing.status")}><SelectValue /></SelectTrigger>
              <SelectContent>{["all", "new", "read", "spam", "converted"].map((s) => <SelectItem key={s} value={s}>{tCode(`support_ext.contact_status.${s}`)}</SelectItem>)}</SelectContent>
            </Select>
          </CardAction>
        </CardHeader>
        <CardContent><FormError message={error} /></CardContent>
      </Card>
      <ListCard<ContactMessage> rows={data?.messages} rowKey={(m) => m.id} loading={isLoading} columns={[
          { header: t("support_ext.from"), primary: true, cell: (m) => <div className="min-w-0"><p className="font-medium">{m.name}</p><p className="truncate text-xs text-muted-foreground">{m.email}</p></div> },
          { header: t("tickets.subject"), cell: (m) => <div className="min-w-0"><p>{m.subject}</p><p className="line-clamp-3 whitespace-pre-wrap text-xs text-muted-foreground">{m.body}</p></div> },
          { header: t("billing.status"), cell: (m) => <div className="flex flex-col items-start gap-1"><Badge variant={STATUS_VARIANT[m.status]}>{tCode(`support_ext.contact_status.${m.status}`)}</Badge>{m.ticket_id && <Link className="text-xs underline" to={`/admin/tickets/${m.ticket_id}`}>{t("support_ext.open_ticket")}</Link>}</div> },
          { header: t("support_ext.date"), cell: (m) => <span className="text-xs">{formatDateTime(m.created_at, lang)}</span> },
          { header: "", cell: (m) => (
            <div className="flex flex-wrap justify-end gap-1">
              {m.status === "new" && <Button size="sm" variant="outline" onClick={() => mark.mutate({ id: m.id, status: "read" })}>{t("support_ext.mark_read")}</Button>}
              {m.status !== "spam" && m.status !== "converted" && <Button size="sm" variant="outline" onClick={() => mark.mutate({ id: m.id, status: "spam" })}>{t("support_ext.mark_spam")}</Button>}
              {m.status !== "converted" && <ConvertDialog msg={m} onDone={refresh} />}
              <ConfirmAction description={t("support_ext.contact_delete_confirm")} onConfirm={() => del.mutate(m.id)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
            </div>) },
        ]} />
    </div>
  );
}

/** Staff time log for one ticket (admin ticket sidebar). */
export function TicketTimeCard({ ticketId }: { ticketId: string }) {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "ticket-time", ticketId];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ entries: TimeEntry[]; total_minutes: number; billable_minutes: number }>(`/admin/tickets/${ticketId}/time`) });
  const refresh = () => void qc.invalidateQueries({ queryKey: key });
  const [minutes, setMinutes] = useState("");
  const [note, setNote] = useState("");
  const [billable, setBillable] = useState(false);
  const { error, setError, onError } = useErr();
  const add = useMutation({ mutationFn: () => api(`/admin/tickets/${ticketId}/time`, { method: "POST", body: { minutes: Number(minutes), note, billable } }), onSuccess: () => { setMinutes(""); setNote(""); setError(null); refresh(); }, onError });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/tickets/${ticketId}/time/${id}`, { method: "DELETE" }), onSuccess: refresh, onError });
  return (
    <Card>
      <CardHeader><CardTitle>{t("support_ext.time")}</CardTitle>
        {data && <CardDescription>{t("support_ext.time_total", { total: formatMinutes(data.total_minutes), billable: formatMinutes(data.billable_minutes) })}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
          <div className="flex gap-2">
            <Input className="w-24" type="number" inputMode="numeric" min={1} max={1440} aria-label={t("support_ext.minutes")} placeholder={t("support_ext.minutes")} value={minutes} onChange={(e) => setMinutes(e.target.value)} />
            <Input aria-label={t("support_ext.note")} placeholder={t("support_ext.note")} value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2"><Switch id={`bill-${ticketId}`} checked={billable} onCheckedChange={setBillable} /><Label htmlFor={`bill-${ticketId}`}>{t("support_ext.billable")}</Label></div>
            <Button type="submit" size="sm" disabled={!minutes || add.isPending}><PlusIcon aria-hidden />{t("common.add")}</Button>
          </div>
        </form>
        <FormError message={error} />
        <ul className="flex flex-col gap-2 text-sm">
          {data?.entries.map((e) => (
            <li key={e.id} className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <p className="font-medium">{formatMinutes(e.minutes)}{e.billable && <Badge variant="outline" className="ml-2">{t("support_ext.billable")}</Badge>}</p>
                <p className="truncate text-xs text-muted-foreground">{[e.staff_name, formatDateTime(e.created_at, lang), e.note].filter(Boolean).join(" · ")}</p>
              </div>
              <Button size="sm" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(e.id)}><TrashIcon aria-hidden /></Button>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

/** Extra staff who follow a ticket and get its notifications. */
export function TicketWatchersCard({ ticketId }: { ticketId: string }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "ticket-watchers", ticketId];
  const staff = useQuery({ queryKey: ["admin", "ticket-staff"], queryFn: () => api<{ items: { id: string; email: string; display_name: string }[] }>("/admin/ticket-staff") }).data?.items ?? [];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ watchers: string[] }>(`/admin/tickets/${ticketId}/watchers`) });
  const { error, onError } = useErr();
  const save = useMutation({ mutationFn: (ids: string[]) => api<{ watchers: string[] }>(`/admin/tickets/${ticketId}/watchers`, { method: "PUT", body: { user_ids: ids } }), onSuccess: (r) => qc.setQueryData(key, r), onError });
  const cur = data?.watchers ?? [];
  return (
    <Card>
      <CardHeader><CardTitle>{t("support_ext.watchers")}</CardTitle><CardDescription>{t("support_ext.watchers_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-2">
        {staff.map((s) => {
          const on = cur.includes(s.id);
          return (
            <div key={s.id} className="flex items-center justify-between gap-2 text-sm">
              <Label htmlFor={`w-${s.id}`} className="min-w-0 truncate">{s.display_name || s.email}</Label>
              <Switch id={`w-${s.id}`} checked={on} disabled={save.isPending} onCheckedChange={(v) => save.mutate(v ? [...cur, s.id] : cur.filter((x) => x !== s.id))} />
            </div>
          );
        })}
        <FormError message={error} />
      </CardContent>
    </Card>
  );
}
