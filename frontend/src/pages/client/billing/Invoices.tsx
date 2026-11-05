import { ReceiptIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { DataTable } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { ClientUnpaidInvoices } from "@/components/billing/ClientUnpaidInvoices";
import { AddFundsDialog } from "@/components/billing/AddFunds";
import { formatDate, formatMoney, invoiceLabel, type Invoice } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { RedeemGiftCardDialog } from "@/pages/admin/billing/GiftCards";

export function ClientInvoicesPage() {
  const { t, lang } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <ClientUnpaidInvoices />
      <DataTable<Invoice>
        emptyIcon={ReceiptIcon} emptyTitle={t("billing.no_invoices")}
        title={t("billing.invoices")}
        description={t("billing.invoices_hint")}
        toolbar={<div className="flex flex-wrap gap-2"><AddFundsDialog /><RedeemGiftCardDialog /></div>}
        path="/client/invoices"
        queryKey={["client", "invoices"]}
        columns={[
          { header: t("billing.number"), cell: (i) => <Link className="font-mono text-xs text-primary hover:underline" to={`/invoices/${i.id}`}>{invoiceLabel(i, t)}</Link> },
          { header: t("billing.status"), cell: (i) => <StatusBadge status={i.status} /> },
          { header: t("billing.total"), cell: (i) => formatMoney(i.total, i.currency, lang) },
          { header: t("billing.issued"), cell: (i) => formatDate(i.issued_at, lang) },
          { header: t("billing.due"), cell: (i) => formatDate(i.due_at, lang) },
        ]}
      />
    </div>
  );
}
