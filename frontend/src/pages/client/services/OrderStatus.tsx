import { OrderStatusBadge } from "@/components/OrderStatusBadge";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useParams, useSearchParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { CheckCircleIcon, CircleIcon, ReceiptIcon, XCircleIcon } from "@phosphor-icons/react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent,} from "@/components/ui/card";
import { StatusBadge } from "@/components/StatusBadge";
import { api } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { orderStage } from "@/lib/store/storefront";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";

export interface OrderView {
  order: { id: string; number: number; status: string; currency: string; total: string; invoice_id: string | null; created_at: string };
  items: { id: string; product_name: string; cycle: string; amount: string; setup_fee: string }[];
  invoice: { id: string; number: string | null; status: string; total: string } | null;
  services: { id: string; product_name: string; status: string }[];
}

/** Order confirmation (`?placed=1`) and status page for the customer. */
export function OrderStatusPage() {
  const { id = "" } = useParams();
  const [sp] = useSearchParams();
  const { t, tCode, lang } = useI18n();
  const { data } = useQuery({ queryKey: ["client", "order", id], queryFn: () => api<OrderView>(`/client/orders/${id}`), refetchInterval: 15_000 });
  if (!data) return <PageSkeleton />;
  const o = data.order;
  const stage = orderStage(o.status, data.invoice?.status, data.services);
  const steps = ["store.step_placed", "store.step_paid", "store.step_approved", "store.step_active"] as const;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("store.order_n", { n: o.number })} badge={<OrderStatusBadge status={o.status} invoiceStatus={data.invoice?.status} />}
        description={`${formatDate(o.created_at, lang)} · ${formatMoney(o.total, o.currency, lang)}`} />
      {sp.get("placed") && (
        <Alert><CheckCircleIcon aria-hidden /><AlertTitle>{t("store.thanks")}</AlertTitle><AlertDescription>{t("store.thanks_hint", { n: o.number })}</AlertDescription></Alert>
      )}
      <Card>
        <CardContent className="grid gap-6">
          <Replaceable section="progress" ctx={{ order_id: o.id, status: o.status, stage }}>{stage < 0 ? (
            <p className="flex items-center gap-2 text-sm text-destructive"><XCircleIcon aria-hidden />{t("store.order_cancelled")}</p>
          ) : (
            <ol className="grid gap-3 *:min-w-0 sm:grid-cols-4">
              {steps.map((s, i) => (
                <li key={s} className="flex items-center gap-2 text-sm" aria-current={i === stage ? "step" : undefined}>
                  {i <= stage ? <CheckCircleIcon weight="fill" className="size-5 text-primary" aria-hidden /> : <CircleIcon className="size-5 text-muted-foreground" aria-hidden />}
                  <span className={i <= stage ? "font-medium" : "text-muted-foreground"}>{t(s)}</span>
                </li>
              ))}
            </ol>
          )}</Replaceable>
          <Replaceable section="items" ctx={{ order_id: o.id, status: o.status }}><ul className="flex flex-col divide-y rounded-md border">
            {data.items.map((it) => (
              <li key={it.id} className="flex items-center justify-between gap-3 p-3 text-sm">
                <span>{it.product_name} · {tCode(`cycle.${it.cycle}`)}</span>
                <span className="tabular-nums">{formatMoney(it.amount, o.currency, lang)}</span>
              </li>
            ))}
          </ul></Replaceable>
          {data.services.length > 0 && <Replaceable section="services" ctx={{ order_id: o.id }}>{(
            <div className="grid gap-2">
              <p className="text-sm font-medium">{t("nav.services")}</p>
              {data.services.map((s) => <Link key={s.id} to={`/services/${s.id}`} className="flex items-center justify-between rounded-md border p-3 text-sm hover:bg-accent/50">{s.product_name}<StatusBadge status={s.status} /></Link>)}
            </div>
          )}</Replaceable>}
          {data.invoice && (
            <div className="flex flex-wrap items-center gap-2">
              <Button asChild variant={data.invoice.status === "unpaid" ? "default" : "outline"}>
                <Link to={`/invoices/${data.invoice.id}`}><ReceiptIcon aria-hidden />{data.invoice.status === "unpaid" ? t("store.pay_now") : t("store.view_invoice")}</Link>
              </Button>
              <StatusBadge status={data.invoice.status} />
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
