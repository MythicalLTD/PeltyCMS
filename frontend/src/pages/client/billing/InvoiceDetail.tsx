import { invoiceLabel } from "@/lib/billing/billing";
import { PageHeader } from "@/components/PageHeader";
import { useState } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { RelatedDocsCard, UblButton } from "@/components/billing/DebitNotes";
import { docTitleKey } from "@/lib/billing/einvoice";
import { Link, useParams } from "react-router";
import { useCrumb } from "@/lib/crumb";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CoinsIcon, CreditCardIcon, FilePdfIcon } from "@phosphor-icons/react";
import { RefundsCard } from "@/components/billing/RefundsCard";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { applicable, creditIn, pdfUrl, type CreditState } from "@/lib/billing/credit";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell as Td, TableHead as Th, TableHeader, TableRow } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { FormError } from "@/components/Field";
import { StatusBadge } from "@/components/StatusBadge";
import { api, ApiError } from "@/lib/api";
import { formatDateTime, formatNumber } from "@/lib/format";
import { formatDate, formatMoney, type InvoiceDetail, type Payment } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

interface Bank { beneficiary: string; iban: string; bic: string; bank_name: string; note: string }
interface Instructions { text: string; reference: string; amount: string; currency: string; bank: Bank | null }

function BankInstructions({ i }: { i: Instructions }) {
  const { t, lang } = useI18n();
  const rows: [string, string][] = [
    [t("billing.bank.beneficiary"), i.bank?.beneficiary ?? ""],
    [t("billing.bank.iban"), (i.bank?.iban ?? "").replace(/(.{4})/g, "$1 ").trim()],
    [t("billing.bank.bic"), i.bank?.bic ?? ""],
    [t("billing.bank.bank_name"), i.bank?.bank_name ?? ""],
    [t("billing.amount"), formatMoney(i.amount, i.currency, lang)],
    [t("billing.bank.reference"), i.reference],
  ];
  const notes = [i.text, i.bank?.note ?? ""].map((n) => n.trim()).filter(Boolean);
  if (!i.bank && notes.length === 0) {
    return <p data-testid="bank-unconfigured" className="rounded-md border bg-muted p-3 text-sm text-muted-foreground">{t("billing.bank.unconfigured")}</p>;
  }
  return (
    <div className="flex flex-col gap-2 rounded-md border bg-muted p-3 text-sm" data-testid="bank-instructions">
      <p>{t("billing.bank.intro")}</p>
      <dl className="grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-[auto_1fr]">
        {rows.filter(([, v]) => v).map(([k, v]) => (
          <div key={k} className="contents"><dt className="text-muted-foreground">{k}</dt><dd className="break-all font-mono">{v}</dd></div>
        ))}
      </dl>
      {notes.map((n) => <p key={n} className="whitespace-pre-wrap">{n}</p>)}
    </div>
  );
}

export { BankInstructions };

export function InvoiceLines({ d, showBalance = true }: { d: InvoiceDetail; showBalance?: boolean }) {
  const { t, lang } = useI18n();
  const m = (v: string) => formatMoney(v, d.invoice.currency, lang);
  const qty = (q: string) => formatNumber(q, lang);
  const totals: [string, string, boolean][] = [
    [t("billing.subtotal"), m(d.invoice.subtotal), false],
    ...(d.invoice.tax_breakdown?.length
      ? d.invoice.tax_breakdown.map((p) => [`${p.name} ${formatNumber(p.rate, lang)} %`, m(p.amount), false] as [string, string, boolean])
      : [[t("billing.tax"), m(d.invoice.tax_total), false] as [string, string, boolean]]),
    [t("billing.total"), m(d.invoice.total), true],
    ...(showBalance ? [[t("billing.balance"), m(d.balance), false] as [string, string, boolean]] : []),
  ];
  return (
    <>
      {(d.invoice.reverse_charge || d.invoice.tax_inclusive) && (
        <div className="flex flex-col gap-1 border-b px-4 py-3 text-sm text-muted-foreground" data-testid="tax-notes">
          {d.invoice.reverse_charge && <p>{t("billing.reverse_charge")}</p>}
          {d.invoice.tax_inclusive && <p>{t("billing.tax_inclusive")}</p>}
          {typeof d.invoice.customer_snapshot?.vat_number === "string" && d.invoice.customer_snapshot.vat_number && <p>{t("billing.vat_number")}: <span className="font-mono">{d.invoice.customer_snapshot.vat_number}</span></p>}
        </div>
      )}
      <div className="hidden md:block">
        <Table>
          <TableHeader><TableRow><Th>{t("billing.description")}</Th><Th className="text-right">{t("billing.quantity")}</Th><Th className="text-right">{t("billing.unit_price")}</Th><Th className="text-right">{t("billing.amount")}</Th></TableRow></TableHeader>
          <TableBody>
            {d.items.map((i) => (
              <TableRow key={i.id}><Td>{i.description}</Td><Td className="text-right tabular-nums">{qty(i.quantity)}</Td><Td className="whitespace-nowrap text-right tabular-nums">{m(i.unit_price)}</Td><Td className="whitespace-nowrap text-right tabular-nums">{m(i.amount)}</Td></TableRow>
            ))}
            {totals.map(([k, v, strong]) => (
              <TableRow key={k}><Td colSpan={3} className={strong ? "text-right font-medium" : "text-right text-muted-foreground"}>{k}</Td><Td className={`whitespace-nowrap text-right tabular-nums ${strong ? "font-medium" : ""}`}>{v}</Td></TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      <div className="flex flex-col md:hidden" data-testid="invoice-lines-mobile">
        <ul className="divide-y border-t">
          {d.items.map((i) => (
            <li key={i.id} className="flex flex-col gap-1 px-4 py-3 text-sm">
              <span className="break-words">{i.description}</span>
              <span className="flex justify-between gap-3 text-muted-foreground"><span className="tabular-nums">{qty(i.quantity)} × {m(i.unit_price)}</span><span className="whitespace-nowrap font-medium tabular-nums text-foreground">{m(i.amount)}</span></span>
            </li>
          ))}
        </ul>
        <dl className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1 border-t px-4 py-3 text-sm">
          {totals.map(([k, v, strong]) => (
            <div key={k} className="contents"><dt className={strong ? "font-medium" : "text-muted-foreground"}>{k}</dt><dd className={`whitespace-nowrap text-right tabular-nums ${strong ? "font-medium" : ""}`}>{v}</dd></div>
          ))}
        </dl>
      </div>
    </>
  );
}

export function ClientInvoicePage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const [error, setError] = useState<string | null>(null);
  const { data } = useQuery({ queryKey: ["client", "invoice", id], queryFn: () => api<InvoiceDetail>(`/client/invoices/${id}`) });
  const qc = useQueryClient();
  const credit = useQuery({ queryKey: ["client", "credit"], queryFn: () => api<CreditState>("/client/credit") });
  const applyCredit = useMutation({
    mutationFn: () => api<unknown>(`/client/invoices/${id}/apply-credit`, { method: "POST", body: {} }),
    onSuccess: () => { setError(null); void qc.invalidateQueries({ queryKey: ["client"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  useCrumb(data ? invoiceLabel(data.invoice, t) : null);
  if (!data) return <PageSkeleton />;
  const inv = data.invoice;
  const m = (v: string) => formatMoney(v, inv.currency, lang);
  const usable = applicable(creditIn(credit.data, inv.currency), data.balance);
  const facts: [string, string][] = [
    [t("billing.issued"), formatDate(inv.issued_at, lang)],
    [t("billing.due"), formatDate(inv.due_at, lang)],
    [t("billing.paid_at"), formatDate(inv.paid_at, lang)],
  ];
  return (
    <div className="flex flex-col gap-6">
    <PageHeader title={inv.number ? `${t(docTitleKey(inv.kind))} ${inv.number}` : invoiceLabel(inv, t)}
      badge={<StatusBadge status={inv.status} />} back={{ to: "/invoices", label: t("nav.invoices") }} />
    {data.merged_into && (
      <Alert>
        <AlertTitle>{t("invoice_ops.merged_notice")}</AlertTitle>
        <AlertDescription>
          <Link className="underline underline-offset-2" to={`/invoices/${data.merged_into}`}>{t("invoice_ops.open_merged")}</Link>
        </AlertDescription>
      </Alert>
    )}
    {data.split_from && (
      <Alert>
        <AlertTitle>{t("invoice_ops.split_notice")}</AlertTitle>
        <AlertDescription>
          <Link className="underline underline-offset-2" to={`/invoices/${data.split_from}`}>{t("invoice_ops.open_split_parent")}</Link>
        </AlertDescription>
      </Alert>
    )}
    {data.client_notes?.trim() && (
      <p className="rounded-md border bg-muted/40 px-3 py-2 text-sm whitespace-pre-wrap">{data.client_notes}</p>
    )}
    <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <div className="flex flex-col gap-4 lg:order-1">
      <Card className="gap-0 overflow-hidden py-0">
        <Replaceable section="lines" ctx={{ invoice_id: inv.id, status: inv.status }}><InvoiceLines d={data} showBalance={false} /></Replaceable>
      </Card>
      {data.payments.length > 0 && <PaymentsCard payments={data.payments} />}
      </div>
      <div className="flex flex-col gap-4 lg:order-2 lg:sticky lg:top-20">
        <Replaceable section="summary" ctx={{ invoice_id: inv.id, status: inv.status }}><Card data-testid="invoice-summary">
          <CardHeader><CardTitle>{t("billing.summary")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-4 text-sm">
            <div className="flex flex-col gap-1">
              <span className="text-muted-foreground">{t("billing.balance")}</span>
              <span className="text-3xl font-semibold tabular-nums">{m(data.balance)}</span>
              <span className="text-muted-foreground">{t("billing.total")}: <span className="tabular-nums text-foreground">{m(inv.total)}</span></span>
            </div>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 border-t pt-4">
              {facts.map(([k, v]) => <div key={k} className="contents"><dt className="text-muted-foreground">{k}</dt><dd className="text-right">{v}</dd></div>)}
            </dl>
            {inv.finalized_at && <Button asChild variant="outline" className="min-h-11 sm:min-h-9"><a href={pdfUrl("client", inv.id)} download><FilePdfIcon aria-hidden />{t("billing.download_pdf")}</a></Button>}
            {inv.number && <UblButton scope="client" id={inv.id} className="min-h-11 sm:min-h-9" />}
          </CardContent>
        </Card></Replaceable>
      {inv.status === "unpaid" && (
        <Card>
          <CardHeader><CardTitle>{t("billing.pay")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-3">
            <FormError message={error} />
            <div className="flex flex-col gap-2 [&_button]:min-h-11 sm:[&_button]:min-h-9">
              {usable !== "0" && (
                <Button variant="secondary" data-testid="apply-credit" disabled={applyCredit.isPending} onClick={() => applyCredit.mutate()}>
                  <CoinsIcon aria-hidden />{t("credit.apply", { amount: m(usable) })}
                </Button>
              )}
              <Button asChild data-testid="pay-link"><Link to={`/invoices/${inv.id}/pay`}><CreditCardIcon aria-hidden />{t("billing.pay")}</Link></Button>
            </div>
          </CardContent>
        </Card>
      )}
      <RelatedDocsCard scope="client" d={data} />
      <RefundsCard scope="client" invoiceId={inv.id} />
      </div>
    </div>
    </div>
  );
}

function PaymentsCard({ payments }: { payments: Payment[] }) {
  const { t, lang } = useI18n();
  return (
    <Card>
      <CardHeader><CardTitle>{t("billing.payments")}</CardTitle></CardHeader>
      <CardContent className="p-0">
        <ul className="divide-y text-sm">
          {payments.map((p) => (
            <li key={p.id} className="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
              <span className="min-w-0">
                <span className="font-mono text-xs">{p.gateway}</span>
                <span className="block text-xs text-muted-foreground">{formatDateTime(p.created_at, lang)}</span>
              </span>
              <span className="flex items-center gap-2">
                <StatusBadge status={p.status} />
                <span className="tabular-nums font-medium">{formatMoney(p.amount, p.currency, lang)}</span>
              </span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
