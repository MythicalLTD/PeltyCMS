import { useQuery } from "@tanstack/react-query";
import { UserSwitchIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

async function stopImpersonation() {
  const r = await api<{ redirect: string }>("/auth/impersonation/stop", { method: "POST" }).catch(() => ({ redirect: "/login" }));
  window.location.assign(r.redirect);
}

/** Shown while staff is signed in as a client; "Return to admin" restores the staff session. */
export function ImpersonationBanner() {
  const { t } = useI18n();
  const { data: me } = useMe();
  const q = useQuery({ queryKey: ["impersonation", me?.id], queryFn: () => api<{ active: boolean; admin_email?: string }>("/auth/impersonation"), enabled: !!me });
  if (!q.data?.active) return null;
  return (
    <output data-testid="impersonation-banner" className="mb-4 flex flex-wrap items-center gap-2 rounded-md border border-primary/40 bg-primary/10 py-1 pr-1 pl-3 text-sm">
      <UserSwitchIcon aria-hidden className="size-4 shrink-0" />
      <span className="min-w-0 flex-1">{t("impersonation.banner", { email: me?.email ?? "", admin: q.data.admin_email ?? "" })}</span>
      <Button size="sm" variant="outline" onClick={() => void stopImpersonation()}>{t("impersonation.stop")}</Button>
    </output>
  );
}
