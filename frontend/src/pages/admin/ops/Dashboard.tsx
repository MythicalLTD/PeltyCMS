import { UpdateBanner } from "@/components/admin/UpdateCheck";
import { LicenseOverageBanner } from "@/components/admin/LicenseGate";
import { SetupWizardBanner } from "@/pages/admin/settings/GettingStarted";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { auditLabel } from "@/lib/admin/audit";
import { WarningIcon, MoneyIcon, FileTextIcon, ListChecksIcon, HardDrivesIcon, ShoppingCartIcon, ClockCounterClockwiseIcon, CaretRightIcon, UsersIcon, ReceiptIcon } from "@phosphor-icons/react";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { UserAvatar } from "@/components/account/UserAvatar";
import { EmptyState } from "@/components/EmptyState";
import { WelcomeHero } from "@/components/admin/WelcomeHero";
import { useMe } from "@/lib/auth";
import type { Paged } from "@/lib/api";
import { Stat, type CurrencyTotal } from "@/components/Stat";
import { RevenueChart, type MonthRevenue } from "@/components/billing/RevenueChart";
import { PluginWidgets } from "@/components/PluginWidgets";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { formatNumber, formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { CustomizeDashboard, QuickLinksCard, StaffOnlineCard, TodosCard, UpcomingCard, usePrefs } from "@/pages/admin/ops/AdminOps";
import { HealthWidget } from "@/pages/admin/ops/AdminTools";
import type { Widget } from "@/lib/admin/adminOps";

interface AdminDashboard { revenue_month: CurrencyTotal[]; unpaid: CurrencyTotal[]; overdue: CurrencyTotal[]; active_services: number; pending_orders: number; failed_jobs: number; revenue_series: MonthRevenue[] }

interface AuditEntry { id: number; actor_id: string | null; actor_email: string | null; actor_name: string | null; actor_avatar_version: number | null; action: string; created_at: string }

function RecentActivity() {
  const { t, tCode, lang } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "audit", "recent"], queryFn: () => api<Paged<AuditEntry>>("/admin/audit-logs?per_page=6"), retry: false });
  return (
    <Card className="h-full shadow-sm">
      <CardHeader>
        <CardTitle>{t("admin.dashboard.recent_activity")}</CardTitle>
        <CardDescription>{t("admin.dashboard.recent_activity_hint")}</CardDescription>
        <CardAction><Button asChild variant="ghost" size="sm"><Link to="/admin/audit">{t("common.view_all")}<CaretRightIcon aria-hidden /></Link></Button></CardAction>
      </CardHeader>
      <CardContent>
        {data && data.items.length === 0 ? <EmptyState icon={ClockCounterClockwiseIcon} title={t("common.no_data")} /> : (
          <ul className="flex flex-col gap-4">
            {(data?.items ?? []).map((e) => (
              <li key={e.id} className="flex items-start gap-3">
                <UserAvatar size="sm" className="mt-0.5 size-8" userId={e.actor_id} version={e.actor_avatar_version} name={e.actor_name ?? (e.actor_id ? null : t("admin.audit.system"))} email={e.actor_email} />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm"><span className="font-medium">{e.actor_name || e.actor_email || t("admin.audit.system")}</span></p>
                  <p className="truncate text-xs text-muted-foreground">{auditLabel(tCode, e.action)}</p>
                </div>
                <time className="shrink-0 text-xs text-muted-foreground">{formatDateTime(e.created_at, lang)}</time>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

export function AdminDashboardPage() {
  const { data: me } = useMe();
  const { t, lang } = useI18n();
  const prefs = usePrefs();
  const { data } = useQuery({ queryKey: ["admin", "dashboard"], queryFn: () => api<AdminDashboard>("/admin/dashboard") });
  const money = (l: CurrencyTotal[] | undefined) => !l ? "…" : l.length === 0 ? "0" : l.map((c) => formatMoney(c.amount, c.currency, lang)).join(" · ");
  const count = (l: CurrencyTotal[] | undefined) => (l ?? []).reduce((n, c) => n + c.count, 0);
  const n = (v: number | undefined) => (v === undefined ? "…" : formatNumber(v, lang));
  return (
    <div className="flex flex-col gap-6">
      <UpdateBanner />
      <LicenseOverageBanner />
      <WelcomeHero title={t("dashboard.welcome", { name: me?.display_name || me?.email || "" })} subtitle={t("admin.dashboard.hero_subtitle")}>
        <Button asChild><Link to="/admin/invoices"><ReceiptIcon aria-hidden />{t("nav.invoices")}</Link></Button>
        <Button asChild variant="outline"><Link to="/admin/customers"><UsersIcon aria-hidden />{t("nav.customers")}</Link></Button>
        {prefs.data && <CustomizeDashboard prefs={prefs.data} />}
      </WelcomeHero>
      <SetupWizardBanner />
      <PluginWidgets slot="admin.dashboard" />
      <PluginSlot slot="admin.dashboard" />
      {(prefs.data?.widgets ?? []).map((w: Widget) => {
        switch (w) {
          case "stats": return (
            <Replaceable key={w} section="stats"><div className="grid gap-3 *:min-w-0 sm:grid-cols-2 sm:gap-4 lg:grid-cols-3">
        <Stat icon={MoneyIcon} label={t("admin.dashboard.revenue_month")} value={money(data?.revenue_month)} tone="success" sub={data && t("admin.dashboard.payments", { count: count(data.revenue_month) })} />
        <Stat icon={FileTextIcon} label={t("admin.dashboard.unpaid")} value={n(data && count(data.unpaid))} sub={data?.unpaid.length ? money(data.unpaid) : undefined} to="/admin/invoices" tone={data && count(data.unpaid) ? "warning" : "primary"} />
        <Stat icon={WarningIcon} label={t("admin.dashboard.overdue")} value={n(data && count(data.overdue))} sub={data?.overdue.length ? money(data.overdue) : undefined} to="/admin/invoices" tone={data && count(data.overdue) ? "danger" : "primary"} />
        <Stat icon={HardDrivesIcon} label={t("admin.dashboard.active_services")} value={n(data?.active_services)} to="/admin/services" />
        <Stat icon={ShoppingCartIcon} label={t("admin.dashboard.pending_orders")} value={n(data?.pending_orders)} to="/admin/orders" tone={data?.pending_orders ? "warning" : "primary"} />
        <Stat icon={ListChecksIcon} label={t("admin.dashboard.failed_jobs")} value={n(data?.failed_jobs)} to="/admin/jobs" tone={data?.failed_jobs ? "danger" : "primary"} />
      </div></Replaceable>
          );
          case "revenue": return <Replaceable key={w} section="revenue"><RevenueChart series={data?.revenue_series ?? []} /></Replaceable>;
          case "activity": return <Replaceable key={w} section="activity"><RecentActivity /></Replaceable>;
          default: return null;
        }
      })}
      <div className="grid gap-4 *:min-w-0 md:grid-cols-2 xl:grid-cols-3">
        {(prefs.data?.widgets ?? []).map((w: Widget) => {
          switch (w) {
            case "staff_online": return <StaffOnlineCard key={w} />;
            case "quick_links": return <QuickLinksCard key={w} links={prefs.data?.quick_links ?? []} />;
            case "todos": return <TodosCard key={w} />;
            case "calendar": return <UpcomingCard key={w} />;
            case "health": return <HealthWidget key={w} />;
            default: return null;
          }
        })}
      </div>
    </div>
  );
}
