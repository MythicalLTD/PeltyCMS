import { useState } from "react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { EmptyState } from "@/components/EmptyState";
import { ChartBarIcon } from "@phosphor-icons/react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { formatNumber } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

function compactMoney(v: number, currency: string, lang: string) {
  return new Intl.NumberFormat(lang === "ro" ? "ro-RO" : "en-GB", { style: "currency", currency, notation: "compact", maximumFractionDigits: 1 }).format(v);
}

export interface MonthRevenue { month: string; currency: string; amount: string }

/** shadcn bar chart: last 6 months of revenue, one series per currency. */
export function RevenueChart({ series, title, hint }: { series: MonthRevenue[]; title?: string; hint?: string }) {
  const { t, lang } = useI18n();
  const currencies = [...new Set(series.map((s) => s.currency))];
  // Always show the last 6 calendar months (UTC), zero-filled, so a single
  // data point doesn't become one full-width bar.
  const [now] = useState(() => new Date());
  const months = Array.from({ length: 6 }, (_, i) => {
    const d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - 5 + i, 1));
    return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
  });
  const hasData = series.some((s) => Number(s.amount) > 0);
  const data = months.map((m) => {
    const row: Record<string, string | number> = { month: m };
    for (const c of currencies) row[c] = Number(series.find((s) => s.month === m && s.currency === c)?.amount ?? 0);
    return row;
  });
  const config: ChartConfig = Object.fromEntries(currencies.map((c, i) => [c, { label: c, color: `var(--chart-${(i % 5) + 1})` }]));
  const monthLabel = (m: string) => new Date(`${m}-01T00:00:00Z`).toLocaleDateString(lang === "ro" ? "ro-RO" : "en-US", { month: "short", timeZone: "UTC" });
  return (
    <Card className="h-full shadow-sm">
      <CardHeader>
        <CardTitle>{title ?? t("admin.dashboard.revenue_chart")}</CardTitle>
        <CardDescription>{hint ?? t("admin.dashboard.revenue_chart_hint")}</CardDescription>
      </CardHeader>
      <CardContent>
        {!hasData ? (
          <EmptyState icon={ChartBarIcon} title={t("chart.empty_title")} description={t("chart.empty_hint")} />
        ) : (
          <ChartContainer config={config} className="aspect-auto h-64 w-full text-sm [&_.recharts-cartesian-axis-tick_text]:fill-foreground/75">
            <BarChart data={data} accessibilityLayer>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="month" tickLine={false} axisLine={false} tickFormatter={monthLabel} />
              <YAxis tickLine={false} axisLine={false} width={64} allowDecimals={false}
                tickFormatter={(v: number) => currencies.length === 1 ? compactMoney(v, currencies[0] ?? "EUR", lang) : formatNumber(v, lang)} />
              <ChartTooltip cursor={{ fill: "var(--muted)", opacity: 0.4 }} content={<ChartTooltipContent labelFormatter={(l) => monthLabel(String(l))} />} />
              {currencies.length > 1 && <ChartLegend content={<ChartLegendContent />} />}
              {currencies.map((c) => <Bar key={c} dataKey={c} fill={`var(--color-${c})`} radius={6} maxBarSize={44} minPointSize={(v) => ((v ?? 0) > 0 ? 3 : 0)} isAnimationActive={false} />)}
            </BarChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  );
}
