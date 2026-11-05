import { Link } from "react-router";
import { DownloadSimpleIcon, FolderOpenIcon } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { ListPage } from "@/components/ListPage";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { fileSize, type Download } from "@/lib/p2";
import { useI18n } from "@/lib/i18n";

interface ServiceRow { id: string; status: string }

export function ClientDownloadsPage() {
  const { t, lang } = useI18n();
  const downloads = useQuery({ queryKey: ["client", "downloads"], queryFn: () => api<{ items: Download[] }>("/client/downloads") });
  const services = useQuery({
    queryKey: ["client", "services", "downloads-gate"],
    queryFn: () => api<{ items: ServiceRow[] }>("/client/services?per_page=100"),
    enabled: !!downloads.data && (downloads.data.items?.length ?? 0) === 0,
  });
  const items = downloads.data?.items ?? [];
  const empty = !downloads.isLoading && items.length === 0;
  const hasActive = (services.data?.items ?? []).some((s) => s.status === "active");
  const gated = empty && services.isSuccess && !hasActive;
  return (
    <ListPage<Download>
      title={t("downloads.title")}
      description={t("downloads.hint")}
      rows={items}
      rowKey={(d) => d.id}
      loading={downloads.isLoading}
      emptyIcon={FolderOpenIcon}
      emptyTitle={gated ? t("downloads.no_service") : t("downloads.empty_gated")}
      emptyDescription={gated ? undefined : t("downloads.empty_gated_hint")}
      actions={gated ? <Button asChild size="sm" variant="outline"><Link to="/services">{t("downloads.browse_services")}</Link></Button> : undefined}
      columns={[
        { header: t("downloads.file"), primary: true, cell: (d) => <div><p className="font-medium">{d.title}</p>{d.description && <p className="text-sm text-muted-foreground">{d.description}</p>}</div> },
        { header: t("downloads.size"), cell: (d) => fileSize(d.size_bytes) },
        { header: t("affiliate.date"), cell: (d) => formatDate(d.created_at, lang) },
        { header: "", cell: (d) => <Button asChild size="sm" variant="outline"><a href={`/api/v1/client/downloads/${d.id}`} download={d.filename}><DownloadSimpleIcon aria-hidden />{t("downloads.download")}</a></Button> },
      ]}
    />
  );
}
