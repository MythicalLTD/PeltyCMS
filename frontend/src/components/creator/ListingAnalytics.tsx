import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from "recharts";
import { SelectField } from "@/components/SelectField";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import type { ListingAnalytics as Stats } from "@/lib/creatorTools";
import { useI18n } from "@/lib/i18n";

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-lg border p-3">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="text-lg font-semibold tabular-nums">{value}</p>
    </div>
  );
}

/** Per-listing analytics: views, sales, revenue, conversion over 7–90 days. */
export function ListingAnalytics({ productId, name, onClose }: { productId: string | null; name: string; onClose: () => void }) {
  const { t, lang } = useI18n();
  const [days, setDays] = useState("30");
  const q = useQuery({
    queryKey: ["client", "creator", "analytics", productId, days],
    queryFn: () => api<Stats>(`/client/creator/listings/${productId}/analytics?days=${days}`),
    enabled: !!productId,
  });
  const config: ChartConfig = {
    views: { label: t("ctools.views"), color: "var(--chart-2)" },
    sales: { label: t("ctools.sales"), color: "var(--chart-1)" },
  };
  const d = q.data;
  const label = (s: string) => new Date(`${s}T00:00:00Z`).toLocaleDateString(lang, { month: "short", day: "numeric", timeZone: "UTC" });
  return (
    <Dialog open={!!productId} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-3xl" data-testid="listing-analytics">
        <DialogHeader>
          <DialogTitle>{t("ctools.analytics_title", { name })}</DialogTitle>
          <DialogDescription>{t("ctools.analytics_hint")}</DialogDescription>
        </DialogHeader>
        <div className="w-40">
          <SelectField id="ct-days" label={t("ctools.period")} value={days} onChange={setDays} options={["7", "30", "90"].map((n) => ({ value: n, label: t("ctools.last_days", { n }) }))} />
        </div>
        {!d ? <Skeleton className="h-72 w-full" /> : (
          <>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-6">
              <Stat label={t("ctools.views")} value={d.totals.views.toLocaleString(lang)} />
              <Stat label={t("ctools.sales")} value={d.totals.sales.toLocaleString(lang)} />
              <Stat label={t("ctools.conversion")} value={`${Number(d.totals.conversion).toLocaleString(lang, { maximumFractionDigits: 2 })}%`} />
              <Stat label={t("ctools.revenue")} value={formatMoney(d.totals.revenue, d.currency, lang)} />
              <Stat label={t("ctools.earnings")} value={formatMoney(d.totals.earnings, d.currency, lang)} />
              <Stat label={t("ctools.wishlists")} value={d.totals.wishlists.toLocaleString(lang)} />
            </div>
            <ChartContainer config={config} className="aspect-auto h-64 w-full">
              <LineChart data={d.series.map((s) => ({ day: s.day, views: s.views, sales: s.sales }))} margin={{ left: 0, right: 8 }}>
                <CartesianGrid vertical={false} />
                <XAxis dataKey="day" tickLine={false} axisLine={false} tickFormatter={label} minTickGap={24} />
                <YAxis allowDecimals={false} tickLine={false} axisLine={false} width={32} />
                <ChartTooltip content={<ChartTooltipContent labelFormatter={(v) => label(String(v))} />} />
                <ChartLegend content={<ChartLegendContent />} />
                <Line dataKey="views" type="monotone" stroke="var(--color-views)" strokeWidth={2} dot={false} />
                <Line dataKey="sales" type="monotone" stroke="var(--color-sales)" strokeWidth={2} dot={false} />
              </LineChart>
            </ChartContainer>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
