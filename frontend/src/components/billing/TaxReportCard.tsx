import { useQuery } from "@tanstack/react-query";
import { DownloadSimpleIcon } from "@phosphor-icons/react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { exportUrl, trimRate, type ExportKind, type TaxReport } from "@/lib/admin/exports";
import { useI18n } from "@/lib/i18n";

export function TaxReportCard({ from, to }: { from: string; to: string }) {
  const { t, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "reports", "tax", from, to], queryFn: () => api<TaxReport>(`/admin/reports/tax?from=${from}&to=${to}`) });
  const kinds: ExportKind[] = ["tax", "invoices", "payments"];
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("reports.tax.title")}</CardTitle>
        <CardDescription>{t("reports.tax.subtitle")}</CardDescription>
        <div className="flex flex-wrap gap-2 pt-2">
          {kinds.map((k) => (
            <Button key={k} variant="outline" size="sm" asChild>
              <a href={exportUrl(k, from, to)} download><DownloadSimpleIcon aria-hidden />{t(`reports.export.${k}`)}</a>
            </Button>
          ))}
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {isLoading ? <Skeleton className="h-32 w-full" /> : !data || data.rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("reports.tax.empty")}</p>
        ) : (
          <>
            <div className="flex flex-wrap gap-2">
              {data.totals.map((x) => (
                <Badge key={x.currency} variant="secondary">{x.currency}: {t("reports.tax.tax")} {formatMoney(x.tax, x.currency, lang)} · {t("reports.tax.gross")} {formatMoney(x.gross, x.currency, lang)}</Badge>
              ))}
            </div>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("reports.tax.period")}</TableHead>
                  <TableHead>{t("reports.tax.country")}</TableHead>
                  <TableHead>{t("reports.tax.rate")}</TableHead>
                  <TableHead className="text-right">{t("reports.tax.documents")}</TableHead>
                  <TableHead className="text-right">{t("reports.tax.net")}</TableHead>
                  <TableHead className="text-right">{t("reports.tax.tax")}</TableHead>
                  <TableHead className="text-right">{t("reports.tax.gross")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {data.rows.map((r) => (
                  <TableRow key={`${r.period}-${r.country}-${r.currency}-${r.rate}-${r.reverse_charge}`}>
                    <TableCell>{r.period}</TableCell>
                    <TableCell>{r.country}</TableCell>
                    <TableCell>{trimRate(r.rate)}%{r.reverse_charge && <Badge variant="outline" className="ml-2">{t("reports.tax.reverse_charge")}</Badge>}</TableCell>
                    <TableCell className="text-right">{r.documents}</TableCell>
                    <TableCell className="text-right">{formatMoney(r.net, r.currency, lang)}</TableCell>
                    <TableCell className="text-right">{formatMoney(r.tax, r.currency, lang)}</TableCell>
                    <TableCell className="text-right">{formatMoney(r.gross, r.currency, lang)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </>
        )}
      </CardContent>
    </Card>
  );
}
