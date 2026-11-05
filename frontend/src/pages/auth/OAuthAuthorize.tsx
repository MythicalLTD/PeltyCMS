import { useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { CheckCircleIcon, ShieldCheckIcon, WarningIcon } from "@phosphor-icons/react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

interface Info {
  redirect_to?: string;
  client?: { name: string; client_id: string; trusted: boolean };
  redirect_host?: string;
  scopes?: string[];
  consent_required?: boolean;
  user?: { email: string; name: string | null };
}

const SCOPE_KEYS: Record<string, TranslationKey> = {
  openid: "oauth.scope_openid",
  profile: "oauth.scope_profile",
  email: "oauth.scope_email",
  offline_access: "oauth.scope_offline_access",
};

/** OAuth consent screen; the user is already signed in (RequireAuth). */
export function OAuthAuthorizePage() {
  const { t, tCode } = useI18n();
  const [sp] = useSearchParams();
  const params = Object.fromEntries(sp.entries());
  const qs = sp.toString();
  const info = useQuery({ queryKey: ["oauth-authorize", qs], queryFn: () => api<Info>(`/oauth/authorize?${qs}`), retry: false });
  const decide = useMutation({
    mutationFn: (approve: boolean) => api<{ redirect_to: string }>("/oauth/authorize", { method: "POST", body: { ...params, approve } }),
    onSuccess: (r) => window.location.assign(r.redirect_to),
  });
  const auto = useRef(false);
  const d = info.data;
  const leaving = !!d?.redirect_to || d?.consent_required === false;
  useEffect(() => {
    if (!d) return;
    if (d.redirect_to) { window.location.assign(d.redirect_to); return; }
    if (d.consent_required === false && !auto.current) { auto.current = true; decide.mutate(true); }
  }, [d, decide]);

  if (info.error) {
    return (
      <Alert variant="destructive" className="mx-auto max-w-md">
        <WarningIcon aria-hidden />
        <AlertTitle>{t("oauth.error_title")}</AlertTitle>
        <AlertDescription>{info.error instanceof ApiError ? tCode(info.error.code) : t("error.internal")}</AlertDescription>
      </Alert>
    );
  }
  if (!d || leaving || d.consent_required === false || !d.client) {
    return <p className="text-center text-sm text-muted-foreground">{t("oauth.redirecting")}</p>;
  }
  return (
    <Card className="mx-auto w-full max-w-md">
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><ShieldCheckIcon aria-hidden />{t("oauth.consent_title", { app: d.client.name })}</CardTitle>
        <CardDescription>{t("oauth.consent_signed_in", { email: d.user?.email ?? "" })}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <p className="text-sm">{t("oauth.consent_wants")}</p>
        <ul className="flex flex-col gap-2">
          {d.scopes?.map((s) => (
            <li key={s} className="flex items-start gap-2 text-sm">
              <CheckCircleIcon aria-hidden className="mt-0.5 shrink-0" />
              <span>{SCOPE_KEYS[s] ? t(SCOPE_KEYS[s]) : s}</span>
            </li>
          ))}
        </ul>
        <p className="text-xs text-muted-foreground">{t("oauth.consent_redirect", { host: d.redirect_host ?? "" })}</p>
      </CardContent>
      <CardFooter className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
        <Button variant="outline" className="w-full sm:w-auto" disabled={decide.isPending} onClick={() => decide.mutate(false)}>{t("oauth.deny")}</Button>
        <Button className="w-full sm:w-auto" disabled={decide.isPending} onClick={() => decide.mutate(true)}>{t("oauth.allow")}</Button>
      </CardFooter>
    </Card>
  );
}
