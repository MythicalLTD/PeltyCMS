import { useState } from "react";
import { XCircleIcon } from "@phosphor-icons/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { DataTable } from "@/components/DataTable";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import type { CancelRequest } from "@/pages/client/services/ServiceManage";

const STATUSES = ["pending", "approved", "rejected", "withdrawn", "completed"] as const;

export function AdminCancellationsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [status, setStatus] = useState<string>("pending");
  const [error, setError] = useState<string | null>(null);
  const decide = useMutation({
    mutationFn: ({ id, approve }: { id: string; approve: boolean }) => api(`/admin/cancellations/${id}/decide`, { method: "POST", body: { approve } }),
    onSuccess: () => { setError(null); void qc.invalidateQueries({ queryKey: ["admin", "cancellations"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <div className="flex flex-col gap-4">
      <FormError message={error} />
      <DataTable<CancelRequest>
        title={t("nav.cancellations")}
        emptyIcon={XCircleIcon} emptyTitle={t("lifecycle.no_requests")}
        path={`/admin/cancellations?status=${status}`}
        queryKey={["admin", "cancellations", status]}
        toolbar={
          <Select value={status} onValueChange={setStatus}>
            <SelectTrigger className="w-44" aria-label={t("billing.status")}><SelectValue /></SelectTrigger>
            <SelectContent>{STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`lifecycle.cancel_status.${s}`)}</SelectItem>)}</SelectContent>
          </Select>
        }
        columns={[
          { header: t("billing.product"), cell: (r) => r.product_name },
          { header: t("lifecycle.customer"), cell: (r) => r.customer_email },
          { header: t("lifecycle.when"), cell: (r) => tCode(`lifecycle.cancel_kind.${r.kind}`) },
          { header: t("lifecycle.reason"), cell: (r) => <span className="line-clamp-2 max-w-xs text-muted-foreground">{r.reason || "-"}</span> },
          { header: t("billing.status"), cell: (r) => <Badge variant="secondary">{tCode(`lifecycle.cancel_status.${r.status}`)}</Badge> },
          { header: t("lifecycle.effective"), cell: (r) => (r.effective_date ? formatDate(r.effective_date, lang) : formatDate(r.created_at, lang)) },
          { header: "", cell: (r) => r.status === "pending" ? (
            <div className="flex gap-1">
              <ConfirmAction description={t("lifecycle.approve_confirm")} onConfirm={() => decide.mutate({ id: r.id, approve: true })}>
                <Button size="sm" variant="outline" disabled={decide.isPending}>{t("lifecycle.approve")}</Button>
              </ConfirmAction>
              <Button size="sm" variant="ghost" disabled={decide.isPending} onClick={() => decide.mutate({ id: r.id, approve: false })}>{t("lifecycle.reject")}</Button>
            </div>
          ) : null },
        ]}
      />
    </div>
  );
}
