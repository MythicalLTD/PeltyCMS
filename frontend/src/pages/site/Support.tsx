import { useState } from "react";
import { Link, useNavigate } from "react-router";
import {
  BookOpenTextIcon, ChatCircleDotsIcon, EnvelopeSimpleIcon, LifebuoyIcon, MagnifyingGlassIcon, MegaphoneIcon, PulseIcon, TicketIcon, type Icon,
} from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { openChat, useLiveChatEnabled } from "@/components/support/ChatWidget";
import { useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

interface Entry { icon: Icon; title: TranslationKey; desc: TranslationKey; to?: string; onClick?: () => void; testid: string }

/** `/support`: one place for the knowledgebase, tickets, live chat, status and announcements, in every public mode. */
export function SupportPage() {
  const { t } = useI18n();
  const nav = useNavigate();
  const { data: me } = useMe();
  const chat = useLiveChatEnabled();
  const [q, setQ] = useState("");
  const entries: Entry[] = [
    { icon: BookOpenTextIcon, title: "pub.support_kb", desc: "pub.support_kb_desc", to: "/kb", testid: "kb" },
    { icon: TicketIcon, title: "pub.support_ticket", desc: me ? "pub.support_ticket_desc" : "pub.support_ticket_login", to: me ? "/tickets?new=1" : "/login", testid: "ticket" },
    ...(me ? [{ icon: LifebuoyIcon, title: "pub.support_my_tickets" as TranslationKey, desc: "pub.support_my_tickets_desc" as TranslationKey, to: "/tickets", testid: "my-tickets" }] : []),
    ...(chat ? [{ icon: ChatCircleDotsIcon, title: "pub.support_chat" as TranslationKey, desc: "pub.support_chat_desc" as TranslationKey, onClick: openChat, testid: "chat" }] : []),
    { icon: PulseIcon, title: "pub.support_status", desc: "pub.support_status_desc", to: "/status", testid: "status" },
    { icon: MegaphoneIcon, title: "pub.support_news", desc: "pub.support_news_desc", to: "/announcements", testid: "news" },
    { icon: EnvelopeSimpleIcon, title: "pub.support_contact", desc: "pub.support_contact_desc", to: "/contact", testid: "contact" },
  ];
  return (
    <div className="flex flex-col gap-8">
      <section className="flex flex-col items-center gap-4 rounded-lg border bg-card px-4 py-10 text-center">
        <LifebuoyIcon aria-hidden className="size-10 text-primary" />
        <h1 className="text-3xl font-bold tracking-tight">{t("pub.support_title")}</h1>
        <p className="max-w-xl text-muted-foreground">{t("pub.support_subtitle")}</p>
        <search className="w-full max-w-xl"><form className="relative flex w-full gap-2" onSubmit={(e) => { e.preventDefault(); nav(q.trim() ? `/kb?q=${encodeURIComponent(q.trim())}` : "/kb"); }}>
          <MagnifyingGlassIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input className="h-11 pl-9" maxLength={200} placeholder={t("content.kb_search")} aria-label={t("content.kb_search")} value={q} onChange={(e) => setQ(e.target.value)} />
          <Button type="submit" className="h-11">{t("pub.search")}</Button>
        </form></search>
      </section>
      <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {entries.map((e) => {
          const body = (
            <Card className="h-full transition hover:border-primary/60">
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-base"><e.icon aria-hidden className="size-5 text-primary" />{t(e.title)}</CardTitle>
                <CardDescription>{t(e.desc)}</CardDescription>
              </CardHeader>
            </Card>
          );
          return (
            <li key={e.testid} data-testid={`support-${e.testid}`}>
              {e.to
                ? <Link to={e.to} className="block h-full rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">{body}</Link>
                : <button type="button" onClick={e.onClick} className="block h-full w-full rounded-lg text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">{body}</button>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
