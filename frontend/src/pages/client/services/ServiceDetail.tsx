import { Link, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeftIcon, HardDrivesIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { StatusBadge } from "@/components/StatusBadge";
import { RenewNowButton } from "@/components/billing/RenewNowButton";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { isUuid } from "@/lib/ids";
import { api } from "@/lib/api";
import { formatDate, formatMoney, type Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { ServiceManageDialog } from "./ServiceManage";
import { ServiceModuleCard, ServiceModuleDialog } from "./ServiceModule";
import { ServiceSsoButton } from "./ServiceSso";
import { ServiceUsageDialog } from "@/pages/admin/billing/Usage";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";

interface ServiceDetail extends Service {
  addons?: { id: string; name: string; status: string; recurring_amount: string; cycle?: string }[];
}

export function ClientServicePage() {
  const { id = "" } = useParams();
  const { t, tCode, lang } = useI18n();
  const ok = isUuid(id);
  const { data, isError } = useQuery({
    queryKey: ["client", "service", id],
    queryFn: () => api<ServiceDetail>(`/client/services/${id}`),
    enabled: ok,
    retry: false,
  });
  if (!ok || isError) return <p className="text-sm text-muted-foreground">{t("common.not_found")}</p>;
  if (!data) return <PageSkeleton />;
  const addons = data.addons ?? [];
  return (
    <div className="flex flex-col gap-4">
      <div>
        <Button asChild variant="ghost" size="sm">
          <Link to="/services"><ArrowLeftIcon aria-hidden />{t("billing.services")}</Link>
        </Button>
      </div>
      <PageHeader
        title={data.product_name}
        description={
          <span className="flex flex-wrap items-center gap-2">
            <StatusBadge status={data.status} />
            <span>{tCode(`cycle.${data.cycle}`)}</span>
            <span className="tabular-nums">{formatMoney(data.recurring_amount, data.currency, lang)}</span>
          </span>
        }
        actions={
          <div className="flex flex-wrap gap-1">
            <ServiceSsoButton service={data} />
            <ServiceModuleDialog service={data} />
            <ServiceUsageDialog service={data} />
            <ServiceManageDialog service={data} />
            <RenewNowButton service={data} />
          </div>
        }
      />
      <Replaceable section="service-detail" ctx={{ service_id: data.id, status: data.status }}>
        <div className="grid gap-4 lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2"><HardDrivesIcon aria-hidden />{t("service.overview")}</CardTitle>
              <CardDescription>{t("service.overview_hint")}</CardDescription>
            </CardHeader>
            <CardContent>
              <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
                <div><dt className="text-muted-foreground">{t("billing.status")}</dt><dd><StatusBadge status={data.status} /></dd></div>
                <div><dt className="text-muted-foreground">{t("billing.cycle")}</dt><dd>{tCode(`cycle.${data.cycle}`)}</dd></div>
                <div><dt className="text-muted-foreground">{t("billing.amount")}</dt><dd className="tabular-nums">{formatMoney(data.recurring_amount, data.currency, lang)}</dd></div>
                <div><dt className="text-muted-foreground">{t("billing.next_due")}</dt><dd>{formatDate(data.next_due_date, lang)}</dd></div>
                <div><dt className="text-muted-foreground">{t("service.created")}</dt><dd>{formatDate(data.created_at, lang)}</dd></div>
                {data.suspend_reason ? (
                  <div className="sm:col-span-2"><dt className="text-muted-foreground">{t("service.suspend_reason")}</dt><dd>{data.suspend_reason}</dd></div>
                ) : null}
              </dl>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>{t("service.addons")}</CardTitle>
              <CardDescription>{t("service.addons_hint")}</CardDescription>
            </CardHeader>
            <CardContent>
              {addons.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t("service.addons_empty")}</p>
              ) : (
                <ul className="grid gap-2">
                  {addons.map((a) => (
                    <li key={a.id} className="flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-sm">
                      <span className="min-w-0 truncate font-medium">{a.name}</span>
                      <span className="flex items-center gap-2">
                        <StatusBadge status={a.status} />
                        <span className="tabular-nums">{formatMoney(a.recurring_amount, data.currency, lang)}</span>
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>
          <div className="lg:col-span-2"><ServiceModuleCard service={data} /></div>
        </div>
      </Replaceable>
    </div>
  );
}
