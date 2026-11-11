import { useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { EnvelopeSimpleIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface SignupInfo { provider: string; provider_name: string; name: string }

/** Plausible e-mail (the server checks again). */
export function signupEmailOk(v: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v.trim());
}

/**
 * Last step of a game sign-in (Steam, FiveM via Cfx.re): neither shares an
 * e-mail, so the player enters one for receipts and account recovery.
 */
export function SsoSignupCard({ token }: { token: string }) {
  const { t, tCode } = useI18n();
  const [email, setEmail] = useState("");
  const [error, setError] = useState<string | null>(null);
  const info = useQuery({ queryKey: ["sso-signup", token], queryFn: () => api<SignupInfo>(`/auth/oauth/signup/${encodeURIComponent(token)}`), retry: false });
  const go = useMutation({
    mutationFn: () => api<{ redirect: string }>("/auth/oauth/signup", { method: "POST", body: { token, email: email.trim() } }),
    // Full navigation: picks up the new session cookie and CSRF token.
    onSuccess: (r) => window.location.assign(r.redirect.startsWith("/") ? r.redirect : "/dashboard"),
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("common.error")),
  });
  if (info.isError) {
    return (
      <Card>
        <CardHeader><CardTitle>{t("sso.signup_title")}</CardTitle></CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FormError message={tCode("error.oauth_state")} />
          <Button asChild variant="outline"><Link to="/login">{t("sso.signup_back")}</Link></Button>
        </CardContent>
      </Card>
    );
  }
  const provider = info.data?.provider_name || "";
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("sso.signup_title")}</CardTitle>
        <CardDescription>{info.data ? t("sso.signup_desc", { provider, name: info.data.name || provider }) : "\u00a0"}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" noValidate onSubmit={(e) => { e.preventDefault(); setError(null); if (!signupEmailOk(email)) { setError(t("validation.email")); return; } go.mutate(); }}>
          <FormError message={error} />
          <Field id="sso-email" type="email" autoComplete="email" label={t("field.email")} placeholder={t("field.email_placeholder")} value={email} onChange={(e) => setEmail(e.target.value)} />
          <p className="-mt-2 text-xs text-muted-foreground">{t("sso.signup_email_hint", { provider: provider || "—" })}</p>
          <Button type="submit" size="lg" disabled={go.isPending || !info.data}><EnvelopeSimpleIcon aria-hidden />{t("sso.signup_submit")}</Button>
          <p className="text-center text-sm text-muted-foreground">{t("sso.signup_have_account", { provider })} <Link to="/login" className="text-primary hover:underline">{t("sso.signup_back")}</Link></p>
        </form>
      </CardContent>
    </Card>
  );
}
