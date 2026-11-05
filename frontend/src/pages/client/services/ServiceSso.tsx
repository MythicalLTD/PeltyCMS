import { useMutation } from "@tanstack/react-query";
import { SignInIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { api, ApiError } from "@/lib/api";
import type { Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

/** One-click login into the service's control panel (signed one-time token). */
export function ServiceSsoButton({ service }: { service: Service }) {
  const { t, tCode } = useI18n();
  const go = useMutation({
    mutationFn: () => api<{ url: string }>(`/client/services/${service.id}/sso`, { method: "POST" }),
    onSuccess: (r) => { window.location.assign(r.url); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (service.status !== "active") return null;
  return <Button size="sm" variant="ghost" disabled={go.isPending} onClick={() => go.mutate()}><SignInIcon aria-hidden />{t("sso.open_panel")}</Button>;
}
