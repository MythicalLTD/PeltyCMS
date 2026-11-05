import { PackageIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { DataTable } from "@/components/DataTable";
import { OrderStatusBadge } from "@/components/OrderStatusBadge";
import { formatDate, formatMoney, type Order } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

export function ClientOrdersPage() {
  const { t, lang } = useI18n();
  return (
    <DataTable<Order>
      emptyIcon={PackageIcon}
      emptyTitle={t("orders.empty")}
      title={t("nav.orders")}
      description={t("orders.hint")}
      path="/client/orders"
      queryKey={["client", "orders"]}
      columns={[
        { header: t("billing.number"), cell: (o) => <Link className="font-mono text-xs text-primary hover:underline" to={`/orders/${o.id}`}>#{o.number}</Link> },
        { header: t("billing.status"), cell: (o) => <OrderStatusBadge status={o.status} invoiceStatus={o.invoice_status} /> },
        { header: t("billing.total"), cell: (o) => formatMoney(o.total, o.currency, lang) },
        { header: t("billing.issued"), cell: (o) => formatDate(o.created_at, lang) },
      ]}
    />
  );
}
