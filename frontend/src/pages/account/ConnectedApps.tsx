import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlugsConnectedIcon, ProhibitIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListPage } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface App { id: string; name: string; scopes: string[]; granted_at: string }

export function ConnectedAppsPage() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["oauth-apps"], queryFn: () => api<{ apps: App[] }>("/account/oauth-apps") });
  const revoke = useMutation({
    mutationFn: (id: string) => api(`/account/oauth-apps/${id}`, { method: "DELETE" }),
    onSuccess: () => { toast.success(t("oauth.apps_revoked")); void qc.invalidateQueries({ queryKey: ["oauth-apps"] }); },
  });
  return (
    <ListPage<App>
      title={t("oauth.apps_title")} description={t("oauth.apps_subtitle")}
      rows={data?.apps} rowKey={(a) => a.id} loading={isLoading} emptyIcon={PlugsConnectedIcon} emptyTitle={t("oauth.apps_empty")}
      columns={[
        { header: t("oauth.app"), cell: (a) => <span className="font-medium">{a.name}</span>, primary: true },
        { header: t("oauth.scopes"), cell: (a) => <span className="flex flex-wrap gap-1">{a.scopes.map((s) => <Badge key={s} variant="outline" className="font-mono text-[11px]">{s}</Badge>)}</span> },
        { header: t("oauth.granted"), cell: (a) => formatDateTime(a.granted_at, lang) },
        { header: "", className: "text-right", cell: (a) => (
          <ConfirmAction description={t("oauth.apps_confirm_revoke")} confirmLabel={t("oauth.revoke")} onConfirm={() => revoke.mutate(a.id)}>
            <Button size="sm" variant="ghost"><ProhibitIcon aria-hidden />{t("oauth.revoke")}</Button>
          </ConfirmAction>
        ) },
      ]}
    />
  );
}
