import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { DownloadSimpleIcon, PrinterIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api } from "@/lib/api";
import { chartRows, groupByCategory, type LibraryItem, type LibraryTable } from "@/lib/admin/reportLibrary";
import { useI18n } from "@/lib/i18n";

/** WHMCS report library: pick a report, see a chart + table, export CSV or print to PDF. */
export function ReportLibrary({ from, to }: { from: string; to: string }) {
  const { t, tCode } = useI18n();
  const [key, setKey] = useState("annual_income");
  const lib = useQuery({ queryKey: ["admin", "report-library"], queryFn: () => api<{ items: LibraryItem[] }>("/admin/reports/library") });
  const r = useQuery({ queryKey: ["admin", "report-library", key, from, to], queryFn: () => api<LibraryTable>(`/admin/reports/library/${key}?from=${from}&to=${to}`) });
  const config: ChartConfig = { value: { label: t("reports.lib.value"), color: "var(--chart-1)" } };
  const data = r.data ? chartRows(r.data) : [];
  return (
    <Card className="print:shadow-none">
      <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div><CardTitle>{t("reports.lib.title")}</CardTitle><CardDescription>{t("reports.lib.description")}</CardDescription></div>
        <div className="flex flex-wrap gap-2 print:hidden">
          <Select value={key} onValueChange={setKey}>
            <SelectTrigger className="w-64"><SelectValue /></SelectTrigger>
            <SelectContent>
              {lib.data && Object.entries(groupByCategory(lib.data.items)).map(([cat, items]) => (
                <SelectGroup key={cat}><SelectLabel>{tCode(`reports.cat.${cat}`)}</SelectLabel>
                  {items.map((i) => <SelectItem key={i.key} value={i.key}>{tCode(`reports.r.${i.key}`)}</SelectItem>)}
                </SelectGroup>
              ))}
            </SelectContent>
          </Select>
          <Button asChild variant="outline" className="h-11 md:h-9"><a href={`/api/v1/admin/reports/library/${key}?from=${from}&to=${to}&format=csv`} download><DownloadSimpleIcon />CSV</a></Button>
          <Button variant="outline" className="h-11 md:h-9" onClick={() => window.print()}><PrinterIcon />PDF</Button>
        </div>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-4">
        {!r.data ? <Skeleton className="h-48" /> : (
          <>
            {r.data.chart && data.length > 0 && (
              <ChartContainer config={config} className="aspect-auto h-56 w-full text-sm">
                <BarChart data={data} accessibilityLayer>
                  <CartesianGrid vertical={false} />
                  <XAxis dataKey="label" tickLine={false} axisLine={false} />
                  <YAxis tickLine={false} axisLine={false} width={56} />
                  <ChartTooltip content={<ChartTooltipContent />} />
                  <Bar dataKey="value" fill="var(--color-value)" radius={4} />
                </BarChart>
              </ChartContainer>
            )}
            <div className="overflow-x-auto">
              <Table>
                <TableHeader><TableRow>{r.data.columns.map((c) => <TableHead key={c}>{tCode(`reports.col.${c}`)}</TableHead>)}</TableRow></TableHeader>
                <TableBody>
                  {r.data.rows.length === 0 ? <TableRow><TableCell colSpan={r.data.columns.length} className="text-muted-foreground">{t("common.empty")}</TableCell></TableRow>
                    : r.data.rows.map((row, i) => <TableRow key={i}>{row.map((c, j) => <TableCell key={j} className="tabular-nums">{c ?? "-"}</TableCell>)}</TableRow>)}
                </TableBody>
              </Table>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
