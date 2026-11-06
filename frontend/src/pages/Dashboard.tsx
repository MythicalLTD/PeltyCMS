import { Link } from "react-router";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { WarningIcon, CalendarIcon, FileTextIcon, KeyIcon, HardDrivesIcon, ReceiptIcon, CaretRightIcon, ShoppingCartIcon, LifebuoyIcon, BookOpenIcon, GlobeIcon, NotePencilIcon, DotsSixVerticalIcon, EyeIcon, EyeSlashIcon, ArrowsInLineHorizontalIcon, ArrowsOutLineHorizontalIcon, SquaresFourIcon, CheckIcon, ArrowCounterClockwiseIcon, type Icon } from "@phosphor-icons/react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Stat, type CurrencyTotal } from "@/components/Stat";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { EmptyState } from "@/components/EmptyState";
import { StatusBadge } from "@/components/StatusBadge";
import { WelcomeHero } from "@/components/admin/WelcomeHero";
import { useMe } from "@/lib/auth";
import { api, type Paged } from "@/lib/api";
import { formatDate, formatMoney, type Invoice } from "@/lib/billing/billing";
import { formatNumber } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { RevenueChart, type MonthRevenue } from "@/components/billing/RevenueChart";
import { PluginWidgets } from "@/components/PluginWidgets";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { HomeWidgets } from "@/components/admin/HomeWidgets";
import { normalizeLayout, patchWidget, reorder, type DashboardLayout, type WidgetId } from "@/lib/client/dashboardLayout";
import type { TranslationKey } from "@/lib/i18n-core";
import { cn } from "@/lib/utils";

interface Upcoming { id: string; product_name: string; next_due_date: string; currency: string; recurring_amount: string }
interface ClientDashboard { active_services: number; unpaid_invoices: number; overdue_invoices: number; due: CurrencyTotal[]; upcoming: Upcoming[]; spend_series: MonthRevenue[] }

function RecentInvoices() {
  const { t, lang } = useI18n();
  const { data } = useQuery({ queryKey: ["client", "invoices", "recent"], queryFn: () => api<Paged<Invoice>>("/client/invoices?per_page=5") });
  return (
    <Card className="shadow-sm">
      <CardHeader>
        <CardTitle>{t("dashboard.recent_invoices")}</CardTitle>
        <CardDescription>{t("dashboard.recent_invoices_hint")}</CardDescription>
        <CardAction><Button asChild variant="ghost" size="sm"><Link to="/invoices">{t("common.view_all")}<CaretRightIcon aria-hidden /></Link></Button></CardAction>
      </CardHeader>
      <CardContent>
        {data && data.items.length === 0 ? (
          <EmptyState icon={ReceiptIcon} title={t("billing.no_invoices")} />
        ) : (
          <ul className="flex flex-col divide-y">
            {(data?.items ?? []).map((i) => (
              <li key={i.id}>
                <Link to={`/invoices/${i.id}`} className="flex min-h-14 items-center gap-3 rounded-md px-1 py-2 hover:bg-accent/50">
                  <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground"><ReceiptIcon weight="duotone" className="size-5" aria-hidden /></div>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">{i.number ?? t("billing.draft_invoice")}</p>
                    <p className="text-xs text-muted-foreground">{i.due_at ? t("dashboard.due_on", { date: formatDate(i.due_at, lang) }) : formatDate(i.created_at, lang)}</p>
                  </div>
                  <div className="flex flex-col items-end gap-1">
                    <span className="text-sm font-semibold tabular-nums">{formatMoney(i.total, i.currency, lang)}</span>
                    <StatusBadge status={i.status} />
                  </div>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

const QUICK: { to: string; label: "dashboard.qa_order" | "dashboard.qa_ticket" | "dashboard.qa_domains" | "dashboard.qa_kb"; icon: Icon }[] = [
  { to: "/order", label: "dashboard.qa_order", icon: ShoppingCartIcon },
  { to: "/tickets/new", label: "dashboard.qa_ticket", icon: LifebuoyIcon },
  { to: "/domains", label: "dashboard.qa_domains", icon: GlobeIcon },
  { to: "/kb", label: "dashboard.qa_kb", icon: BookOpenIcon },
];

function PendingQuotesCallout() {
  const { t } = useI18n();
  const { data } = useQuery({
    queryKey: ["client", "quotes", "pending"],
    queryFn: () => api<{ items: { id: string; number: number; status: string }[] }>("/client/quotes?per_page=25"),
  });
  const items = (data?.items ?? []).filter((q) => q.status === "sent");
  if (items.length === 0) return null;
  return (
    <Alert>
      <NotePencilIcon aria-hidden />
      <AlertTitle>{t("dashboard.quotes_pending", { n: items.length })}</AlertTitle>
      <AlertDescription className="flex flex-wrap items-center gap-2">
        <span>{t("dashboard.quotes_pending_hint")}</span>
        <Button asChild size="sm" variant="outline"><Link to="/quotes">{t("nav.quotes")}</Link></Button>
      </AlertDescription>
    </Alert>
  );
}

/** Shortcut tiles (client-area style): big targets, works as a 2x2 grid on phones. */
function QuickActions() {
  const { t } = useI18n();
  return (
    <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
      {QUICK.map(({ to, label, icon: I }) => (
        <Link key={to} to={to} className="flex min-h-20 flex-col items-start justify-between gap-2 rounded-xl border bg-card p-4 shadow-sm hover:border-primary">
          <I weight="duotone" className="size-6 text-primary" aria-hidden />
          <span className="text-sm font-medium">{t(label)}</span>
        </Link>
      ))}
    </div>
  );
}

/** Client-arrangeable dashboard: drag to reorder, hide, half/full width; saved per user. */
function ArrangeableWidgets({ widgets }: { widgets: Record<WidgetId, React.ReactNode> }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["client", "dashboard", "layout"], queryFn: async () => normalizeLayout(await api<DashboardLayout>("/client/dashboard/layout").catch(() => null)) });
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<DashboardLayout | null>(null);
  const [drag, setDrag] = useState<WidgetId | null>(null);
  const [over, setOver] = useState<WidgetId | null>(null);
  const save = useMutation({
    mutationFn: (l: DashboardLayout) => api<DashboardLayout>("/client/dashboard/layout", { method: "PUT", body: l }),
    onSuccess: (l) => { qc.setQueryData(["client", "dashboard", "layout"], normalizeLayout(l)); setEditing(false); setDraft(null); toast.success(t("common.saved")); },
    onError: () => toast.error(t("error.internal")),
  });
  const reset = useMutation({
    mutationFn: () => api<DashboardLayout>("/client/dashboard/layout", { method: "DELETE" }),
    onSuccess: (l) => { qc.setQueryData(["client", "dashboard", "layout"], normalizeLayout(l)); setDraft(normalizeLayout(l)); },
  });
  const layout = draft ?? q.data ?? normalizeLayout(null);
  const label = (id: WidgetId) => t(`dashboard.widget.${id}` as TranslationKey);
  return (
    <>
      <div className="flex flex-wrap items-center justify-end gap-2">
        {editing ? (
          <>
            <span className="mr-auto text-sm text-muted-foreground">{t("dashboard.customize_hint")}</span>
            <Button size="sm" variant="ghost" onClick={() => reset.mutate()} disabled={reset.isPending}><ArrowCounterClockwiseIcon aria-hidden />{t("dashboard.customize_reset")}</Button>
            <Button size="sm" variant="outline" onClick={() => { setEditing(false); setDraft(null); }}>{t("common.cancel")}</Button>
            <Button size="sm" data-testid="layout-save" onClick={() => save.mutate(layout)} disabled={save.isPending}><CheckIcon aria-hidden />{t("common.save")}</Button>
          </>
        ) : (
          <Button size="sm" variant="ghost" data-testid="layout-customize" onClick={() => { setDraft(layout); setEditing(true); }}><SquaresFourIcon aria-hidden />{t("dashboard.customize")}</Button>
        )}
      </div>
      <div className="grid gap-4 *:min-w-0 lg:grid-cols-2" data-testid="dashboard-widgets">
        {layout.widgets.filter((w) => editing || w.visible).map((w) => (
          <div
            key={w.id}
            data-widget={w.id}
            onDragOver={editing ? (e) => { e.preventDefault(); setOver(w.id); } : undefined}
            onDrop={editing ? (e) => { e.preventDefault(); if (drag) setDraft(reorder(layout, drag, w.id)); setDrag(null); setOver(null); } : undefined}
            className={cn(w.size === "full" ? "lg:col-span-2" : "", "*:h-full", editing && "relative rounded-xl border-2 border-dashed p-2", editing && over === w.id && drag !== w.id && "border-primary", editing && !w.visible && "opacity-40", drag === w.id && "opacity-50")}
          >
            {editing && (
              <div className="mb-2 flex items-center gap-1 !h-auto">
                <span draggable onDragStart={(e) => { e.dataTransfer.setData("text/plain", w.id); e.dataTransfer.effectAllowed = "move"; setDrag(w.id); }} onDragEnd={() => { setDrag(null); setOver(null); }} className="flex cursor-grab items-center gap-1 text-sm font-medium" data-testid="widget-handle"><DotsSixVerticalIcon size={18} aria-hidden />{label(w.id)}</span>
                <span className="ml-auto flex gap-1">
                  <Button size="icon" variant="ghost" className="size-8" aria-label={w.size === "full" ? t("dashboard.widget_half") : t("dashboard.widget_full")} onClick={() => setDraft(patchWidget(layout, w.id, { size: w.size === "full" ? "half" : "full" }))}>{w.size === "full" ? <ArrowsInLineHorizontalIcon /> : <ArrowsOutLineHorizontalIcon />}</Button>
                  <Button size="icon" variant="ghost" className="size-8" data-testid="widget-visibility" aria-label={w.visible ? t("dashboard.widget_hide") : t("dashboard.widget_show")} onClick={() => setDraft(patchWidget(layout, w.id, { visible: !w.visible }))}>{w.visible ? <EyeIcon /> : <EyeSlashIcon />}</Button>
                </span>
              </div>
            )}
            {editing ? <div className="pointer-events-none select-none">{widgets[w.id]}</div> : widgets[w.id]}
          </div>
        ))}
      </div>
    </>
  );
}

export function DashboardPage() {
  const { t, tCode, lang } = useI18n();
  const { data: me } = useMe();
  const { data: access } = useQuery({ queryKey: ["client", "access"], queryFn: () => api<{ owner: boolean }>("/client/access") });
  const { data, isLoading } = useQuery({ queryKey: ["client", "dashboard"], queryFn: () => api<ClientDashboard>("/client/dashboard"), enabled: !!access?.owner });
  if (access && !access.owner) {
    // Sub-account users: only the widgets for the areas they were granted.
    return (
      <div className="flex flex-col gap-6">
        <WelcomeHero title={t("dashboard.welcome", { name: me?.display_name || me?.email || "" })} subtitle={t("home.subaccount_hint")} />
        <HomeWidgets />
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-6">
      <WelcomeHero
        title={t("dashboard.welcome", { name: me?.display_name || me?.email || "" })}
        subtitle={t("dashboard.hero_subtitle")}
        badges={me?.roles.map((r) => <Badge key={r} variant="info">{tCode(`role.${r}`)}</Badge>)}
      >
        <Button asChild variant={data?.unpaid_invoices ? "default" : "outline"}><Link to="/invoices"><FileTextIcon aria-hidden />{t("dashboard.pay_invoices")}</Link></Button>
        <Button asChild variant="outline"><Link to="/services"><HardDrivesIcon aria-hidden />{t("dashboard.view_services")}</Link></Button>
        <Button asChild variant="ghost"><Link to="/sessions"><KeyIcon aria-hidden />{t("dashboard.security")}</Link></Button>
      </WelcomeHero>
      <ArrangeableWidgets widgets={{
        quick_actions: <Replaceable section="quick-actions"><QuickActions /></Replaceable>,
        quotes: <PendingQuotesCallout />,
        addons: <><PluginWidgets slot="client.dashboard" /><PluginSlot slot="client.dashboard" /></>,
        stats: <Replaceable section="stats"><div className="grid gap-3 *:min-w-0 sm:grid-cols-3 sm:gap-4">
          <Stat icon={HardDrivesIcon} label={t("dashboard.active_services")} value={data ? formatNumber(data.active_services, lang) : "…"} to="/services" tone="success" />
          <Stat icon={FileTextIcon} label={t("dashboard.unpaid_invoices")} value={data ? formatNumber(data.unpaid_invoices, lang) : "…"} to="/invoices"
            tone={data?.unpaid_invoices ? "warning" : "primary"}
            sub={data && data.due.length > 0 && t("dashboard.total_due", { amount: data.due.map((c) => formatMoney(c.amount, c.currency, lang)).join(" · ") })} />
          <Stat icon={WarningIcon} label={t("dashboard.overdue")} value={data ? formatNumber(data.overdue_invoices, lang) : "…"} to="/invoices" tone={data?.overdue_invoices ? "danger" : "primary"} />
        </div></Replaceable>,
        spend_chart: <RevenueChart series={data?.spend_series ?? []} title={t("dashboard.spend_chart")} hint={t("dashboard.spend_chart_hint")} />,
        recent_invoices: <RecentInvoices />,
        home: <HomeWidgets />,
        upcoming: <Replaceable section="upcoming"><Card className="shadow-sm">
          <CardHeader className="flex flex-row items-center gap-2"><CalendarIcon weight="duotone" className="size-5 text-primary" aria-hidden /><CardTitle>{t("dashboard.next_due")}</CardTitle></CardHeader>
          <CardContent>
            {data && data.upcoming.length === 0 ? (
              <EmptyState icon={HardDrivesIcon} title={t("dashboard.no_services")} description={t("dashboard.no_services_hint")} />
            ) : (
              <ResponsiveTable<Upcoming> rows={data?.upcoming} rowKey={(u) => u.id} loading={isLoading} columns={[
                { header: t("billing.product"), primary: true, cell: (u) => <Link className="font-medium hover:underline" to={`/services/${u.id}`}>{u.product_name}</Link> },
                { header: t("billing.next_due"), cell: (u) => formatDate(u.next_due_date, lang) },
                { header: t("billing.amount"), cell: (u) => formatMoney(u.recurring_amount, u.currency, lang) },
              ]} />
            )}
          </CardContent>
        </Card></Replaceable>,
      }} />
    </div>
  );
}
