import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useSearchParams } from "react-router";
import { ArrowLeftIcon, ChatsCircleIcon, LockIcon, LockOpenIcon, PaperPlaneRightIcon, SealCheckIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { LoadError } from "@/components/LoadError";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { unreadFor, type Conversation, type Inbox, type Thread } from "@/lib/creatorTools";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

function When({ at }: { at: string }) {
  const { lang } = useI18n();
  return <time dateTime={at} className="text-xs text-muted-foreground">{new Date(at).toLocaleString(lang, { dateStyle: "short", timeStyle: "short" })}</time>;
}

function Row({ c, creatorId, active }: { c: Conversation; creatorId: string | null; active: boolean }) {
  const { t } = useI18n();
  const mine = creatorId === c.creator_id;
  const n = unreadFor(c, creatorId);
  return (
    <Link to={`?c=${c.id}`} data-testid="conversation" aria-current={active ? "true" : undefined}
      className={cn("flex flex-col gap-0.5 rounded-md p-3 text-sm hover:bg-muted", active && "bg-muted")}>
      <span className="flex items-center gap-2">
        <span className={cn("min-w-0 flex-1 truncate", n > 0 && "font-semibold")}>{c.subject}</span>
        {n > 0 && <Badge>{n}</Badge>}
      </span>
      <span className="truncate text-muted-foreground">{mine ? t("ctools.from_buyer", { name: c.buyer }) : t("ctools.with_creator", { name: c.creator })}{c.product && ` · ${c.product}`}</span>
      <When at={c.last_message_at} />
    </Link>
  );
}

function ThreadView({ id }: { id: string }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [body, setBody] = useState("");
  const q = useQuery({
    queryKey: ["client", "market-messages", id],
    // Opening a thread marks it read: refresh the inbox badges after each fetch.
    queryFn: async () => { const r = await api<Thread>(`/client/market-messages/${id}`); void qc.invalidateQueries({ queryKey: ["client", "market-messages", "inbox"] }); return r; },
    refetchInterval: 20_000,
  });
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["client", "market-messages"] }); };
  const reply = useMutation({ mutationFn: () => api(`/client/market-messages/${id}/reply`, { method: "POST", body: { body: body.trim() } }), onSuccess: () => { setBody(""); refresh(); }, onError: fail });
  const status = useMutation({ mutationFn: (open: boolean) => api(`/client/market-messages/${id}/status`, { method: "POST", body: { open } }), onSuccess: refresh, onError: fail });
  if (q.isError) return <EmptyState icon={ChatsCircleIcon} title={t("ctools.conversation_missing")} />;
  if (!q.data) return <Skeleton className="h-80 w-full" />;
  const { conversation: c, messages, as_creator } = q.data;
  const closed = c.status === "closed";
  return (
    <Card className="flex min-h-[28rem] flex-col" data-testid="thread">
      <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-2 border-b">
        <div className="min-w-0">
          <CardTitle className="truncate text-base">{c.subject}</CardTitle>
          <CardDescription className="flex flex-wrap items-center gap-2">
            {as_creator ? t("ctools.from_buyer", { name: c.buyer }) : <Link className="hover:text-foreground" to={`/creators/${c.creator_slug}`}>{t("ctools.with_creator", { name: c.creator })}</Link>}
            {c.product_id && <Link className="hover:text-foreground" to={`/marketplace/r/${c.product_id}`}>{c.product}</Link>}
            {c.verified_buyer && <Badge variant="secondary"><SealCheckIcon aria-hidden />{t("ctools.verified_buyer")}</Badge>}
            {closed && <Badge variant="outline">{t("ctools.closed")}</Badge>}
          </CardDescription>
        </div>
        <Button size="sm" variant="outline" disabled={status.isPending} onClick={() => status.mutate(closed)}>
          {closed ? <><LockOpenIcon aria-hidden />{t("ctools.reopen")}</> : <><LockIcon aria-hidden />{t("ctools.close")}</>}
        </Button>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col gap-3 overflow-y-auto py-4" aria-live="polite">
        {messages.map((m) => {
          const own = m.from_creator === as_creator;
          return (
            <div key={m.id} className={cn("flex max-w-[85%] flex-col gap-1", own ? "self-end items-end" : "self-start")}>
              <p className={cn("whitespace-pre-wrap rounded-lg px-3 py-2 text-sm", own ? "bg-primary text-primary-foreground" : "bg-muted")}>{m.body}</p>
              <span className="text-xs text-muted-foreground">{m.from_creator ? c.creator : c.buyer} · <When at={m.created_at} /></span>
            </div>
          );
        })}
        <div key={messages.length} ref={(el) => el?.scrollIntoView?.({ block: "nearest" })} />
      </CardContent>
      {!closed ? (
        <form className="flex items-end gap-2 border-t p-3" onSubmit={(e) => { e.preventDefault(); if (body.trim()) reply.mutate(); }}>
          <Textarea aria-label={t("ctools.message")} rows={2} maxLength={5000} className="min-h-0 flex-1" value={body} onChange={(e) => setBody(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && body.trim()) reply.mutate(); }} placeholder={t("ctools.reply_placeholder")} />
          <Button type="submit" disabled={!body.trim() || reply.isPending}><PaperPlaneRightIcon aria-hidden />{t("ctools.send")}</Button>
        </form>
      ) : <p className="border-t p-3 text-sm text-muted-foreground">{t("ctools.closed_hint")}</p>}
    </Card>
  );
}

/** Client: conversations between buyers and marketplace creators. */
export function MarketMessagesPage() {
  const { t } = useI18n();
  const [params] = useSearchParams();
  const open = params.get("c");
  const q = useQuery({ queryKey: ["client", "market-messages", "inbox"], queryFn: () => api<Inbox>("/client/market-messages"), refetchInterval: 30_000 });
  if (q.isError && !q.data) return <LoadError retry={() => void q.refetch()} />;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("ctools.messages_title")} description={t("ctools.messages_description")} />
      {!q.data ? <Skeleton className="h-64 w-full" /> : q.data.items.length === 0 && !open ? (
        <EmptyState icon={ChatsCircleIcon} title={t("ctools.no_messages")} description={t("ctools.no_messages_hint")} />
      ) : (
        <div className="grid gap-4 lg:grid-cols-[20rem_minmax(0,1fr)]">
          <Card className={cn("h-fit p-1", open && "hidden lg:block")}>
            <nav aria-label={t("ctools.messages_title")} className="flex flex-col">
              {q.data.items.map((c) => <Row key={c.id} c={c} creatorId={q.data.creator_id} active={c.id === open} />)}
            </nav>
          </Card>
          {open ? (
            <div className="flex flex-col gap-2">
              <Button asChild size="sm" variant="ghost" className="self-start lg:hidden"><Link to="?"><ArrowLeftIcon aria-hidden />{t("ctools.all_messages")}</Link></Button>
              <ThreadView key={open} id={open} />
            </div>
          ) : <div className="hidden lg:block"><EmptyState icon={ChatsCircleIcon} title={t("ctools.pick_conversation")} /></div>}
        </div>
      )}
    </div>
  );
}
