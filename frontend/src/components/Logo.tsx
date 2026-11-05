import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { Theme } from "@/lib/theme";
import { useI18n } from "@/lib/i18n";
import { safeHref, usePublicSite } from "@/lib/admin/site";

export function Logo() {
  const { t } = useI18n();
  const { data } = useQuery({ queryKey: ["theme", "active"], queryFn: () => api<Theme | null>("/themes/active") });
  const site = usePublicSite().data?.config;
  const logo = safeHref(site?.logo_url) ?? data?.logo_url ?? "/brand/pelty-logo-128.png";
  return (
    <div className="flex items-center gap-2.5">
      <span className="inline-flex size-7 shrink-0 items-center justify-center overflow-hidden rounded-md border border-border bg-white">
        <img src={logo} alt="" className="size-full object-contain" />
      </span>
      <span className="font-bold tracking-tight">{site?.site_name || t("app.name")}</span>
    </div>
  );
}
