import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { ArrowClockwiseIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Button } from "@/components/ui/button";
import { api, ApiError } from "@/lib/api";
import type { Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

/** Services that can be renewed ahead of the cron. */
export function renewable(s: Pick<Service, "status" | "cycle" | "next_due_date">): boolean {
  return (s.status === "active" || s.status === "suspended") && s.cycle !== "one_time" && !!s.next_due_date;
}

/** On-demand renewal: invoice the next period now (client or admin). */
export function RenewNowButton({ service, admin = false }: { service: Service; admin?: boolean }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const cfg = useQuery({ queryKey: ["renewals", "config"], queryFn: () => api<{ enabled: boolean; max_days_ahead: number }>("/client/renewals/config"), enabled: !admin });
  const run = useMutation({
    mutationFn: () => api<{ invoice_id: string }>(`/${admin ? "admin" : "client"}/services/${service.id}/renew-now`, { method: "POST" }),
    onSuccess: (r) => {
      toast.success(t("renew.created"));
      void qc.invalidateQueries({ queryKey: [admin ? "admin" : "client"] });
      if (!admin) void nav(`/invoices/${r.invoice_id}`);
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!renewable(service) || (!admin && !cfg.data?.enabled)) return null;
  return (
    <ConfirmAction description={t("renew.confirm")} onConfirm={() => run.mutate()}>
      <Button size="sm" variant="outline" className="min-h-11 sm:min-h-8" disabled={run.isPending}><ArrowClockwiseIcon aria-hidden />{t("renew.now")}</Button>
    </ConfirmAction>
  );
}
