import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyIcon, LinkBreakIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { SSO_ICON, SsoButtons, useSsoProviders } from "@/components/account/SsoButtons";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Identity { id: string; provider: string; email: string; username?: string; created_at: string; last_login_at: string | null }

/** Social / SSO identities linked to the account: link more, unlink. */
export function LinkedAccountsCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const providers = useSsoProviders().data?.providers ?? [];
  const ids = useQuery({ queryKey: ["account", "identities"], queryFn: () => api<{ identities: Identity[] }>("/account/identities") });
  const unlink = useMutation({
    mutationFn: (id: string) => api(`/account/identities/${id}`, { method: "DELETE" }),
    onSuccess: () => { toast.success(t("sso.unlinked")); void qc.invalidateQueries({ queryKey: ["account", "identities"] }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const list = ids.data?.identities ?? [];
  if (!providers.length && !list.length) return null;
  return (
    <Card>
      <CardHeader><CardTitle>{t("sso.linked_title")}</CardTitle><CardDescription>{t("sso.linked_hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4">
        {list.length > 0 && (
          <ul className="flex flex-col divide-y rounded-md border">
            {list.map((i) => {
              const p = providers.find((x) => x.id === i.provider);
              const Icon = (p && SSO_ICON[p.engine]) || KeyIcon;
              return (
                <li key={i.id} className="flex flex-wrap items-center gap-3 p-3">
                  <Icon aria-hidden className="size-5 shrink-0" />
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="font-medium">{p?.label ?? i.provider}</span>
                    <span className="truncate text-xs text-muted-foreground">{i.username || i.email || "-"} · {t("sso.linked_at", { date: formatDateTime(i.created_at, lang) })}</span>
                  </span>
                  <Button size="sm" variant="outline" disabled={unlink.isPending} onClick={() => unlink.mutate(i.id)}><LinkBreakIcon />{t("sso.unlink")}</Button>
                </li>
              );
            })}
          </ul>
        )}
        <SsoButtons mode="link" />
      </CardContent>
    </Card>
  );
}
