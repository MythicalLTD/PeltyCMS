import type { ReactNode } from "react";
import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { BookOpenTextIcon, CaretRightIcon, FileTextIcon, HardDrivesIcon, LifebuoyIcon, MegaphoneIcon, type Icon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { StatusBadge } from "@/components/StatusBadge";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { api } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

export interface Home {
  owner: boolean;
  services: { id: string; product_name: string; status: string; currency: string; recurring_amount: string; next_due_date: string | null }[] | null;
  invoices: { id: string; number: string | null; currency: string; total: string; amount_paid: string; due_at: string | null; overdue: boolean }[] | null;
  tickets: { id: string; number: number; subject: string; status: string }[] | null;
  announcements: { id: string; title: string; excerpt: string; published_at: string }[];
  kb: { id: string; slug: string; title: string }[];
}

function Widget({ icon: I, title, to, empty, children }: { icon: Icon; title: string; to: string; empty: boolean; children: ReactNode }) {
  const { t } = useI18n();
  return (
    <Card className="shadow-sm">
      <CardHeader>
        <CardTitle className="flex min-w-0 items-center gap-2"><I weight="duotone" className="size-5 text-primary" aria-hidden />{title}</CardTitle>
        <CardAction><Button asChild variant="ghost" size="sm"><Link to={to}>{t("common.view_all")}<CaretRightIcon aria-hidden /></Link></Button></CardAction>
      </CardHeader>
      <CardContent>{empty ? <p className="text-sm text-muted-foreground">{t("home.nothing")}</p> : <ul className="flex flex-col divide-y">{children}</ul>}</CardContent>
    </Card>
  );
}

const row = "flex min-h-11 items-center justify-between gap-3 py-2 text-sm";

/** Client home widgets, each shown only when the viewer may see that area, then plugin widgets. */
export function HomeWidgets() {
  const { t, lang } = useI18n();
  const { data } = useQuery({ queryKey: ["client", "home", lang], queryFn: () => api<Home>(`/client/home?locale=${lang}`) });
  if (!data) return null;
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 *:min-w-0 md:grid-cols-2 xl:grid-cols-3">
        {data.services && (
          <Widget icon={HardDrivesIcon} title={t("home.active_services")} to="/services" empty={data.services.length === 0}>
            {data.services.map((s) => <li key={s.id}><Link to={`/services/${s.id}`} className={`${row} hover:bg-accent/50`}><span className="min-w-0 truncate">{s.product_name}</span><span className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">{s.next_due_date && formatDate(s.next_due_date, lang)}<StatusBadge status={s.status} /></span></Link></li>)}
          </Widget>
        )}
        {data.invoices && (
          <Widget icon={FileTextIcon} title={t("home.unpaid_invoices")} to="/invoices" empty={data.invoices.length === 0}>
            {data.invoices.map((i) => (
              <li key={i.id}><Link to={`/invoices/${i.id}`} className={`${row} hover:bg-accent/50`}>
                <span>{i.number ?? t("billing.draft_invoice")}{i.overdue && <span className="ml-2 text-xs text-destructive">{t("home.overdue")}</span>}</span>
                <span className="tabular-nums">{formatMoney((Number(i.total) - Number(i.amount_paid)).toFixed(2), i.currency, lang)}</span>
              </Link></li>
            ))}
          </Widget>
        )}
        {data.tickets && (
          <Widget icon={LifebuoyIcon} title={t("home.open_tickets")} to="/tickets" empty={data.tickets.length === 0}>
            {data.tickets.map((k) => <li key={k.id}><Link to={`/tickets/${k.id}`} className={`${row} hover:bg-accent/50`}><span className="min-w-0 truncate">#{k.number} {k.subject}</span><StatusBadge status={k.status} /></Link></li>)}
          </Widget>
        )}
        <Widget icon={MegaphoneIcon} title={t("nav.announcements")} to="/announcements" empty={data.announcements.length === 0}>
          {data.announcements.map((a) => <li key={a.id} className="grid gap-0.5 py-2 text-sm"><span className="font-medium">{a.title}</span><span className="line-clamp-2 text-xs text-muted-foreground">{a.excerpt}</span></li>)}
        </Widget>
        <Widget icon={BookOpenTextIcon} title={t("home.kb_suggestions")} to="/kb" empty={data.kb.length === 0}>
          {data.kb.map((a) => <li key={a.id}><Link to={`/kb/${a.slug}`} className={`${row} hover:bg-accent/50`}>{a.title}<CaretRightIcon aria-hidden className="text-muted-foreground" /></Link></li>)}
        </Widget>
      </div>
      <PluginSlot slot="client.home" />
    </div>
  );
}
