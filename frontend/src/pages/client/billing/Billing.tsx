import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { CoinsIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { AddFundsDialog } from "@/components/billing/AddFunds";
import { PaymentMethodsPanel } from "@/components/billing/PaymentMethods";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { type CreditState } from "@/lib/billing/credit";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { RedeemGiftCardDialog } from "@/pages/admin/billing/GiftCards";

function creditKindLabel(kind: string, note: string, tCode: (k: string) => string) {
  if (note.trim()) return note;
  const key = `credit.kind.${kind}`;
  const labeled = tCode(key);
  return labeled === key ? kind : labeled;
}

function CreditLedger() {
  const { t, tCode, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["client", "credit"], queryFn: () => api<CreditState>("/client/credit") });
  if (isLoading) return <Skeleton className="h-48" />;
  const balances = data?.balances ?? [];
  const entries = data?.entries ?? [];
  return (
    <Card>
      <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <CardTitle className="flex items-center gap-2"><CoinsIcon aria-hidden />{t("credit.title")}</CardTitle>
          <CardDescription>{t("credit.client_hint")}</CardDescription>
        </div>
        <div className="flex flex-wrap gap-2">
          <AddFundsDialog />
          <RedeemGiftCardDialog />
        </div>
      </CardHeader>
      <CardContent className="grid gap-6">
        <div className="flex flex-wrap gap-4" data-testid="credit-balances">
          {balances.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t("credit.none")}</p>
          ) : balances.map((b) => (
            <div key={b.currency} className="min-w-[8rem]">
              <p className="text-xs text-muted-foreground">{b.currency}</p>
              <p className="text-2xl font-semibold tabular-nums">{formatMoney(b.amount, b.currency, lang)}</p>
            </div>
          ))}
        </div>
        {entries.length === 0 ? (
          <EmptyState icon={CoinsIcon} title={t("credit.ledger_empty")} />
        ) : (
          <ul className="divide-y border-y text-sm" data-testid="credit-entries">
            {entries.map((e) => (
              <li key={e.id} className="flex flex-col gap-1 py-3 sm:flex-row sm:items-center sm:justify-between sm:gap-3">
                <span className="min-w-0">
                  <span className="block truncate">{creditKindLabel(e.kind, e.note, tCode)}</span>
                  <span className="text-xs text-muted-foreground">
                    {formatDateTime(e.created_at, lang)}
                    {e.invoice_id ? <> · <Link className="underline-offset-2 hover:underline" to={`/invoices/${e.invoice_id}`}>{t("credit.view_invoice")}</Link></> : null}
                  </span>
                </span>
                <span className="whitespace-nowrap tabular-nums font-medium">{formatMoney(e.amount, e.currency, lang)}</span>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

export function ClientBillingPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("autopay.billing_title")} description={t("credit.billing_subtitle")} />
      <CreditLedger />
      <PaymentMethodsPanel scope="client" />
    </div>
  );
}
