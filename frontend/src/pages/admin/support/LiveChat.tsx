import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { useLiveChat } from "@/hooks/useLiveChat";
import { useMe } from "@/lib/auth";
import { LiveChatSettings, LiveChatActions, LiveChatSuggest } from "@/components/support/LiveChatAdmin";
import { ChatsCircleIcon, PaperPlaneRightIcon, XCircleIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { Bubble } from "@/components/support/ChatWidget";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { chatComposerKeyDown, lastId, mergeMessages, validBody, type ChatMessage, type ChatSession } from "@/lib/support/liveChat";

export function AdminLiveChatPage() {
  const { t, tCode } = useI18n();
  const [search, setSearch] = useSearchParams();
  const status = search.get("status") === "closed" ? "closed" : "open";
  const sel = search.get("session");
  const setSel = (session: string) => setSearch(previous => { const next = new URLSearchParams(previous); next.set("session", session); return next; });
  const inbox = useQuery({
    queryKey: ["admin", "chats", status],
    queryFn: () => api<{ sessions: ChatSession[]; unread: number }>(`/admin/chats?status=${status}`),
    refetchInterval: 15000,
  });
  const me = useMe();
  useLiveChat(me.data?.permissions.includes("tickets.manage") ? "/admin/chats/ws" : null, { csrf: me.data?.csrf_token }, () => { void inbox.refetch(); }, () => 0);
  const sessions = inbox.data?.sessions ?? [];
  const current = sessions.find((s) => s.id === sel) ?? null;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("chat.admin.title")} description={t("chat.admin.subtitle")} />
      {me.data?.permissions.includes("tickets.manage") && <LiveChatSettings />}
      <div className="grid gap-4 lg:grid-cols-[20rem_1fr]">
        <Card>
          <CardHeader className="gap-3">
            <CardTitle className="flex items-center gap-2"><ChatsCircleIcon /> {t("chat.admin.inbox")} {!!inbox.data?.unread && <Badge>{inbox.data.unread}</Badge>}</CardTitle>
            <Tabs value={status} onValueChange={v => setSearch(previous => { const next = new URLSearchParams(previous); next.set("status", v); next.delete("session"); return next; })}>
              <TabsList><TabsTrigger value="open">{t("chat.admin.open")}</TabsTrigger><TabsTrigger value="closed">{t("chat.admin.closed")}</TabsTrigger></TabsList>
            </Tabs>
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            {sessions.length === 0 && <p className="text-sm text-muted-foreground">{t("chat.admin.empty")}</p>}
            {sessions.map((s) => (
              <Button key={s.id} variant={s.id === sel ? "secondary" : "ghost"} className="h-auto flex-col items-start gap-0.5 text-left" onClick={() => setSel(s.id)}>
                <span className="flex w-full items-center justify-between gap-2 font-medium">{s.name}{s.staff_unread > 0 && <Badge>{s.staff_unread}</Badge>}</span>
                <span className="w-full truncate text-xs text-muted-foreground">{s.last_body ?? s.email}</span>
              </Button>
            ))}
          </CardContent>
        </Card>
        {current ? <Conversation key={current.id} s={current} onClosed={() => void inbox.refetch()} /> : sel ? (
          <Card><CardContent className="grid justify-items-center gap-3 py-10 text-center text-sm text-muted-foreground">
            <p role={inbox.isError ? "alert" : "status"}>{tCode(inbox.isPending ? "workspace.loading" : inbox.isError ? "workspace.chat_load_error" : "workspace.chat_unavailable")}</p>
            {!inbox.isPending && <Button variant="outline" size="sm" disabled={inbox.isFetching} onClick={() => void inbox.refetch()}>{tCode("workspace.retry")}</Button>}
          </CardContent></Card>
        ) : (
          <Card><CardContent className="py-10 text-center text-sm text-muted-foreground">{t("chat.admin.pick")}</CardContent></Card>
        )}
      </div>
    </div>
  );
}

function TypingBubble() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col items-start gap-0.5" aria-live="polite" aria-label={t("chat.typing_bubble")}>
      <span className="text-xs text-muted-foreground">{t("chat.visitor")}</span>
      <div className="flex items-center gap-1 rounded-lg bg-muted px-3 py-2">
        <span className="size-1.5 rounded-full bg-muted-foreground/70 motion-safe:animate-bounce motion-safe:[animation-delay:-0.3s]" />
        <span className="size-1.5 rounded-full bg-muted-foreground/70 motion-safe:animate-bounce motion-safe:[animation-delay:-0.15s]" />
        <span className="size-1.5 rounded-full bg-muted-foreground/70 motion-safe:animate-bounce" />
      </div>
    </div>
  );
}

function Conversation({ s, onClosed }: { s: ChatSession; onClosed: () => void }) {
  const { t, tCode } = useI18n();
  const [msgs, setMsgs] = useState<ChatMessage[]>([]);
  const [st, setSt] = useState(s.status);
  const [body, setBody] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const msgsRef = useRef(msgs);
  useEffect(() => { msgsRef.current = msgs; });
  const me = useMe();
  const canManage = me.data?.permissions.includes("tickets.manage") === true;
  const live = useLiveChat(canManage ? `/admin/chats/${s.id}/ws` : null, { csrf: me.data?.csrf_token }, (event) => {
    setSt(event.status);
    if (event.messages?.length) setMsgs((p) => mergeMessages(p, event.messages));
  }, () => lastId(msgsRef.current));
  const fetchNew = useCallback(async () => {
    const r = await api<{ status: "open" | "closed"; messages: ChatMessage[] }>(`/admin/chats/${s.id}/messages?after=${lastId(msgsRef.current)}`);
    setSt(r.status);
    if (r.messages.length) setMsgs((p) => mergeMessages(p, r.messages));
  }, [s.id]);
  useEffect(() => {
    void fetchNew().catch(() => undefined);
    if (live.connected) return;
    const h = window.setInterval(() => void fetchNew().catch(() => undefined), 15000);
    return () => window.clearInterval(h);
  }, [fetchNew, live.connected]);
  const fail = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const send = async () => {
    if (!canManage || !validBody(body)) return;
    setErr(null); setBusy(true);
    try {
      await api(`/admin/chats/${s.id}/messages`, { method: "POST", body: { body } });
      setBody("");
      live.sendTyping(false);
      if (!live.connected) await fetchNew();
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const close = async () => {
    if (!canManage) return;
    try { await api(`/admin/chats/${s.id}/close`, { method: "POST" }); await fetchNew(); onClosed(); } catch (e) { fail(e); }
  };
  return (
    <Card className="flex flex-col">
      <CardHeader className="flex flex-row items-start justify-between gap-2">
        <div className="flex flex-col gap-1"><CardTitle>{s.name}</CardTitle><CardDescription>{s.email}{s.page && ` · ${s.page}`}</CardDescription></div>
        {canManage && st === "open" && <Button variant="outline" size="sm" onClick={close}><XCircleIcon /> {t("chat.admin.close")}</Button>}
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <output className="block text-xs">{t(live.online ? "chat.online" : "chat.offline")}</output>
        <div className="flex max-h-[28rem] flex-col gap-2 overflow-y-auto">
          {msgs.map((m) => <Bubble key={m.id} m={m} mine={m.author === "staff"} />)}
          {live.typing && st === "open" && <TypingBubble />}
        </div>
        {canManage && st === "open" ? (
          <div className="flex flex-col gap-2">
            <LiveChatSuggest sessionId={s.id} onSent={() => { if (!live.connected) void fetchNew().catch(() => undefined); }} onError={fail} />
            <Textarea
              value={body}
              maxLength={4000}
              rows={3}
              placeholder={t("chat.placeholder")}
              onChange={(e) => { setBody(e.target.value); live.sendTyping(Boolean(e.target.value)); }}
              onKeyDown={(e) => chatComposerKeyDown(e, () => { if (!busy && validBody(body)) void send(); })}
            />
            <Button className="self-end" disabled={busy || !validBody(body)} onClick={() => void send()}><PaperPlaneRightIcon /> {t("chat.send")}</Button>
          </div>
        ) : st === "closed" ? <p className="text-sm text-muted-foreground">{t("chat.closed")}</p> : null}
        {canManage && <LiveChatActions session={s} onChanged={onClosed} />}
        {err && <p className="text-sm text-destructive" role="alert">{err}</p>}
      </CardContent>
    </Card>
  );
}
