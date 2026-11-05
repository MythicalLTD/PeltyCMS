import { HourglassMediumIcon } from "@phosphor-icons/react";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/StatusBadge";
import { awaitingPayment } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

/** Order status plus an "Awaiting payment" marker when the order is active but its invoice is not paid yet. */
export function OrderStatusBadge({ status, invoiceStatus }: { status: string; invoiceStatus?: string | null }) {
  const { t } = useI18n();
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      <StatusBadge status={status} />
      {awaitingPayment(status, invoiceStatus) && (
        <Badge variant="outline" title={t("orders.awaiting_payment_hint")}><HourglassMediumIcon aria-hidden />{t("orders.awaiting_payment")}</Badge>
      )}
    </span>
  );
}
