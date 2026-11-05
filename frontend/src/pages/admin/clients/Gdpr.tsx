import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, DownloadSimpleIcon, ShieldCheckIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ConfirmAction } from "@/components/ConfirmAction";
import { DataTable } from "@/components/DataTable";
import { downloadJson, type GdprRequest } from "@/components/account/PrivacyCard";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

type Row = GdprRequest & { blocker: string | null };
const STATUSES = ["pending", "completed", "rejected", "cancelled"];

export function AdminGdprPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [status, setStatus] = useState("pending");
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const done = () => void qc.invalidateQueries({ queryKey: ["admin", "gdpr"] });
  const approve = useMutation({ mutationFn: (id: string) => api(`/admin/gdpr/requests/${id}/approve`, { method: "POST" }), onSuccess: () => { toast.success(t("gdpr.erased")); done(); }, onError: onErr });
  const reject = useMutation({ mutationFn: (id: string) => api(`/admin/gdpr/requests/${id}/reject`, { method: "POST", body: {} }), onSuccess: done, onError: onErr });
  const exp = useMutation({ mutationFn: (uid: string) => api<unknown>(`/admin/gdpr/users/${uid}/export`).then((d) => downloadJson(d, `peltycms-data-${uid}.json`)), onError: onErr });
  return (
    <DataTable<Row>
      title={t("gdpr.title")} description={t("gdpr.hint")}
      path={`/admin/gdpr/requests?status=${status}`} queryKey={["admin", "gdpr", status]}
      emptyIcon={ShieldCheckIcon} emptyTitle={t("gdpr.empty")}
      toolbar={
        <Select value={status} onValueChange={setStatus}>
          <SelectTrigger className="w-full sm:w-56" aria-label={t("gdpr.status")}><SelectValue /></SelectTrigger>
          <SelectContent>{STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`gdpr.status_${s}`)}</SelectItem>)}</SelectContent>
        </Select>
      }
      columns={[
        { header: t("gdpr.subject"), primary: true, cell: (r) => <span className="break-all font-medium">{r.subject_ref}</span> },
        { header: t("gdpr.requested"), cell: (r) => formatDateTime(r.requested_at, lang) },
        { header: t("gdpr.reason"), cell: (r) => <span className="text-muted-foreground">{r.reason || "-"}</span> },
        { header: t("gdpr.status"), cell: (r) => <span className="flex flex-col gap-1"><Badge variant={r.status === "pending" ? "default" : "secondary"}>{tCode(`gdpr.status_${r.status}`)}</Badge>{r.blocker && <span className="text-xs text-muted-foreground">{tCode(r.blocker)}</span>}</span> },
        { header: "", className: "text-right", cell: (r) => (
          <span className="flex flex-wrap justify-end gap-1">
            {r.user_id && r.status !== "completed" && <Button size="sm" variant="ghost" onClick={() => exp.mutate(r.user_id!)} disabled={exp.isPending}><DownloadSimpleIcon aria-hidden />{t("gdpr.export")}</Button>}
            {r.status === "pending" && (
              <>
                <ConfirmAction destructive description={t("gdpr.confirm_erase", { email: r.subject_ref })} confirmLabel={t("gdpr.approve")} onConfirm={() => approve.mutate(r.id)} disabled={!!r.blocker}>
                  <Button size="sm" variant="ghost" disabled={!!r.blocker || approve.isPending}><CheckIcon aria-hidden />{t("gdpr.approve")}</Button>
                </ConfirmAction>
                <ConfirmAction description={t("gdpr.confirm_reject")} confirmLabel={t("gdpr.reject")} onConfirm={() => reject.mutate(r.id)}>
                  <Button size="sm" variant="ghost" disabled={reject.isPending}><XIcon aria-hidden />{t("gdpr.reject")}</Button>
                </ConfirmAction>
              </>
            )}
          </span>
        ) },
      ]}
    />
  );
}
