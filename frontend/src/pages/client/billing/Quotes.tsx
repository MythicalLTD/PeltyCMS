import { useMemo, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useNavigate, useParams } from "react-router";
import { CheckIcon, DownloadSimpleIcon, FileTextIcon, XIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import type { Quote, QuoteItem } from "@/lib/p2";
import { useI18n } from "@/lib/i18n";

/** Sent quotes past valid_until are treated as expired in the UI. */
export function quoteExpired(q: Pick<Quote, "status" | "valid_until">, now = Date.now()) {
  if (q.status !== "sent" || !q.valid_until) return false;
  const end = Date.parse(q.valid_until);
  return Number.isFinite(end) && end < now;
}

export function QuoteStatus({ status, expired }: { status: string; expired?: boolean }) {
  const { tCode } = useI18n();
  if (expired) return <Badge variant="destructive">{tCode("quotes.status.expired")}</Badge>;
  const v = status === "accepted" ? "default" : status === "declined" ? "destructive" : status === "sent" ? "secondary" : "outline";
  return <Badge variant={v}>{tCode(`quotes.status.${status}`)}</Badge>;
}

export function QuoteBody({ q }: { q: Quote }) {
  const { t, lang } = useI18n();
  return (
    <>
      <ResponsiveTable<QuoteItem & { i: number }> rows={q.items.map((it, i) => ({ ...it, i }))} rowKey={(r) => r.i}
        columns={[
          { header: t("billing.description"), primary: true, cell: (r) => r.description },
          { header: t("billing.quantity"), cell: (r) => Number(r.quantity) },
          { header: t("billing.unit_price"), cell: (r) => formatMoney(r.unit_price, q.currency, lang) },
          { header: t("billing.tax"), cell: (r) => `${Number(r.tax_rate)}%` },
          { header: t("billing.amount"), cell: (r) => formatMoney(r.amount ?? "0", q.currency, lang) },
        ]} />
      <div className="grid gap-1 p-4 text-sm sm:ml-auto sm:w-72">
        <div className="flex justify-between"><span className="text-muted-foreground">{t("billing.subtotal")}</span><span className="tabular-nums">{formatMoney(q.subtotal, q.currency, lang)}</span></div>
        <div className="flex justify-between"><span className="text-muted-foreground">{t("billing.tax")}</span><span className="tabular-nums">{formatMoney(q.tax_total, q.currency, lang)}</span></div>
        <Separator className="my-1" />
        <div className="flex justify-between font-semibold"><span>{t("billing.total")}</span><span className="tabular-nums">{formatMoney(q.total, q.currency, lang)}</span></div>
      </div>
      {q.notes && <p className="whitespace-pre-wrap border-t p-4 text-sm text-muted-foreground">{q.notes}</p>}
    </>
  );
}

export function ClientQuotesPage() {
  const { t, lang } = useI18n();
  const [pendingOnly, setPendingOnly] = useState(true);
  const q = useQuery({ queryKey: ["client", "quotes"], queryFn: () => api<{ items: Quote[] }>("/client/quotes?per_page=100") });
  const rows = useMemo(() => {
    const items = q.data?.items ?? [];
    if (!pendingOnly) return items;
    return items.filter((x) => x.status === "sent" && !quoteExpired(x));
  }, [q.data, pendingOnly]);
  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title={t("quotes.title")}
        actions={
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant={pendingOnly ? "default" : "outline"} onClick={() => setPendingOnly(true)}>{t("quotes.filter_pending")}</Button>
            <Button size="sm" variant={!pendingOnly ? "default" : "outline"} onClick={() => setPendingOnly(false)}>{t("quotes.filter_all")}</Button>
          </div>
        }
      />
      <ResponsiveTable<Quote>
        rows={rows}
        rowKey={(x) => x.id}
        loading={q.isLoading}
        emptyIcon={FileTextIcon}
        emptyTitle={t("quotes.empty")}
        columns={[
          { header: t("billing.number"), primary: true, cell: (x) => <Link className="font-medium underline-offset-4 hover:underline" to={`/quotes/${x.id}`}>{x.number}</Link> },
          { header: t("quotes.subject"), cell: (x) => x.subject },
          { header: t("billing.total"), cell: (x) => formatMoney(x.total, x.currency, lang) },
          { header: t("quotes.valid_until"), cell: (x) => formatDate(x.valid_until, lang) },
          { header: t("billing.status"), cell: (x) => <QuoteStatus status={x.status} expired={quoteExpired(x)} /> },
        ]}
      />
    </div>
  );
}

export function ClientQuotePage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [error, setError] = useState<string | null>(null);
  const { data: q, isLoading } = useQuery({ queryKey: ["client", "quote", id], queryFn: () => api<Quote>(`/client/quotes/${id}`) });
  const decide = useMutation({
    mutationFn: (accept: boolean) => api<{ invoice_id?: string }>(`/client/quotes/${id}/${accept ? "accept" : "decline"}`, { method: "POST" }),
    onSuccess: (r) => { void qc.invalidateQueries({ queryKey: ["client"] }); if (r.invoice_id) void nav(`/invoices/${r.invoice_id}`); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (isLoading || !q) return <PageSkeleton />;
  const expired = quoteExpired(q);
  const canDecide = q.status === "sent" && !expired;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={q.number} badge={<QuoteStatus status={q.status} expired={expired} />} back={{ to: "/quotes", label: t("quotes.title") }}
        description={`${q.subject} · ${t("quotes.valid_until")} ${formatDate(q.valid_until, lang)}`}
        actions={canDecide && <>
          <Button variant="outline" onClick={() => decide.mutate(false)} disabled={decide.isPending}><XIcon aria-hidden />{t("quotes.decline")}</Button>
          <Button onClick={() => decide.mutate(true)} disabled={decide.isPending}><CheckIcon aria-hidden />{t("quotes.accept")}</Button>
        </>} />
      {expired && <p className="text-sm text-destructive" role="status">{t("quotes.expired_hint", { date: formatDate(q.valid_until, lang) })}</p>}
      <FormError message={error} />
      <Card className="gap-0 overflow-hidden py-0">
        <QuoteBody q={q} />
      </Card>
      <Button asChild variant="outline" className="self-start"><a href={`/api/v1/client/quotes/${q.id}/pdf`}><DownloadSimpleIcon aria-hidden />{t("quotes.download_pdf")}</a></Button>
      {q.invoice_id && <Button asChild variant="outline" className="self-start"><Link to={`/invoices/${q.invoice_id}`}>{t("quotes.view_invoice")}</Link></Button>}
    </div>
  );
}
