import { useEffect, useState } from "react";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { ArrowLeftIcon, BookOpenIcon, LifebuoyIcon, PlusIcon, StarIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button, buttonVariants } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PageHeader } from "@/components/PageHeader";
import { ListPage } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { TicketThread } from "@/components/support/TicketThread";
import { TicketStatusBadge } from "@/components/support/TicketStatuses";
import { isUuid } from "@/lib/ids";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { Captcha } from "@/components/Captcha";
import { TicketAttachments, type PendingAttachment } from "@/components/support/TicketAttachments";
import { canRate, PRIORITIES, suggestText, priorityVariant, type Department, type Ticket, type TicketDetail } from "@/lib/support/support";

export function TicketStatus({ status }: { status: string }) {
  return <TicketStatusBadge status={status} />;
}

export function TicketPriority({ priority }: { priority: string }) {
  const { tCode } = useI18n();
  return <Badge variant={priorityVariant(priority)}>{tCode(`tickets.priority.${priority}`)}</Badge>;
}

function KbSuggestions({ subject, body }: { subject: string; body: string }) {
  const { t, lang } = useI18n();
  const [q, setQ] = useState<string | null>(null);
  useEffect(() => {
    const id = setTimeout(() => setQ(suggestText(subject, body)), 400);
    return () => clearTimeout(id);
  }, [subject, body]);
  const { data } = useQuery({
    queryKey: ["kb", "suggest", lang, q],
    queryFn: () => api<{ articles: { slug: string; title: string; excerpt: string }[] }>(`/public/kb/suggest?q=${encodeURIComponent(q ?? "")}&locale=${lang}`),
    enabled: !!q,
    staleTime: 60_000,
  });
  const items = q ? data?.articles ?? [] : [];
  if (items.length === 0) return null;
  return (
    <div className="grid gap-2 rounded-md border bg-muted/40 p-3" aria-live="polite">
      <p className="flex items-center gap-2 text-sm font-medium"><BookOpenIcon aria-hidden />{t("tickets.kb_suggest")}</p>
      <ul className="grid gap-1">
        {items.map((a) => (
          <li key={a.slug} className="min-w-0">
            <a href={`/kb/${a.slug}`} target="_blank" rel="noreferrer" className="block min-w-0 rounded-sm text-sm hover:underline">
              <span className="font-medium">{a.title}</span>
              <span className="line-clamp-1 text-xs text-muted-foreground">{a.excerpt}</span>
            </a>
          </li>
        ))}
      </ul>
    </div>
  );
}

function OpenTicketDialog() {
  const { t, tCode } = useI18n();
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const [open, setOpenState] = useState(params.get("new") === "1");
  const setOpen = (v: boolean) => {
    setOpenState(v);
    if (!v && params.has("new")) { params.delete("new"); setParams(params, { replace: true }); }
  };
  const [d, setD] = useState({ department_id: "", subject: "", body: "", priority: "medium" });
  const [files, setFiles] = useState<PendingAttachment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const { data: deps } = useQuery({ queryKey: ["client", "ticket-departments"], queryFn: () => api<{ items: Department[] }>("/client/ticket-departments"), enabled: open });
  const save = useMutation({
    mutationFn: () => api<TicketDetail>("/client/tickets", { method: "POST", body: { ...d, attachment_ids: files.map((f) => f.id) } }),
    onSuccess: (r) => { setOpen(false); setFiles([]); nav(`/tickets/${r.ticket.id}`); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button><PlusIcon aria-hidden />{t("tickets.new")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{t("tickets.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("tickets.department")}</Label>
              <Select value={d.department_id} onValueChange={(v) => setD({ ...d, department_id: v })}>
                <SelectTrigger className="w-full"><SelectValue placeholder={t("tickets.choose_department")} /></SelectTrigger>
                <SelectContent>{(deps?.items ?? []).map((x) => <SelectItem key={x.id} value={x.id}>{x.name}</SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label>{t("tickets.priority_label")}</Label>
              <Select value={d.priority} onValueChange={(v) => setD({ ...d, priority: v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{PRIORITIES.map((p) => <SelectItem key={p} value={p}>{tCode(`tickets.priority.${p}`)}</SelectItem>)}</SelectContent>
              </Select></div>
          </div>
          <div className="grid gap-2"><Label htmlFor="t-subj">{t("tickets.subject")}</Label>
            <Input id="t-subj" required maxLength={200} value={d.subject} onChange={(e) => setD({ ...d, subject: e.target.value })} /></div>
          <div className="grid gap-2"><Label htmlFor="t-body">{t("tickets.message")}</Label>
            <Textarea id="t-body" required rows={6} value={d.body} onChange={(e) => setD({ ...d, body: e.target.value })} /></div>
          <KbSuggestions subject={d.subject} body={d.body} />
          <TicketAttachments files={files} onChange={setFiles} disabled={save.isPending} />
          <Captcha form="ticket" signedIn />
          <DialogFooter><Button type="submit" disabled={save.isPending || !d.department_id}>{t("tickets.submit")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function ClientTicketsPage() {
  const { t, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["client", "tickets"], queryFn: () => api<{ items: Ticket[] }>("/client/tickets?per_page=100") });
  return (
    <ListPage<Ticket>
      title={t("tickets.title")} description={t("tickets.client_hint")} actions={<OpenTicketDialog />}
      rows={data?.items} rowKey={(x) => x.id} loading={isLoading} emptyIcon={LifebuoyIcon} emptyTitle={t("tickets.empty")}
      columns={[
        { header: t("tickets.subject"), primary: true, cell: (x) => <Link className="font-medium hover:underline" to={`/tickets/${x.id}`}>#{x.number} {x.subject}</Link> },
        { header: t("tickets.department"), cell: (x) => x.department_name },
        { header: t("billing.status"), cell: (x) => <TicketStatus status={x.status} /> },
        { header: t("tickets.last_reply"), cell: (x) => formatDateTime(x.last_reply_at, lang) },
      ]}
    />
  );
}

function RateCard({ ticket }: { ticket: Ticket }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [stars, setStars] = useState(0);
  const [comment, setComment] = useState("");
  const [error, setError] = useState<string | null>(null);
  const rate = useMutation({
    mutationFn: () => api(`/client/tickets/${ticket.id}/rate`, { method: "POST", body: { rating: stars, comment } }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["client", "ticket", ticket.id] }),
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (ticket.rating != null) {
    return <p className="text-sm text-muted-foreground">{t("tickets.rated", { rating: ticket.rating })}</p>;
  }
  if (!canRate(ticket)) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("tickets.rate_title")}</CardTitle><CardDescription>{t("tickets.rate_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <FormError message={error} />
        <div className="flex gap-1" role="radiogroup" aria-label={t("tickets.rate_title")}>
          {[1, 2, 3, 4, 5].map((n) => (
            <label key={n} className={cn(buttonVariants({ size: "icon", variant: n <= stars ? "default" : "outline" }), "cursor-pointer")}>
              <input type="radio" name="ticket-stars" className="sr-only" checked={n === stars} onChange={() => setStars(n)} />
              <span className="sr-only">{n}</span>
              <StarIcon aria-hidden weight={n <= stars ? "fill" : "regular"} />
            </label>
          ))}
        </div>
        <Textarea rows={2} maxLength={2000} placeholder={t("tickets.rate_comment")} value={comment} onChange={(e) => setComment(e.target.value)} />
        <div><Button disabled={!stars || rate.isPending} onClick={() => rate.mutate()}>{t("tickets.rate_send")}</Button></div>
      </CardContent>
    </Card>
  );
}

export function ClientTicketPage() {
  const { id = "" } = useParams();
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [body, setBody] = useState("");
  const [files, setFiles] = useState<PendingAttachment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const key = ["client", "ticket", id];
  const { data, isError } = useQuery({ queryKey: key, queryFn: () => api<TicketDetail>(`/client/tickets/${id}`), enabled: isUuid(id), retry: false });
  const onErr = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const reply = useMutation({
    mutationFn: () => api<TicketDetail>(`/client/tickets/${id}/reply`, { method: "POST", body: { body, attachment_ids: files.map((f) => f.id) } }),
    onSuccess: (r) => { setBody(""); setFiles([]); setError(null); qc.setQueryData(key, r); }, onError: onErr,
  });
  const close = useMutation({
    mutationFn: () => api(`/client/tickets/${id}/close`, { method: "POST" }),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }), onError: onErr,
  });
  if (!isUuid(id) || isError) return <p className="text-sm text-muted-foreground">{t("common.not_found")}</p>;
  if (!data) return <PageSkeleton />;
  const tk = data.ticket;
  return (
    <div className="flex flex-col gap-4">
      <div><Button asChild variant="ghost" size="sm"><Link to="/tickets"><ArrowLeftIcon aria-hidden />{t("tickets.title")}</Link></Button></div>
      <PageHeader
        title={`#${tk.number} ${tk.subject}`}
        description={<span className="flex flex-wrap items-center gap-2">{tk.department_name}<TicketStatus status={tk.status} /><TicketPriority priority={tk.priority} /></span>}
        actions={tk.status !== "closed" ? <Button variant="outline" onClick={() => close.mutate()} disabled={close.isPending}>{t("tickets.close")}</Button> : undefined}
      />
      <FormError message={error} />
      {tk.merged_into && <p className="text-sm text-muted-foreground"><Link className="underline" to={`/tickets/${tk.merged_into}`}>{t("tickets.merged_notice")}</Link></p>}
      <TicketThread ticketId={tk.id} messages={data.messages} area="client" />
      <RateCard ticket={tk} />
      {!tk.merged_into && (
        <Card>
          <CardHeader><CardTitle>{t("tickets.reply")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-3">
            <PluginSlot slot="client.ticket.reply" ctx={{ ticket_id: id }} />
            <Textarea rows={5} value={body} onChange={(e) => setBody(e.target.value)} aria-label={t("tickets.reply")} />
            <TicketAttachments files={files} onChange={setFiles} disabled={reply.isPending} />
            <div><Button disabled={!body.trim() || reply.isPending} onClick={() => reply.mutate()}>{t("tickets.send")}</Button></div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
