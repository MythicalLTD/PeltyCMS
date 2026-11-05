import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useLocation } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { useLiveChat } from "@/hooks/useLiveChat";
import { ChatsCircleIcon, PaperPlaneRightIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NativeSelect } from "@/components/ui/native-select";
import { Textarea } from "@/components/ui/textarea";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { api, ApiError } from "@/lib/api";
import { usePublicGeneral } from "@/lib/admin/general";
import { useI18n } from "@/lib/i18n";
import {
  CHAT_KEY,
  chatComposerKeyDown,
  lastId,
  loadCreds,
  mergeMessages,
  parseChatAction,
  validBody,
  type ChatCreds,
  type ChatMessage,
} from "@/lib/support/liveChat";

export function ChatWidget() {
  const g = usePublicGeneral();
  if (!g.data?.support?.live_chat) return null;
  return <ChatPanel />;
}

function TypingBubble() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col items-start gap-0.5" aria-live="polite" aria-label={t("chat.typing_bubble")}>
      <span className="text-xs text-muted-foreground">{t("chat.staff")}</span>
      <div className="flex items-center gap-1 rounded-lg bg-muted px-3 py-2">
        <span className="size-1.5 rounded-full bg-muted-foreground/70 motion-safe:animate-bounce motion-safe:[animation-delay:-0.3s]" />
        <span className="size-1.5 rounded-full bg-muted-foreground/70 motion-safe:animate-bounce motion-safe:[animation-delay:-0.15s]" />
        <span className="size-1.5 rounded-full bg-muted-foreground/70 motion-safe:animate-bounce" />
      </div>
    </div>
  );
}

function ChatPanel() {
  const { t, tCode } = useI18n();
  const loc = useLocation();
  const [open, setOpen] = useState(false);
  const [creds, setCreds] = useState<ChatCreds | null>(() => (typeof localStorage === "undefined" ? null : loadCreds(localStorage)));
  const [msgs, setMsgs] = useState<ChatMessage[]>([]);
  const [status, setStatus] = useState<"open" | "closed">("open");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [body, setBody] = useState("");
  const [department, setDepartment] = useState("");
  const availability = useQuery({ queryKey: ["chat", "availability"], queryFn: () => api<{ enabled: boolean; open: boolean; departments: { id: string; name: string }[] }>("/chat/availability"), refetchInterval: 60000 });
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const msgsRef = useRef(msgs);
  const credsRef = useRef(creds);
  useEffect(() => {
    msgsRef.current = msgs;
    credsRef.current = creds;
  });
  const endRef = useRef<HTMLDivElement>(null);
  const pollActiveRef = useRef(false);
  const live = useLiveChat(open && creds ? `/chat/sessions/${creds.id}/ws` : null, { token: creds?.token }, (event) => {
    setStatus(event.status);
    if (event.messages?.length) setMsgs((p) => mergeMessages(p, event.messages));
  }, () => lastId(msgsRef.current));

  const forget = useCallback(() => { localStorage.removeItem(CHAT_KEY); setCreds(null); setMsgs([]); setStatus("open"); }, []);
  const fail = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));

  const pull = useCallback(async () => {
    const c = credsRef.current;
    if (!c) return;
    try {
      const r = await api<{ status: "open" | "closed"; messages: ChatMessage[] }>(`/chat/sessions/${c.id}/messages?after=${lastId(msgsRef.current)}`, { headers: { "x-chat-token": c.token } });
      setStatus(r.status);
      if (r.messages.length) setMsgs((p) => mergeMessages(p, r.messages));
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) forget();
    }
  }, [forget]);

  const pullRef = useRef(pull);
  useEffect(() => { pullRef.current = pull; }, [pull]);
  useEffect(() => {
    if (!open || !creds) return;
    pollActiveRef.current = true;
    void pullRef.current().finally(() => { /* one-shot on open */ });
    if (live.connected) return () => { pollActiveRef.current = false; };
    const h = window.setInterval(() => { if (pollActiveRef.current) void pullRef.current(); }, 15000);
    return () => { pollActiveRef.current = false; window.clearInterval(h); };
  }, [open, creds, live.connected]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  });

  const start = async () => {
    setErr(null); setBusy(true);
    try {
      const r = await api<ChatCreds>("/chat/sessions", { method: "POST", body: { name, email, message: body, page: loc.pathname, department_id: department || availability.data?.departments[0]?.id || null } });
      localStorage.setItem(CHAT_KEY, JSON.stringify(r));
      setCreds(r); setBody("");
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const send = async () => {
    if (!creds || !validBody(body)) return;
    setErr(null); setBusy(true);
    const text = body;
    try {
      await api(`/chat/sessions/${creds.id}/messages`, { method: "POST", body: { body: text }, headers: { "x-chat-token": creds.token } });
      setBody("");
      live.sendTyping(false);
      if (!live.connected) await pull();
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const end = async () => {
    if (!creds) return;
    try { await api(`/chat/sessions/${creds.id}/close`, { method: "POST", headers: { "x-chat-token": creds.token } }); setStatus("closed"); } catch (e) { fail(e); }
  };

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger asChild>
        <Button className="fixed right-4 bottom-4 z-40 rounded-full shadow-md" size="lg" aria-label={t("chat.open")}>
          <ChatsCircleIcon /> {t("chat.title")}
        </Button>
      </SheetTrigger>
      <SheetContent className="flex flex-col gap-3">
        <SheetHeader>
          <SheetTitle>{t("chat.title")}</SheetTitle>
          <SheetDescription>{t("chat.subtitle")}</SheetDescription>
          {creds && <output className="block text-xs">{t(live.online ? "chat.online" : "chat.offline")}</output>}
        </SheetHeader>
        <div className="flex flex-1 flex-col gap-3 overflow-hidden px-4 pb-4">
          {creds ? (
            <>
              <div className="flex flex-1 flex-col gap-2 overflow-y-auto" aria-live="polite">
                {msgs.map((m) => <Bubble key={m.id} m={m} mine={m.author === "visitor"} />)}
                {live.typing && status === "open" && <TypingBubble />}
                <div ref={endRef} />
              </div>
              {status === "closed" ? (
                <div className="flex flex-col gap-2">
                  <p className="text-sm text-muted-foreground">{t("chat.closed")}</p>
                  <Button variant="outline" onClick={forget}>{t("chat.new")}</Button>
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  <Textarea
                    value={body}
                    maxLength={4000}
                    rows={3}
                    placeholder={t("chat.placeholder")}
                    onChange={(e) => { setBody(e.target.value); live.sendTyping(Boolean(e.target.value)); }}
                    onKeyDown={(e) => chatComposerKeyDown(e, () => { if (!busy && validBody(body)) void send(); })}
                  />
                  <div className="flex gap-2">
                    <Button className="flex-1" disabled={busy || !validBody(body)} onClick={() => void send()}><PaperPlaneRightIcon /> {t("chat.send")}</Button>
                    <Button variant="outline" onClick={end}>{t("chat.end")}</Button>
                  </div>
                </div>
              )}
            </>
          ) : (
            <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); void start(); }}>
              <div className="flex flex-col gap-1.5"><Label htmlFor="chat-name">{t("chat.name")}</Label><Input id="chat-name" value={name} maxLength={120} onChange={(e) => setName(e.target.value)} required /></div>
              <div className="flex flex-col gap-1.5"><Label htmlFor="chat-email">{t("chat.email")}</Label><Input id="chat-email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} required /></div>
              <div className="flex flex-col gap-1.5"><Label htmlFor="chat-msg">{t("chat.message")}</Label><Textarea id="chat-msg" value={body} maxLength={4000} rows={4} onChange={(e) => setBody(e.target.value)} required /></div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="chat-department">{t("chat.department")}</Label>
                <NativeSelect id="chat-department" value={department || availability.data?.departments[0]?.id || ""} onChange={(e) => setDepartment(e.target.value)}>
                  {availability.data?.departments.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
                </NativeSelect>
              </div>
              {availability.data && !availability.data.open && <output className="block">{t("error.chat_unavailable")}</output>}
              <Button type="submit" disabled={!availability.data?.open || busy || !validBody(body) || !name.trim() || !email.trim()}>{t("chat.start")}</Button>
            </form>
          )}
          {err && <p className="text-sm text-destructive" role="alert">{err}</p>}
        </div>
      </SheetContent>
    </Sheet>
  );
}

export function Bubble({ m, mine }: { m: ChatMessage; mine: boolean }) {
  const { t, tCode } = useI18n();
  if (m.author === "system") {
    const action = parseChatAction(m.body);
    if (action) {
      return (
        <div className="flex flex-col items-start gap-1 self-stretch">
          <div className="max-w-[85%] rounded-lg border bg-muted/60 px-3 py-2 text-sm">
            <p className="font-medium">{action.label}</p>
            <Link className="mt-1 inline-flex text-primary underline" to={action.href} onClick={(e) => e.stopPropagation()}>{t("chat.action_open")}</Link>
          </div>
        </div>
      );
    }
    return <p className="text-center text-xs text-muted-foreground">{tCode(`chat.system.${m.body}`)}</p>;
  }
  return (
    <div className={`flex flex-col gap-0.5 ${mine ? "items-end" : "items-start"}`}>
      <span className="text-xs text-muted-foreground">{m.author === "staff" ? (m.staff_name ?? t("chat.staff")) : t("chat.visitor")}</span>
      <div className={`max-w-[85%] rounded-lg px-3 py-2 text-sm whitespace-pre-wrap break-words ${mine ? "bg-primary text-primary-foreground" : "bg-muted"}`}>{m.body}</div>
    </div>
  );
}
