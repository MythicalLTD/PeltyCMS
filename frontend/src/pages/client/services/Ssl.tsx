import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { LockKeyIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { SslBadge, type SslCheck } from "@/components/domains/SslStatus";
import { api } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/** Client list of SSL check results for their domains. */
export function ClientSslPage() {
  const { t, lang } = useI18n();
  const q = useQuery({ queryKey: ["client", "ssl"], queryFn: () => api<{ items: SslCheck[] }>("/client/ssl") });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("ssl.title")} description={t("ssl.client_hint")} />
      <ResponsiveTable<SslCheck>
        rows={q.data?.items}
        rowKey={(x) => x.domain_id}
        loading={q.isLoading}
        emptyIcon={LockKeyIcon}
        emptyTitle={t("ssl.empty")}
        columns={[
          { header: t("domains.name"), primary: true, cell: (x) => <Link className="font-medium break-all hover:underline" to={`/domains/${x.domain_id}`}>{x.domain}</Link> },
          { header: t("billing.status"), cell: (x) => <SslBadge check={x} /> },
          { header: t("ssl.expires"), cell: (x) => (x.expires_at ? formatDate(x.expires_at, lang) : "-") },
          { header: t("ssl.issuer"), cell: (x) => <span className="text-sm text-muted-foreground break-all">{x.issuer || x.error || "-"}</span> },
          { header: t("ssl.checked_at"), cell: (x) => formatDate(x.checked_at, lang) },
        ]}
      />
    </div>
  );
}
