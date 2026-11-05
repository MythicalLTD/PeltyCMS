import { useState } from "react";
import { ArrowClockwiseIcon, LockKeyIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface SslCheck { domain_id: string; domain: string; customer_id: string; status: string; issuer: string; valid_from: string | null; expires_at: string | null; error: string; checked_at: string }
export const SSL_STATUSES = ["valid", "expiring", "expired", "invalid", "unreachable"] as const;

export function sslVariant(s: string): "default" | "secondary" | "destructive" | "outline" {
  if (s === "valid") return "default";
  if (s === "expiring") return "secondary";
  if (s === "unreachable") return "outline";
  return "destructive";
}

export function SslBadge({ check }: { check?: SslCheck }) {
  const { t, tCode } = useI18n();
  if (!check) return <span className="text-sm text-muted-foreground">{t("ssl.unchecked")}</span>;
  return <Badge variant={sslVariant(check.status)} title={check.error || check.issuer}>{tCode(`ssl.status.${check.status}`)}</Badge>;
}

/** Admin overview of certificate checks with an on-demand recheck. */
export function AdminSslList() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [status, setStatus] = useState("all");
  const q = useQuery({ queryKey: ["admin", "ssl", status], queryFn: () => api<{ items: SslCheck[] }>(`/admin/ssl?per_page=100${status === "all" ? "" : `&status=${status}`}`) });
  const check = useMutation({
    mutationFn: (id: string) => api<SslCheck>(`/admin/domains/${id}/ssl-check`, { method: "POST" }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["admin", "ssl"] }); toast.success(t("ssl.checked")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <p className="text-sm text-muted-foreground">{t("ssl.admin_hint")}</p>
        <Select value={status} onValueChange={setStatus}>
          <SelectTrigger className="w-full sm:w-48" aria-label={t("billing.status")}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("ssl.all")}</SelectItem>
            {SSL_STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`ssl.status.${s}`)}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      <ResponsiveTable<SslCheck> rows={q.data?.items} rowKey={(x) => x.domain_id} loading={q.isLoading} emptyIcon={LockKeyIcon} emptyTitle={t("ssl.empty")}
        columns={[
          { header: t("domains.name"), primary: true, cell: (x) => <span className="font-medium break-all">{x.domain}</span> },
          { header: t("billing.status"), cell: (x) => <SslBadge check={x} /> },
          { header: t("ssl.expires"), cell: (x) => (x.expires_at ? formatDate(x.expires_at, lang) : "-") },
          { header: t("ssl.issuer"), cell: (x) => <span className="text-sm text-muted-foreground break-all">{x.issuer || x.error || "-"}</span> },
          { header: t("ssl.checked_at"), cell: (x) => formatDate(x.checked_at, lang) },
          { header: "", cell: (x) => <Button size="sm" variant="outline" disabled={check.isPending} onClick={() => check.mutate(x.domain_id)}><ArrowClockwiseIcon aria-hidden />{t("ssl.recheck")}</Button> },
        ]} />
    </div>
  );
}

/** Map of domain id → latest check for the signed-in client. */
export function useClientSsl() {
  const q = useQuery({ queryKey: ["client", "ssl"], queryFn: () => api<{ items: SslCheck[] }>("/client/ssl") });
  return new Map((q.data?.items ?? []).map((c) => [c.domain_id, c]));
}
