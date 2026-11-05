import { HardDrivesIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { DataTable } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { formatDate, formatMoney, type Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { ServiceManageDialog } from "./ServiceManage";
import { RenewNowButton } from "@/components/billing/RenewNowButton";
import { ServiceUsageDialog } from "@/pages/admin/billing/Usage";
import { ServiceSsoButton } from "./ServiceSso";
import { ServiceModuleDialog } from "./ServiceModule";

export function ClientServicesPage() {
  const { t, tCode, lang } = useI18n();
  return (
    <DataTable<Service>
      emptyIcon={HardDrivesIcon} emptyTitle={t("dashboard.no_services")}
      title={t("billing.services")}
      path="/client/services"
      queryKey={["client", "services"]}
      columns={[
        { header: t("billing.product"), cell: (s) => <Link className="font-medium hover:underline" to={`/services/${s.id}`}>{s.product_name}</Link> },
        { header: t("billing.status"), cell: (s) => <StatusBadge status={s.status} /> },
        { header: t("billing.cycle"), cell: (s) => tCode(`cycle.${s.cycle}`) },
        { header: t("billing.amount"), cell: (s) => formatMoney(s.recurring_amount, s.currency, lang) },
        { header: t("billing.next_due"), cell: (s) => formatDate(s.next_due_date, lang) },
        { header: "", cell: (s) => <div className="flex flex-wrap gap-1"><ServiceSsoButton service={s} /><ServiceModuleDialog service={s} /><ServiceUsageDialog service={s} /><ServiceManageDialog service={s} /><RenewNowButton service={s} /></div> },
      ]}
    />
  );
}
