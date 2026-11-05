import { UserAvatar } from "@/components/account/UserAvatar";
import { toneVariant } from "@/lib/utils";
import { DataTable } from "@/components/DataTable";
import { Badge } from "@/components/ui/badge";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, SealCheckIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";

interface User { id: string; email: string; display_name: string; avatar_version: number | null; is_active: boolean; email_verified_at: string | null; created_at: string }

export function AdminUsersPage() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const verify = useMutation({
    mutationFn: (id: string) => api(`/admin/users/${id}/verify-email`, { method: "POST" }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "users"] }),
  });
  return (
    <DataTable<User>
      title={t("admin.users.title")} path="/admin/users" queryKey={["admin", "users"]}
      columns={[
        { header: t("admin.users.email"), cell: (u) => <span className="flex min-w-0 items-center gap-2.5"><UserAvatar size="sm" className="size-8" userId={u.id} version={u.avatar_version} name={u.display_name} email={u.email} /><span className="truncate">{u.email}</span></span>, primary: true },
        { header: t("admin.users.name"), cell: (u) => u.display_name || "-" },
        { header: t("admin.users.status"), cell: (u) => <Badge variant={toneVariant(u.is_active ? "success" : "danger")}>{u.is_active ? t("admin.users.active") : t("admin.users.inactive")}</Badge> },
        { header: t("admin.users.email_status"), cell: (u) => u.email_verified_at
          ? <Badge variant="outline"><SealCheckIcon aria-hidden />{t("admin.users.verified")}</Badge>
          : <Button size="sm" variant="outline" disabled={verify.isPending} onClick={() => verify.mutate(u.id)}><CheckCircleIcon aria-hidden />{t("admin.users.mark_verified")}</Button> },
        { header: t("admin.users.created"), cell: (u) => formatDate(u.created_at, lang) },
      ]}
    />
  );
}
