import { useState } from "react";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { OpenTicketDialog } from "@/pages/admin/support/OpenTicketDialog";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useNavigate, useParams } from "react-router";
import { ArrowLeftIcon, LifebuoyIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PageHeader } from "@/components/PageHeader";
import { DataTable } from "@/components/DataTable";
import { MassBar, selectColumn, useSelection } from "@/components/admin/MassBar";
import { FormError } from "@/components/Field";
import { TicketThread } from "@/components/support/TicketThread";
import { TicketAiSuggestion } from "@/components/support/TicketAiSuggestion";
import { TicketPriority, TicketStatus } from "@/pages/client/support/Tickets";
import { isUuid } from "@/lib/ids";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useTicketStatuses } from "@/components/support/TicketStatuses";
import { TicketTimeCard, TicketWatchersCard } from "@/pages/admin/support/SupportExt";
import { TicketSlaBadges } from "@/pages/admin/support/TicketSla";
import { TicketMacroButton } from "@/pages/admin/support/TicketMacros";
import { TicketAttachments, type PendingAttachment } from "@/components/support/TicketAttachments";
import { parseTags, PRIORITIES, type Canned, type Department, type Staff, type Ticket, type TicketDetail } from "@/lib/support/support";

export function AdminTicketsPage() {
  const statuses = useTicketStatuses();
  const { t, tCode, lang } = useI18n();
  const sel = useSelection();
  const [status, setStatus] = useState("active");
  const [q, setQ] = useState("");
  const qs = new URLSearchParams({
    ...(status !== "all" ? { status } : {}),
    ...(q.trim() ? { q: q.trim() } : {}),
  });
  const path = `/admin/tickets${qs.toString() ? `?${qs}` : ""}`;
  return (
    <div className="flex flex-col gap-3">
      <MassBar sel={sel} endpoint="/admin/mass/tickets" invalidate={["admin", "tickets"]} actions={[
        { key: "set_status", label: t("mass.set_status"), options: statuses.items.map((s) => ({ value: s.slug, label: statuses.label(s.slug) })) },
        { key: "set_priority", label: t("mass.set_priority"), options: PRIORITIES.map((p) => ({ value: p, label: tCode(`tickets.priority.${p}`) })) },
        { key: "unassign", action: "assign", label: t("mass.unassign"), body: (ids) => ({ ids, action: "assign", value: null }) },
      ]} />
      <DataTable<Ticket>
        title={t("tickets.title")} path={path} queryKey={["admin", "tickets", status, q]}
        emptyIcon={LifebuoyIcon} emptyTitle={t("tickets.empty")}
        toolbar={
          <>
            <Input className="h-8 w-56" placeholder={t("common.search")} aria-label={t("common.search")} value={q} onChange={(e) => setQ(e.target.value)} />
            <Select value={status} onValueChange={setStatus}>
              <SelectTrigger aria-label={t("billing.status")} className="h-11 w-48 md:h-8"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="active" className="min-h-11 md:min-h-8">{t("tickets.filter_active")}</SelectItem>
                <SelectItem value="all" className="min-h-11 md:min-h-8">{t("common.all")}</SelectItem>
                {statuses.items.map((s) => <SelectItem key={s.slug} value={s.slug} className="min-h-11 md:min-h-8">{statuses.label(s.slug)}</SelectItem>)}
              </SelectContent>
            </Select>
            <Button asChild variant="outline"><Link to="/admin/support-settings">{t("tickets.settings")}</Link></Button>
            <OpenTicketDialog />
          </>
        }
        columns={[
          selectColumn<Ticket>(sel, (x) => x.subject),
          { header: t("tickets.subject"), primary: true, cell: (x) => <Link className="font-medium hover:underline" to={`/admin/tickets/${x.id}`}>#{x.number} {x.subject}</Link> },
          { header: t("tickets.customer"), cell: (x) => <span className="break-all">{x.customer_name}</span> },
          { header: t("tickets.department"), cell: (x) => x.department_name },
          { header: t("tickets.priority_label"), cell: (x) => <TicketPriority priority={x.priority} /> },
          { header: t("billing.status"), cell: (x) => <TicketStatus status={x.status} /> },
          { header: t("tickets.last_reply"), cell: (x) => formatDateTime(x.last_reply_at, lang) },
        ]}
      />
    </div>
  );
}

export function AdminTicketPage() {
  const statuses = useTicketStatuses();
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const qc = useQueryClient();
  const key = ["admin", "ticket", id];
  const [body, setBody] = useState("");
  const [files, setFiles] = useState<PendingAttachment[]>([]);
  const [internal, setInternal] = useState(false);
  const [tags, setTags] = useState<string | null>(null);
  const [mergeNo, setMergeNo] = useState("");
  const [picked, setPicked] = useState<string[]>([]);
  const [splitSubject, setSplitSubject] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { data, isError } = useQuery({ queryKey: key, queryFn: () => api<TicketDetail>(`/admin/tickets/${id}`), enabled: isUuid(id), retry: false });
  const { data: deps } = useQuery({ queryKey: ["admin", "ticket-departments"], queryFn: () => api<{ items: Department[] }>("/admin/ticket-departments") });
  const { data: staff } = useQuery({ queryKey: ["admin", "ticket-staff"], queryFn: () => api<{ items: Staff[] }>("/admin/ticket-staff") });
  const dep = data?.ticket.department_id;
  const { data: canned } = useQuery({ queryKey: ["admin", "ticket-canned", dep], queryFn: () => api<{ items: Canned[] }>(`/admin/ticket-canned?department_id=${dep}`), enabled: !!dep });
  const onErr = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const done = (r?: TicketDetail) => { setError(null); if (r?.ticket) qc.setQueryData(key, r); else void qc.invalidateQueries({ queryKey: key }); };
  const reply = useMutation({
    mutationFn: () => api<TicketDetail>(`/admin/tickets/${id}/reply`, { method: "POST", body: { body, internal, attachment_ids: files.map((f) => f.id) } }),
    onSuccess: (r) => { setBody(""); setFiles([]); done(r); }, onError: onErr,
  });
  const patch = useMutation({ mutationFn: (b: Record<string, unknown>) => api<TicketDetail>(`/admin/tickets/${id}`, { method: "PATCH", body: b }), onSuccess: done, onError: onErr });
  const saveTags = useMutation({ mutationFn: () => api(`/admin/tickets/${id}/tags`, { method: "PUT", body: { tags: parseTags(tags ?? "") } }), onSuccess: () => { setTags(null); done(); }, onError: onErr });
  const merge = useMutation({
    mutationFn: async () => {
      const found = await api<{ items: Ticket[] }>(`/admin/tickets?q=${encodeURIComponent(mergeNo.trim())}`);
      const target = found.items.find((x) => String(x.number) === mergeNo.trim());
      if (!target) throw new ApiError(404, "error.not_found", "");
      return api<{ ticket_id: string }>(`/admin/tickets/${id}/merge`, { method: "POST", body: { target_id: target.id } });
    },
    onSuccess: (r) => nav(`/admin/tickets/${r.ticket_id}`), onError: onErr,
  });
  const split = useMutation({
    mutationFn: () => api<{ ticket_id: string }>(`/admin/tickets/${id}/split`, { method: "POST", body: { message_ids: picked, subject: splitSubject } }),
    onSuccess: (r) => { setPicked([]); setSplitSubject(""); nav(`/admin/tickets/${r.ticket_id}`); }, onError: onErr,
  });
  if (!isUuid(id) || isError) return <p className="text-sm text-muted-foreground">{t("common.not_found")}</p>;
  if (!data) return <PageSkeleton />;
  const tk = data.ticket;
  return (
    <div className="flex flex-col gap-4">
      <div><Button asChild variant="ghost" size="sm"><Link to="/admin/tickets"><ArrowLeftIcon aria-hidden />{t("tickets.title")}</Link></Button></div>
      <PageHeader
        title={`#${tk.number} ${tk.subject}`}
        description={<span className="flex flex-wrap items-center gap-2">{tk.customer_name} · {tk.customer_email}<TicketStatus status={tk.status} /><TicketPriority priority={tk.priority} />
          <TicketSlaBadges ticketId={tk.id} version={`${tk.status}|${tk.priority}|${tk.department_id}|${tk.first_response_at}`} />
          {tk.rating != null && <Badge variant="outline">{t("tickets.rated", { rating: tk.rating })}</Badge>}</span>}
      />
      <FormError message={error} />
      {tk.merged_into && <p className="text-sm text-muted-foreground"><Link className="underline" to={`/admin/tickets/${tk.merged_into}`}>{t("tickets.merged_notice")}</Link></p>}
      <div className="grid gap-4 *:min-w-0 lg:grid-cols-[1fr_20rem]">
        <div className="flex min-w-0 flex-col gap-4">
          <TicketThread ticketId={tk.id} messages={data.messages} area="admin" selected={picked} onSelect={(mid, on) => setPicked((p) => (on ? [...p, mid] : p.filter((x) => x !== mid)))} />
          {picked.length > 0 && (
            <Card>
              <CardHeader><CardTitle>{t("tickets.split_title", { count: picked.length })}</CardTitle></CardHeader>
              <CardContent className="flex flex-col gap-2 sm:flex-row">
                <Input placeholder={t("tickets.subject")} value={splitSubject} onChange={(e) => setSplitSubject(e.target.value)} />
                <Button disabled={!splitSubject.trim() || split.isPending} onClick={() => split.mutate()}>{t("tickets.split")}</Button>
              </CardContent>
            </Card>
          )}
          {!tk.merged_into && (
            <Card>
              <CardHeader><CardTitle>{internal ? t("tickets.add_note") : t("tickets.reply")}</CardTitle></CardHeader>
              <CardContent className="flex flex-col gap-3">
                <div className="flex flex-col gap-2 sm:flex-row">
                  {(canned?.items.length ?? 0) > 0 && (
                    <Select value="" onValueChange={(v) => { const c = canned?.items.find((x) => x.id === v); if (c) setBody((b) => (b ? `${b}\n\n${c.body}` : c.body)); }}>
                      <SelectTrigger className="w-full sm:w-64"><SelectValue placeholder={t("tickets.insert_canned")} /></SelectTrigger>
                      <SelectContent>{canned?.items.map((c) => <SelectItem key={c.id} value={c.id}>{c.title}</SelectItem>)}</SelectContent>
                    </Select>
                  )}
                  <TicketMacroButton ticketId={tk.id} departmentId={tk.department_id} onApplied={done} />
                </div>
                {!internal && <TicketAiSuggestion key={tk.id} ticketId={tk.id} onInsert={(draft) => setBody((current) => current ? `${current}\n\n${draft}` : draft)} />}
                <PluginSlot slot="admin.ticket.reply" ctx={{ ticket_id: id }} />
                <Textarea rows={6} value={body} onChange={(e) => setBody(e.target.value)} aria-label={t("tickets.reply")} />
                <TicketAttachments files={files} onChange={setFiles} disabled={reply.isPending} />
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <label className="flex items-center gap-2 text-sm"><Switch checked={internal} onCheckedChange={setInternal} />{t("tickets.internal_note")}</label>
                  <Button disabled={!body.trim() || reply.isPending} onClick={() => reply.mutate()}>{internal ? t("tickets.add_note") : t("tickets.send")}</Button>
                </div>
              </CardContent>
            </Card>
          )}
        </div>
        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader><CardTitle>{t("tickets.details")}</CardTitle></CardHeader>
            <CardContent className="grid gap-3">
              <div className="grid gap-2"><Label>{t("billing.status")}</Label>
                <Select value={tk.status} onValueChange={(v) => patch.mutate({ status: v })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{statuses.items.map((s) => <SelectItem key={s.slug} value={s.slug}>{statuses.label(s.slug)}</SelectItem>)}</SelectContent>
                </Select></div>
              <div className="grid gap-2"><Label>{t("tickets.priority_label")}</Label>
                <Select value={tk.priority} onValueChange={(v) => patch.mutate({ priority: v })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{PRIORITIES.map((p) => <SelectItem key={p} value={p}>{tCode(`tickets.priority.${p}`)}</SelectItem>)}</SelectContent>
                </Select></div>
              <div className="grid gap-2"><Label>{t("tickets.department")}</Label>
                <Select value={tk.department_id} onValueChange={(v) => patch.mutate({ department_id: v })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{(deps?.items ?? []).map((d) => <SelectItem key={d.id} value={d.id}>{d.name}</SelectItem>)}</SelectContent>
                </Select></div>
              <div className="grid gap-2"><Label>{t("tickets.assignee")}</Label>
                <Select value={tk.assigned_to ?? "none"} onValueChange={(v) => patch.mutate({ assigned_to: v === "none" ? null : v })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t("tickets.unassigned")}</SelectItem>
                    {(staff?.items ?? []).map((u) => <SelectItem key={u.id} value={u.id}>{u.display_name || u.email}</SelectItem>)}
                  </SelectContent>
                </Select></div>
              <p className="text-sm text-muted-foreground">{t("tickets.first_response")}: {tk.first_response_at ? formatDateTime(tk.first_response_at, lang) : "-"}</p>
            </CardContent>
          </Card>
          {data && <TicketTimeCard ticketId={data.ticket.id} />}
          {data && <TicketWatchersCard ticketId={data.ticket.id} />}
          <Card>
            <CardHeader><CardTitle>{t("tickets.tags")}</CardTitle></CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-1">{data.tags.length ? data.tags.map((x) => <Badge key={x} variant="secondary">{x}</Badge>) : <span className="text-sm text-muted-foreground">-</span>}</div>
              <Input placeholder={t("tickets.tags_hint")} value={tags ?? data.tags.join(", ")} onChange={(e) => setTags(e.target.value)} />
              <div><Button size="sm" variant="outline" disabled={tags === null || saveTags.isPending} onClick={() => saveTags.mutate()}>{t("common.save")}</Button></div>
            </CardContent>
          </Card>
          {!tk.merged_into && (
            <Card>
              <CardHeader><CardTitle>{t("tickets.merge")}</CardTitle></CardHeader>
              <CardContent className="flex flex-col gap-3">
                <p className="text-sm text-muted-foreground">{t("tickets.merge_hint")}</p>
                <div className="flex gap-2">
                  <Input inputMode="numeric" placeholder="1234" value={mergeNo} onChange={(e) => setMergeNo(e.target.value.replace(/\D/g, ""))} aria-label={t("tickets.merge_target")} />
                  <Button variant="outline" disabled={!mergeNo || merge.isPending} onClick={() => merge.mutate()}>{t("tickets.merge")}</Button>
                </div>
              </CardContent>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}
