import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Area, AreaChart, Bar, BarChart, CartesianGrid, ComposedChart, Line, XAxis, YAxis } from "recharts";
import { ChartLineUpIcon } from "@phosphor-icons/react";
import { TaxReportCard } from "@/components/billing/TaxReportCard";
import { ReportLibrary } from "@/components/admin/ReportLibrary";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { formatNumber } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { currenciesOf, lastMonths, monthsBetween, series, sumExact, type Report } from "@/lib/admin/reports";

const RANGES = ["6", "12", "24", "36"] as const;
const AXIS = "aspect-auto h-64 w-full text-sm [&_.recharts-cartesian-axis-tick_text]:fill-foreground/75";

export function AdminReportsPage() {
  const { t, lang } = useI18n();
  const [range, setRange] = useState<string>("12");
  const [cur, setCur] = useState<string | null>(null);
  const { from, to } = lastMonths(Number(range));
  const { data, isLoading } = useQuery({ queryKey: ["admin", "reports", from, to], queryFn: () => api<Report>(`/admin/reports?from=${from}&to=${to}`) });
  const currencies = data ? currenciesOf(data) : [];
  const currency = cur && currencies.includes(cur) ? cur : currencies[0] ?? "EUR";
  const months = monthsBetween(from, to);
  const locale = lang === "ro" ? "ro-RO" : "en-US";
  const monthLabel = (m: string) => new Date(`${m}-01T00:00:00Z`).toLocaleDateString(locale, { month: "short", year: "2-digit", timeZone: "UTC" });
  const money = (v: number) => new Intl.NumberFormat(locale, { style: "currency", currency, notation: "compact", maximumFractionDigits: 1 }).format(v);

  const rev = data ? series(data.revenue, months, currency) : [];
  const ref = data ? series(data.refunds, months, currency) : [];
  const mrr = data ? series(data.mrr, months, currency) : [];
  const revData = months.map((m, i) => ({ month: m, revenue: rev[i], refunds: ref[i] }));
  const mrrData = months.map((m, i) => ({ month: m, mrr: mrr[i] }));
  const churnData = months.map((m) => {
    const c = data?.churn.find((x) => x.month === m);
    return { month: m, churned: c?.churned ?? 0, gained: c?.gained ?? 0, rate: Number(c?.churn_rate ?? 0) * 100 };
  });
  const lastMrr = data?.mrr.filter((p) => p.currency === currency && p.month === to)[0]?.amount ?? "0";
  const churned = data?.churn.reduce((a, c) => a + c.churned, 0) ?? 0;
  const startAvg = data && data.churn.length ? data.churn.reduce((a, c) => a + Number(c.churn_rate), 0) / data.churn.length : 0;

  const revCfg: ChartConfig = { revenue: { label: t("reports.revenue"), color: "var(--chart-1)" }, refunds: { label: t("reports.refunds"), color: "var(--chart-4)" } };
  const mrrCfg: ChartConfig = { mrr: { label: t("reports.mrr"), color: "var(--chart-2)" } };
  const churnCfg: ChartConfig = { gained: { label: t("reports.gained"), color: "var(--chart-2)" }, churned: { label: t("reports.churned"), color: "var(--chart-5)" }, rate: { label: t("reports.churn_rate"), color: "var(--chart-1)" } };
  const tip = <ChartTooltipContent labelFormatter={(l) => monthLabel(String(l))} />;
  const x = <XAxis dataKey="month" tickLine={false} axisLine={false} tickFormatter={monthLabel} minTickGap={16} />;

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("reports.title")} description={t("reports.subtitle")} />
      <div className="flex flex-col gap-3 sm:flex-row">
        <div className="flex flex-col gap-2">
          <Label htmlFor="rep-range">{t("reports.range")}</Label>
          <Select value={range} onValueChange={setRange}>
            <SelectTrigger id="rep-range" className="w-full sm:w-44"><SelectValue /></SelectTrigger>
            <SelectContent>{RANGES.map((r) => <SelectItem key={r} value={r}>{t("reports.last_months", { count: r })}</SelectItem>)}</SelectContent>
          </Select>
        </div>
        {currencies.length > 1 && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="rep-cur">{t("reports.currency")}</Label>
            <Select value={currency} onValueChange={setCur}>
              <SelectTrigger id="rep-cur" className="w-full sm:w-32"><SelectValue /></SelectTrigger>
              <SelectContent>{currencies.map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent>
            </Select>
          </div>
        )}
      </div>
      {isLoading || !data ? (
        <div className="grid gap-4 *:min-w-0 md:grid-cols-3">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-28" />)}</div>
      ) : (
        <>
          <div className="grid gap-4 *:min-w-0 md:grid-cols-3">
            <Card><CardHeader><CardDescription>{t("reports.total_revenue")}</CardDescription><CardTitle className="text-2xl tabular-nums">{formatMoney(sumExact(data.revenue, currency), currency, lang)}</CardTitle></CardHeader>
              <CardContent className="text-sm text-muted-foreground">{t("reports.refunds")}: {formatMoney(sumExact(data.refunds, currency), currency, lang)}</CardContent></Card>
            <Card><CardHeader><CardDescription>{t("reports.current_mrr")}</CardDescription><CardTitle className="text-2xl tabular-nums">{formatMoney(lastMrr, currency, lang)}</CardTitle></CardHeader>
              <CardContent className="text-sm text-muted-foreground">{t("reports.arr")}: {formatMoney(sumExact(Array.from({ length: 12 }, () => ({ month: to, currency, amount: lastMrr, count: 1 })), currency), currency, lang)}</CardContent></Card>
            <Card><CardHeader><CardDescription>{t("reports.avg_churn")}</CardDescription><CardTitle className="text-2xl tabular-nums">{formatNumber((startAvg * 100).toFixed(2), lang)}%</CardTitle></CardHeader>
              <CardContent className="text-sm text-muted-foreground">{t("reports.churned_total", { count: churned })}</CardContent></Card>
          </div>
          {data.revenue.length === 0 && data.mrr.length === 0 && data.churn.every((c) => c.customers_start === 0 && c.gained === 0) ? (
            <Card><CardContent><EmptyState icon={ChartLineUpIcon} title={t("chart.empty_title")} description={t("chart.empty_hint")} /></CardContent></Card>
          ) : (
            <div className="grid gap-4 *:min-w-0 lg:grid-cols-2">
              <Card className="lg:col-span-2">
                <CardHeader><CardTitle>{t("reports.revenue")}</CardTitle><CardDescription>{t("reports.revenue_hint")}</CardDescription></CardHeader>
                <CardContent>
                  <ChartContainer config={revCfg} className={AXIS}>
                    <BarChart data={revData} accessibilityLayer>
                      <CartesianGrid vertical={false} />{x}
                      <YAxis tickLine={false} axisLine={false} width={64} tickFormatter={money} />
                      <ChartTooltip cursor={{ fill: "var(--muted)", opacity: 0.4 }} content={tip} />
                      <ChartLegend content={<ChartLegendContent />} />
                      <Bar dataKey="revenue" fill="var(--color-revenue)" radius={4} maxBarSize={40} isAnimationActive={false} />
                      <Bar dataKey="refunds" fill="var(--color-refunds)" radius={4} maxBarSize={40} isAnimationActive={false} />
                    </BarChart>
                  </ChartContainer>
                </CardContent>
              </Card>
              <Card>
                <CardHeader><CardTitle>{t("reports.mrr")}</CardTitle><CardDescription>{t("reports.mrr_hint")}</CardDescription></CardHeader>
                <CardContent>
                  <ChartContainer config={mrrCfg} className={AXIS}>
                    <AreaChart data={mrrData} accessibilityLayer>
                      <CartesianGrid vertical={false} />{x}
                      <YAxis tickLine={false} axisLine={false} width={64} tickFormatter={money} />
                      <ChartTooltip content={tip} />
                      <Area dataKey="mrr" type="monotone" stroke="var(--color-mrr)" fill="var(--color-mrr)" fillOpacity={0.2} isAnimationActive={false} />
                    </AreaChart>
                  </ChartContainer>
                </CardContent>
              </Card>
              <Card>
                <CardHeader><CardTitle>{t("reports.churn")}</CardTitle><CardDescription>{t("reports.churn_hint")}</CardDescription></CardHeader>
                <CardContent>
                  <ChartContainer config={churnCfg} className={AXIS}>
                    <ComposedChart data={churnData} accessibilityLayer>
                      <CartesianGrid vertical={false} />{x}
                      <YAxis yAxisId="n" tickLine={false} axisLine={false} width={40} allowDecimals={false} />
                      <YAxis yAxisId="p" orientation="right" tickLine={false} axisLine={false} width={44} tickFormatter={(v: number) => `${v}%`} />
                      <ChartTooltip content={tip} />
                      <ChartLegend content={<ChartLegendContent />} />
                      <Bar yAxisId="n" dataKey="gained" fill="var(--color-gained)" radius={4} maxBarSize={28} isAnimationActive={false} />
                      <Bar yAxisId="n" dataKey="churned" fill="var(--color-churned)" radius={4} maxBarSize={28} isAnimationActive={false} />
                      <Line yAxisId="p" dataKey="rate" type="monotone" stroke="var(--color-rate)" dot={false} isAnimationActive={false} />
                    </ComposedChart>
                  </ChartContainer>
                </CardContent>
              </Card>
            </div>
          )}
        </>
      )}
      <TaxReportCard from={from} to={to} />
      <ReportLibrary from={from} to={to} />
    </div>
  );
}
